//! OHLC Spread indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// OHLC Spread — rolling average of `(high - low) / close`, normalizing the
/// bar range by the close price.
///
/// This gives a percentage-of-price measure of intrabar volatility that is
/// comparable across different price levels.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen or
/// if any close is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::OhlcSpread;
/// use fin_primitives::signals::Signal;
///
/// let os = OhlcSpread::new("os", 10).unwrap();
/// assert_eq!(os.period(), 10);
/// ```
pub struct OhlcSpread {
    name: String,
    period: usize,
    spreads: VecDeque<Decimal>,
    sum: Decimal,
}

impl OhlcSpread {
    /// Constructs a new `OhlcSpread`.
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
            spreads: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for OhlcSpread {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.spreads.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if bar.close.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let spread = (bar.range()) / bar.close;
        self.spreads.push_back(spread);
        self.sum += spread;
        if self.spreads.len() > self.period {
            self.sum -= self.spreads.pop_front().unwrap();
        }
        if self.spreads.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let nd = Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(self.sum / nd))
    }

    fn reset(&mut self) {
        self.spreads.clear();
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

    fn bar(h: &str, l: &str, c: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_os_invalid_period() {
        assert!(OhlcSpread::new("os", 0).is_err());
    }

    #[test]
    fn test_os_unavailable_before_warm_up() {
        let mut os = OhlcSpread::new("os", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(os.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_os_constant_spread() {
        // range=10, close=100 → spread=0.1 each bar → avg=0.1
        let mut os = OhlcSpread::new("os", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            last = os.update_bar(&bar("110", "100", "100")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(0.1)));
    }

    #[test]
    fn test_os_reset() {
        let mut os = OhlcSpread::new("os", 3).unwrap();
        for _ in 0..3 { os.update_bar(&bar("110", "90", "100")).unwrap(); }
        assert!(os.is_ready());
        os.reset();
        assert!(!os.is_ready());
    }
}
