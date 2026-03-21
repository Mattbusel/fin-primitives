//! High-Minus-PrevClose indicator.
//!
//! Tracks the EMA of `(high[t] - close[t-1])`, measuring the strength of each
//! bar's upside extension beyond the previous bar's close.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// EMA of `high[t] − close[t−1]`.
///
/// - **Positive**: each bar's high consistently extends above the prior close —
///   strong upward range extension / gap-up tendency.
/// - **Small positive**: slight upward extension, normal bullish drift.
/// - **Near zero or negative**: the bar's high rarely exceeds the prior close
///   (possible downward gap regime or range contraction).
///
/// Returns [`SignalValue::Unavailable`] on the first bar (no prior close exists).
/// `is_ready()` returns `true` after the second bar.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::HighMinusPrevClose;
/// use fin_primitives::signals::Signal;
///
/// let hmpc = HighMinusPrevClose::new("hmpc", 10).unwrap();
/// assert_eq!(hmpc.period(), 10);
/// assert!(!hmpc.is_ready());
/// ```
pub struct HighMinusPrevClose {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    ema: Option<Decimal>,
    k: Decimal,
    seen_bars: usize,
}

impl HighMinusPrevClose {
    /// Constructs a new `HighMinusPrevClose`.
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
            prev_close: None,
            ema: None,
            k,
            seen_bars: 0,
        })
    }
}

impl crate::signals::Signal for HighMinusPrevClose {
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

        let result = if let Some(pc) = self.prev_close {
            let delta = bar.high - pc;
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

        self.prev_close = Some(bar.close);
        Ok(result)
    }

    fn reset(&mut self) {
        self.prev_close = None;
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
    fn test_hmpc_invalid_period() {
        assert!(HighMinusPrevClose::new("hmpc", 0).is_err());
    }

    #[test]
    fn test_hmpc_first_bar_unavailable() {
        let mut hmpc = HighMinusPrevClose::new("hmpc", 5).unwrap();
        assert_eq!(hmpc.update_bar(&bar("110", "105")).unwrap(), SignalValue::Unavailable);
        assert!(!hmpc.is_ready());
    }

    #[test]
    fn test_hmpc_ready_after_second_bar() {
        let mut hmpc = HighMinusPrevClose::new("hmpc", 5).unwrap();
        hmpc.update_bar(&bar("110", "105")).unwrap();
        hmpc.update_bar(&bar("115", "112")).unwrap();
        assert!(hmpc.is_ready());
    }

    #[test]
    fn test_hmpc_positive_extension() {
        let mut hmpc = HighMinusPrevClose::new("hmpc", 5).unwrap();
        hmpc.update_bar(&bar("110", "105")).unwrap(); // close=105
        // high=120 > 105 → delta = 15
        let v = hmpc.update_bar(&bar("120", "115")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(15)));
    }

    #[test]
    fn test_hmpc_no_extension_negative() {
        let mut hmpc = HighMinusPrevClose::new("hmpc", 5).unwrap();
        hmpc.update_bar(&bar("110", "108")).unwrap(); // close=108
        // high=105 < 108 → delta = -3
        let v = hmpc.update_bar(&bar("105", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-3)));
    }

    #[test]
    fn test_hmpc_reset() {
        let mut hmpc = HighMinusPrevClose::new("hmpc", 5).unwrap();
        hmpc.update_bar(&bar("110", "105")).unwrap();
        hmpc.update_bar(&bar("115", "112")).unwrap();
        assert!(hmpc.is_ready());
        hmpc.reset();
        assert!(!hmpc.is_ready());
    }
}
