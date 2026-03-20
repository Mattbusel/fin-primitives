//! ROC Ratio indicator — momentum acceleration/deceleration signal.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// ROC Ratio — ratio of short-term ROC to long-term ROC.
///
/// ```text
/// short_roc = (close - close[fast]) / close[fast] × 100
/// long_roc  = (close - close[slow]) / close[slow] × 100
/// ratio     = short_roc / long_roc   (or 0 when long_roc == 0)
/// ```
///
/// - Values > 1 indicate momentum acceleration (short ROC outpacing long ROC).
/// - Values < 1 indicate deceleration.
/// - Sign reflects directional agreement between fast and slow.
///
/// Returns [`SignalValue::Unavailable`] until `slow + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RocRatio;
/// use fin_primitives::signals::Signal;
/// let r = RocRatio::new("rr", 5, 20).unwrap();
/// assert_eq!(r.period(), 20);
/// ```
pub struct RocRatio {
    name: String,
    fast: usize,
    slow: usize,
    closes: VecDeque<Decimal>,
}

impl RocRatio {
    /// Constructs a new `RocRatio`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `fast == 0`, `slow == 0`, or `fast >= slow`.
    pub fn new(name: impl Into<String>, fast: usize, slow: usize) -> Result<Self, FinError> {
        if fast == 0 { return Err(FinError::InvalidPeriod(fast)); }
        if slow == 0 { return Err(FinError::InvalidPeriod(slow)); }
        if fast >= slow { return Err(FinError::InvalidPeriod(fast)); }
        Ok(Self {
            name: name.into(),
            fast,
            slow,
            closes: VecDeque::with_capacity(slow + 1),
        })
    }
}

impl Signal for RocRatio {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.slow + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() < self.slow + 1 {
            return Ok(SignalValue::Unavailable);
        }

        let current = *self.closes.back().unwrap();
        let len = self.closes.len();

        let fast_prev = self.closes[len - 1 - self.fast];
        let slow_prev = self.closes[len - 1 - self.slow];

        if fast_prev.is_zero() || slow_prev.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let short_roc = (current - fast_prev)
            .checked_div(fast_prev)
            .ok_or(FinError::ArithmeticOverflow)?
            * Decimal::ONE_HUNDRED;

        let long_roc = (current - slow_prev)
            .checked_div(slow_prev)
            .ok_or(FinError::ArithmeticOverflow)?
            * Decimal::ONE_HUNDRED;

        if long_roc.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let ratio = short_roc
            .checked_div(long_roc)
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(ratio))
    }

    fn is_ready(&self) -> bool { self.closes.len() >= self.slow + 1 }
    fn period(&self) -> usize { self.slow }
    fn reset(&mut self) { self.closes.clear(); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};

    fn bar(c: &str) -> OhlcvBar {
        let p = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_roc_ratio_invalid_params() {
        assert!(RocRatio::new("r", 0, 20).is_err());
        assert!(RocRatio::new("r", 20, 5).is_err()); // fast >= slow
        assert!(RocRatio::new("r", 5, 0).is_err());
    }

    #[test]
    fn test_roc_ratio_unavailable_before_warmup() {
        let mut r = RocRatio::new("r", 2, 5).unwrap();
        for _ in 0..5 {
            assert_eq!(r.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!r.is_ready());
    }

    #[test]
    fn test_roc_ratio_ready_after_warmup() {
        let mut r = RocRatio::new("r", 2, 5).unwrap();
        let prices: Vec<String> = (100..107).map(|i| i.to_string()).collect();
        let mut last = SignalValue::Unavailable;
        for p in &prices {
            last = r.update_bar(&bar(p)).unwrap();
        }
        assert!(r.is_ready());
        assert!(matches!(last, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_roc_ratio_period_is_slow() {
        let r = RocRatio::new("r", 3, 10).unwrap();
        assert_eq!(r.period(), 10);
    }

    #[test]
    fn test_roc_ratio_reset() {
        let mut r = RocRatio::new("r", 2, 5).unwrap();
        for p in &["100", "101", "102", "103", "104", "105"] {
            r.update_bar(&bar(p)).unwrap();
        }
        assert!(r.is_ready());
        r.reset();
        assert!(!r.is_ready());
    }
}
