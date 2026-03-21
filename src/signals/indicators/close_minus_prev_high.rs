//! Close-Minus-PrevHigh indicator.
//!
//! Tracks the EMA of `(close[t] - high[t-1])`, measuring whether the current
//! close consistently breaks above (positive) or fails to reach (negative) the
//! prior bar's high.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// EMA of `close[t] − high[t−1]`.
///
/// - **Positive**: the close persistently exceeds the prior bar's high — strong
///   follow-through breakout behavior.
/// - **Negative**: the close persistently fails to reach the prior bar's high —
///   the market is not extending upward.
/// - **Near zero**: closes are hovering right at the prior bar's high.
///
/// Returns [`SignalValue::Unavailable`] on the first bar (no prior high exists).
/// `is_ready()` returns `true` after the second bar.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseMinusPrevHigh;
/// use fin_primitives::signals::Signal;
///
/// let cmph = CloseMinusPrevHigh::new("cmph", 10).unwrap();
/// assert_eq!(cmph.period(), 10);
/// assert!(!cmph.is_ready());
/// ```
pub struct CloseMinusPrevHigh {
    name: String,
    period: usize,
    prev_high: Option<Decimal>,
    ema: Option<Decimal>,
    k: Decimal,
    seen_bars: usize,
}

impl CloseMinusPrevHigh {
    /// Constructs a new `CloseMinusPrevHigh`.
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
            prev_high: None,
            ema: None,
            k,
            seen_bars: 0,
        })
    }
}

impl crate::signals::Signal for CloseMinusPrevHigh {
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

        let result = if let Some(ph) = self.prev_high {
            let delta = bar.close - ph;
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

        self.prev_high = Some(bar.high);
        Ok(result)
    }

    fn reset(&mut self) {
        self.prev_high = None;
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

    fn bar(high: &str, close: &str) -> OhlcvBar {
        let h = Price::new(high.parse().unwrap()).unwrap();
        let c = Price::new(close.parse().unwrap()).unwrap();
        let low = if c < h { c } else { h };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: low, high: h, low, close: c,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_cmph_invalid_period() {
        assert!(CloseMinusPrevHigh::new("cmph", 0).is_err());
    }

    #[test]
    fn test_cmph_first_bar_unavailable() {
        let mut cmph = CloseMinusPrevHigh::new("cmph", 5).unwrap();
        assert_eq!(cmph.update_bar(&bar("110", "105")).unwrap(), SignalValue::Unavailable);
        assert!(!cmph.is_ready());
    }

    #[test]
    fn test_cmph_ready_after_second_bar() {
        let mut cmph = CloseMinusPrevHigh::new("cmph", 5).unwrap();
        cmph.update_bar(&bar("110", "105")).unwrap();
        cmph.update_bar(&bar("115", "112")).unwrap();
        assert!(cmph.is_ready());
    }

    #[test]
    fn test_cmph_close_above_prev_high_positive() {
        let mut cmph = CloseMinusPrevHigh::new("cmph", 5).unwrap();
        cmph.update_bar(&bar("110", "105")).unwrap(); // high=110
        // close=115 > 110 → delta = 5
        let v = cmph.update_bar(&bar("120", "115")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(5)));
    }

    #[test]
    fn test_cmph_close_below_prev_high_negative() {
        let mut cmph = CloseMinusPrevHigh::new("cmph", 5).unwrap();
        cmph.update_bar(&bar("110", "105")).unwrap(); // high=110
        // close=100 < 110 → delta = -10
        let v = cmph.update_bar(&bar("108", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-10)));
    }

    #[test]
    fn test_cmph_reset() {
        let mut cmph = CloseMinusPrevHigh::new("cmph", 5).unwrap();
        cmph.update_bar(&bar("110", "105")).unwrap();
        cmph.update_bar(&bar("115", "112")).unwrap();
        assert!(cmph.is_ready());
        cmph.reset();
        assert!(!cmph.is_ready());
    }
}
