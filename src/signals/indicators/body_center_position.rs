//! Body Center Position indicator.
//!
//! Tracks where, on average, the body of each bar is positioned within the
//! bar's range. Smoothed via EMA.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Body Center Position — EMA of `(body_center - low) / range`.
///
/// For each bar the raw position is:
/// ```text
/// body_center = (max(open, close) + min(open, close)) / 2
/// position    = (body_center - low) / (high - low)   when high > low
///             = 0.5                                   when high == low (flat bar)
/// ```
///
/// This ranges from `0` (body centred at the low) to `1` (body centred at the
/// high). An EMA over `period` bars smooths noise.
///
/// - **Near 1.0**: bodies cluster near the top of the range — sustained buying
///   pressure and bullish commitment.
/// - **Near 0.0**: bodies cluster near the bottom — bearish commitment.
/// - **0.5**: bodies are centred in the range on average — no positional bias.
///
/// Returns a value from the first bar (EMA seeds with first observation).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BodyCenterPosition;
/// use fin_primitives::signals::Signal;
/// let bcp = BodyCenterPosition::new("bcp_14", 14).unwrap();
/// assert_eq!(bcp.period(), 14);
/// ```
pub struct BodyCenterPosition {
    name: String,
    period: usize,
    ema: Option<Decimal>,
    k: Decimal,
}

impl BodyCenterPosition {
    /// Constructs a new `BodyCenterPosition`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        #[allow(clippy::cast_possible_truncation)]
        let k = Decimal::from(2u32) / (Decimal::from(period as u32) + Decimal::ONE);
        Ok(Self { name: name.into(), period, ema: None, k })
    }
}

impl Signal for BodyCenterPosition {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.ema.is_some() }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        let raw = if range.is_zero() {
            // Flat bar: body centre is at midpoint = 0.5
            Decimal::new(5, 1)
        } else {
            let body_top = bar.open.max(bar.close);
            let body_bot = bar.open.min(bar.close);
            let body_center = (body_top + body_bot)
                .checked_div(Decimal::from(2u32))
                .ok_or(FinError::ArithmeticOverflow)?;
            (body_center - bar.low)
                .checked_div(range)
                .ok_or(FinError::ArithmeticOverflow)?
        };

        let ema = match self.ema {
            None => {
                self.ema = Some(raw);
                raw
            }
            Some(prev) => {
                let next = raw * self.k + prev * (Decimal::ONE - self.k);
                self.ema = Some(next);
                next
            }
        };

        Ok(SignalValue::Scalar(ema))
    }

    fn reset(&mut self) {
        self.ema = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, h: &str, l: &str, c: &str) -> OhlcvBar {
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(o.parse().unwrap()).unwrap(),
            high: Price::new(h.parse().unwrap()).unwrap(),
            low: Price::new(l.parse().unwrap()).unwrap(),
            close: Price::new(c.parse().unwrap()).unwrap(),
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_bcp_invalid_period() {
        assert!(BodyCenterPosition::new("bcp", 0).is_err());
    }

    #[test]
    fn test_bcp_ready_after_first_bar() {
        let mut bcp = BodyCenterPosition::new("bcp", 5).unwrap();
        bcp.update_bar(&bar("100", "110", "90", "105")).unwrap();
        assert!(bcp.is_ready());
    }

    #[test]
    fn test_bcp_body_at_top_near_one() {
        // open=100, high=110, low=90, close=108 → body_top=108, body_bot=100
        // body_center = 104, position = (104-90)/20 = 0.7
        let mut bcp = BodyCenterPosition::new("bcp", 5).unwrap();
        let v = bcp.update_bar(&bar("100", "110", "90", "108")).unwrap();
        if let SignalValue::Scalar(val) = v {
            assert!(val > dec!(0.5), "body near top → > 0.5: {val}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_bcp_body_at_bottom_near_zero() {
        // open=100, high=110, low=90, close=92 → body_top=100, body_bot=92
        // body_center = 96, position = (96-90)/20 = 0.3
        let mut bcp = BodyCenterPosition::new("bcp", 5).unwrap();
        let v = bcp.update_bar(&bar("100", "110", "90", "92")).unwrap();
        if let SignalValue::Scalar(val) = v {
            assert!(val < dec!(0.5), "body near bottom → < 0.5: {val}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_bcp_flat_bar_half() {
        // Flat bar (high == low): position = 0.5
        let mut bcp = BodyCenterPosition::new("bcp", 5).unwrap();
        let v = bcp.update_bar(&bar("100", "100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0.5)));
    }

    #[test]
    fn test_bcp_persistent_upper_bodies() {
        let mut bcp = BodyCenterPosition::new("bcp", 3).unwrap();
        // open=95, high=110, low=90, close=108:
        // body_top=108, body_bot=95, center=101.5, position=(101.5-90)/20=0.575 > 0.5
        for _ in 0..6 {
            bcp.update_bar(&bar("95", "110", "90", "108")).unwrap();
        }
        if let SignalValue::Scalar(v) = bcp.update_bar(&bar("95", "110", "90", "108")).unwrap() {
            assert!(v > dec!(0.5), "persistent upper bodies → EMA > 0.5: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_bcp_reset() {
        let mut bcp = BodyCenterPosition::new("bcp", 5).unwrap();
        bcp.update_bar(&bar("100", "110", "90", "105")).unwrap();
        assert!(bcp.is_ready());
        bcp.reset();
        assert!(!bcp.is_ready());
    }
}
