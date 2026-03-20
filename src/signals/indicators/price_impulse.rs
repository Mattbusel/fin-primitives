//! Price Impulse — rolling sum of signed volume-weighted bar moves.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Impulse — `sum((close - open) * volume)` over the last `period` bars.
///
/// Combines directional price move with volume for each bar:
/// - Bullish bars (close > open) contribute positively.
/// - Bearish bars (close < open) contribute negatively.
/// - Large-volume bars have proportionally more influence.
///
/// Useful as a momentum measure that accounts for volume conviction.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceImpulse;
/// use fin_primitives::signals::Signal;
/// let pi = PriceImpulse::new("impulse_10", 10).unwrap();
/// assert_eq!(pi.period(), 10);
/// ```
pub struct PriceImpulse {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl PriceImpulse {
    /// Constructs a new `PriceImpulse`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for PriceImpulse {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let impulse = (bar.close - bar.open) * bar.volume;
        self.sum += impulse;
        self.window.push_back(impulse);
        if self.window.len() > self.period {
            let removed = self.window.pop_front().unwrap();
            self.sum -= removed;
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(self.sum))
    }

    fn reset(&mut self) {
        self.window.clear();
        self.sum = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, c: &str, vol: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: cp.max(op), low: cp.min(op), close: cp,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_pi_invalid_period() {
        assert!(PriceImpulse::new("pi", 0).is_err());
    }

    #[test]
    fn test_pi_unavailable_before_period() {
        let mut s = PriceImpulse::new("pi", 3).unwrap();
        assert_eq!(s.update_bar(&bar("100", "105", "1000")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("105", "110", "1000")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_pi_all_bullish_positive() {
        let mut s = PriceImpulse::new("pi", 2).unwrap();
        s.update_bar(&bar("100", "105", "1000")).unwrap(); // +5000
        let v = s.update_bar(&bar("105", "110", "2000")).unwrap(); // +10000; sum=15000
        assert_eq!(v, SignalValue::Scalar(dec!(15000)));
    }

    #[test]
    fn test_pi_bearish_bars_negative() {
        let mut s = PriceImpulse::new("pi", 2).unwrap();
        s.update_bar(&bar("105", "100", "1000")).unwrap(); // -5000
        let v = s.update_bar(&bar("100", "95", "1000")).unwrap(); // -5000; sum=-10000
        if let SignalValue::Scalar(r) = v {
            assert!(r < dec!(0), "bearish impulse should be negative: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_pi_doji_zero() {
        let mut s = PriceImpulse::new("pi", 1).unwrap();
        let v = s.update_bar(&bar("100", "100", "5000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_pi_reset() {
        let mut s = PriceImpulse::new("pi", 2).unwrap();
        s.update_bar(&bar("100", "105", "1000")).unwrap();
        s.update_bar(&bar("105", "110", "1000")).unwrap();
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
