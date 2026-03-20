//! Fibonacci Retracement Levels indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Fibonacci Retracement Levels — computes the six classic Fibonacci retracement
/// prices from the swing high and low of the last `period` bars.
///
/// Standard levels: 0%, 23.6%, 38.2%, 50%, 61.8%, 100%.
///
/// The scalar output is the **61.8%** level (the "golden ratio" retracement),
/// which is the most widely watched by practitioners. Use [`FibonacciRetrace::levels`]
/// for all six levels.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::FibonacciRetrace;
/// use fin_primitives::signals::Signal;
///
/// let f = FibonacciRetrace::new("fib", 20).unwrap();
/// assert_eq!(f.period(), 20);
/// assert!(!f.is_ready());
/// ```
pub struct FibonacciRetrace {
    name: String,
    period: usize,
    window: VecDeque<BarInput>,
    /// Cached levels (level_0, level_236, level_382, level_500, level_618, level_1000).
    cached: Option<(Decimal, Decimal, Decimal, Decimal, Decimal, Decimal)>,
}

impl FibonacciRetrace {
    // Fibonacci ratios stored as exact string fractions converted at construction.
    const R236: &'static str = "0.236";
    const R382: &'static str = "0.382";
    const R618: &'static str = "0.618";

    /// Constructs a new `FibonacciRetrace` with the given lookback `period`.
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
            cached: None,
        })
    }

    /// Returns all six Fibonacci levels `(F0, F23.6, F38.2, F50, F61.8, F100)`,
    /// or `None` if not yet ready.
    ///
    /// Levels are relative to the window's swing low (F0) and swing high (F100).
    pub fn levels(&self) -> Option<(Decimal, Decimal, Decimal, Decimal, Decimal, Decimal)> {
        self.cached
    }

    /// Returns the 23.6% retracement level, or `None` if not ready.
    pub fn level_236(&self) -> Option<Decimal> {
        self.cached.map(|(_, l, _, _, _, _)| l)
    }

    /// Returns the 38.2% retracement level, or `None` if not ready.
    pub fn level_382(&self) -> Option<Decimal> {
        self.cached.map(|(_, _, l, _, _, _)| l)
    }

    /// Returns the 50.0% retracement level, or `None` if not ready.
    pub fn level_500(&self) -> Option<Decimal> {
        self.cached.map(|(_, _, _, l, _, _)| l)
    }

    /// Returns the 61.8% retracement level (golden ratio), or `None` if not ready.
    pub fn level_618(&self) -> Option<Decimal> {
        self.cached.map(|(_, _, _, _, l, _)| l)
    }

    fn compute(window: &VecDeque<BarInput>) -> Result<(Decimal, Decimal, Decimal, Decimal, Decimal, Decimal), FinError> {
        let swing_high = window.iter().map(|b| b.high).fold(Decimal::MIN, Decimal::max);
        let swing_low  = window.iter().map(|b| b.low).fold(Decimal::MAX, Decimal::min);
        let range = swing_high - swing_low;
        let r236 = Decimal::from_str_exact(Self::R236).map_err(|_| FinError::ArithmeticOverflow)?;
        let r382 = Decimal::from_str_exact(Self::R382).map_err(|_| FinError::ArithmeticOverflow)?;
        let r618 = Decimal::from_str_exact(Self::R618).map_err(|_| FinError::ArithmeticOverflow)?;
        let f0    = swing_low;
        let f236  = swing_high - range * r236;
        let f382  = swing_high - range * r382;
        let f500  = swing_low + range / Decimal::TWO;
        let f618  = swing_high - range * r618;
        let f1000 = swing_high;
        Ok((f0, f236, f382, f500, f618, f1000))
    }
}

impl Signal for FibonacciRetrace {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(*bar);
        if self.window.len() > self.period {
            self.window.pop_front();
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let levels = Self::compute(&self.window)?;
        let f618 = levels.4;
        self.cached = Some(levels);
        Ok(SignalValue::Scalar(f618))
    }

    fn is_ready(&self) -> bool {
        self.cached.is_some()
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.window.clear();
        self.cached = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: hp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_fib_period_zero_fails() {
        assert!(FibonacciRetrace::new("f", 0).is_err());
    }

    #[test]
    fn test_fib_unavailable_before_period() {
        let mut f = FibonacciRetrace::new("f", 3).unwrap();
        assert_eq!(f.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert_eq!(f.update_bar(&bar("112", "88")).unwrap(), SignalValue::Unavailable);
        assert!(!f.is_ready());
    }

    #[test]
    fn test_fib_levels_correct() {
        let mut f = FibonacciRetrace::new("f", 3).unwrap();
        // swing_high=120, swing_low=80, range=40
        f.update_bar(&bar("120", "90")).unwrap();
        f.update_bar(&bar("110", "80")).unwrap();
        let v = f.update_bar(&bar("115", "85")).unwrap();
        assert!(matches!(v, SignalValue::Scalar(_)));
        let (f0, f236, f382, f500, f618, f1000) = f.levels().unwrap();
        assert_eq!(f0,    dec!(80));
        assert_eq!(f1000, dec!(120));
        assert_eq!(f500,  dec!(100));
        // f618 = 120 - 40 * 0.618 = 120 - 24.72 = 95.28
        assert_eq!(f618, dec!(120) - dec!(40) * dec!(0.618));
        // f236 = 120 - 40 * 0.236 = 120 - 9.44 = 110.56
        assert_eq!(f236, dec!(120) - dec!(40) * dec!(0.236));
        // f382 = 120 - 40 * 0.382 = 120 - 15.28 = 104.72
        assert_eq!(f382, dec!(120) - dec!(40) * dec!(0.382));
    }

    #[test]
    fn test_fib_scalar_is_618_level() {
        let mut f = FibonacciRetrace::new("f", 2).unwrap();
        f.update_bar(&bar("100", "80")).unwrap();
        let v = f.update_bar(&bar("100", "80")).unwrap();
        // range=20, f618 = 100 - 20*0.618 = 100 - 12.36 = 87.64
        assert_eq!(v, SignalValue::Scalar(f.level_618().unwrap()));
    }

    #[test]
    fn test_fib_reset() {
        let mut f = FibonacciRetrace::new("f", 2).unwrap();
        f.update_bar(&bar("110", "90")).unwrap();
        f.update_bar(&bar("110", "90")).unwrap();
        assert!(f.is_ready());
        f.reset();
        assert!(!f.is_ready());
        assert!(f.levels().is_none());
    }

    #[test]
    fn test_fib_accessors_before_ready_return_none() {
        let f = FibonacciRetrace::new("f", 5).unwrap();
        assert!(f.level_236().is_none());
        assert!(f.level_382().is_none());
        assert!(f.level_500().is_none());
        assert!(f.level_618().is_none());
    }
}
