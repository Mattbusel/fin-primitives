//! Close-Above-PrevLow indicator.
//!
//! Tracks the EMA of `(close[t] - low[t-1])`, measuring how far the current
//! close is above the prior bar's low — a measure of upside follow-through
//! from support.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// EMA of `close[t] − low[t−1]`.
///
/// - **Large positive**: the close is consistently well above the prior bar's
///   low — strong upside follow-through from support levels.
/// - **Small positive**: close is just above the prior low — marginal strength.
/// - **Negative**: the close is consistently closing below the prior bar's low —
///   breakdown below support, bearish.
///
/// Returns [`SignalValue::Unavailable`] on the first bar. `is_ready()` is `true`
/// after the second bar.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseAbovePrevLow;
/// use fin_primitives::signals::Signal;
///
/// let capl = CloseAbovePrevLow::new("capl", 10).unwrap();
/// assert_eq!(capl.period(), 10);
/// assert!(!capl.is_ready());
/// ```
pub struct CloseAbovePrevLow {
    name: String,
    period: usize,
    prev_low: Option<Decimal>,
    ema: Option<Decimal>,
    k: Decimal,
    seen_bars: usize,
}

impl CloseAbovePrevLow {
    /// Constructs a new `CloseAbovePrevLow`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        #[allow(clippy::cast_possible_truncation)]
        let k = Decimal::from(2u32) / (Decimal::from(period as u32) + Decimal::ONE);
        Ok(Self {
            name: name.into(),
            period,
            prev_low: None,
            ema: None,
            k,
            seen_bars: 0,
        })
    }
}

impl crate::signals::Signal for CloseAbovePrevLow {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.seen_bars >= 2
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.seen_bars += 1;

        let result = if let Some(pl) = self.prev_low {
            let delta = bar.close - pl;
            let ema = match self.ema {
                None => { self.ema = Some(delta); delta }
                Some(prev) => {
                    let next = delta * self.k + prev * (Decimal::ONE - self.k);
                    self.ema = Some(next);
                    next
                }
            };
            SignalValue::Scalar(ema)
        } else {
            SignalValue::Unavailable
        };

        self.prev_low = Some(bar.low);
        Ok(result)
    }

    fn reset(&mut self) {
        self.prev_low = None;
        self.ema = None;
        self.seen_bars = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(low: &str, close: &str) -> OhlcvBar {
        let l = Price::new(low.parse().unwrap()).unwrap();
        let c = Price::new(close.parse().unwrap()).unwrap();
        let high = if c > l { c } else { l };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: l, high, low: l, close: c,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_capl_invalid_period() {
        assert!(CloseAbovePrevLow::new("capl", 0).is_err());
    }

    #[test]
    fn test_capl_first_bar_unavailable() {
        let mut capl = CloseAbovePrevLow::new("capl", 5).unwrap();
        assert_eq!(capl.update_bar(&bar("90", "105")).unwrap(), SignalValue::Unavailable);
        assert!(!capl.is_ready());
    }

    #[test]
    fn test_capl_ready_after_second_bar() {
        let mut capl = CloseAbovePrevLow::new("capl", 5).unwrap();
        capl.update_bar(&bar("90", "105")).unwrap();
        capl.update_bar(&bar("88", "102")).unwrap();
        assert!(capl.is_ready());
    }

    #[test]
    fn test_capl_close_above_prev_low_positive() {
        let mut capl = CloseAbovePrevLow::new("capl", 5).unwrap();
        capl.update_bar(&bar("90", "105")).unwrap(); // low=90
        // close=100 > 90 → delta = 10
        let v = capl.update_bar(&bar("88", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(10)));
    }

    #[test]
    fn test_capl_close_below_prev_low_negative() {
        let mut capl = CloseAbovePrevLow::new("capl", 5).unwrap();
        capl.update_bar(&bar("90", "105")).unwrap(); // low=90
        // close=85 < 90 → delta = -5 (breakdown)
        let v = capl.update_bar(&bar("80", "85")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-5)));
    }

    #[test]
    fn test_capl_reset() {
        let mut capl = CloseAbovePrevLow::new("capl", 5).unwrap();
        capl.update_bar(&bar("90", "105")).unwrap();
        capl.update_bar(&bar("88", "102")).unwrap();
        assert!(capl.is_ready());
        capl.reset();
        assert!(!capl.is_ready());
    }
}
