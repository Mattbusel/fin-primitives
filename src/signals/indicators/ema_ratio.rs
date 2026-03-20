//! EMA Ratio — fast EMA relative to slow EMA, expressed as a fraction.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// EMA Ratio — `fast_ema / slow_ema - 1`.
///
/// Measures how far the fast EMA has moved relative to the slow EMA. Positive
/// values indicate the fast EMA is above the slow EMA (bullish bias); negative
/// values indicate it is below (bearish bias). A value of zero means the two
/// EMAs are equal.
///
/// Useful as a continuous trend-strength measure that avoids the scale dependency
/// of raw EMA crossover signals.
///
/// Returns [`SignalValue::Unavailable`] until both EMAs have warmed up (slow EMA
/// requires `slow_period` bars), or when the slow EMA is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::EmaRatio;
/// use fin_primitives::signals::Signal;
/// let er = EmaRatio::new("er_5_20", 5, 20).unwrap();
/// assert_eq!(er.period(), 20);
/// ```
pub struct EmaRatio {
    name: String,
    fast_period: usize,
    slow_period: usize,
    fast_ema: Option<Decimal>,
    slow_ema: Option<Decimal>,
    fast_k: Decimal,
    slow_k: Decimal,
    bars_seen: usize,
}

impl EmaRatio {
    /// Constructs a new `EmaRatio`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `fast_period == 0`, `slow_period == 0`,
    /// or `fast_period >= slow_period`.
    pub fn new(name: impl Into<String>, fast_period: usize, slow_period: usize) -> Result<Self, FinError> {
        if fast_period == 0 || slow_period == 0 {
            return Err(FinError::InvalidPeriod(fast_period.min(slow_period)));
        }
        if fast_period >= slow_period {
            return Err(FinError::InvalidPeriod(fast_period));
        }
        let fast_k = Decimal::from(2u32) / Decimal::from((fast_period + 1) as u64);
        let slow_k = Decimal::from(2u32) / Decimal::from((slow_period + 1) as u64);
        Ok(Self {
            name: name.into(),
            fast_period,
            slow_period,
            fast_ema: None,
            slow_ema: None,
            fast_k,
            slow_k,
            bars_seen: 0,
        })
    }

    /// Returns the fast EMA period.
    pub fn fast_period(&self) -> usize {
        self.fast_period
    }

    /// Returns the slow EMA period.
    pub fn slow_period(&self) -> usize {
        self.slow_period
    }
}

impl Signal for EmaRatio {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.slow_period
    }

    fn is_ready(&self) -> bool {
        self.bars_seen >= self.slow_period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let close = bar.close;
        self.bars_seen += 1;

        self.fast_ema = Some(match self.fast_ema {
            None => close,
            Some(prev) => prev + self.fast_k * (close - prev),
        });

        self.slow_ema = Some(match self.slow_ema {
            None => close,
            Some(prev) => prev + self.slow_k * (close - prev),
        });

        if self.bars_seen < self.slow_period {
            return Ok(SignalValue::Unavailable);
        }

        let fast = self.fast_ema.unwrap();
        let slow = self.slow_ema.unwrap();

        if slow.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let ratio = fast.checked_div(slow).ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(ratio - Decimal::ONE))
    }

    fn reset(&mut self) {
        self.fast_ema = None;
        self.slow_ema = None;
        self.bars_seen = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(close: &str) -> OhlcvBar {
        let p = Price::new(close.parse().unwrap()).unwrap();
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
    fn test_er_invalid_period() {
        assert!(EmaRatio::new("er", 0, 20).is_err());
        assert!(EmaRatio::new("er", 20, 0).is_err());
        assert!(EmaRatio::new("er", 20, 20).is_err()); // fast >= slow
        assert!(EmaRatio::new("er", 20, 5).is_err());  // fast > slow
    }

    #[test]
    fn test_er_unavailable_before_slow_period() {
        let mut er = EmaRatio::new("er", 3, 5).unwrap();
        for _ in 0..4 {
            assert_eq!(er.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!er.is_ready());
    }

    #[test]
    fn test_er_constant_prices_zero_ratio() {
        let mut er = EmaRatio::new("er", 3, 5).unwrap();
        for _ in 0..10 {
            er.update_bar(&bar("100")).unwrap();
        }
        // Constant price → fast_ema = slow_ema = 100 → ratio = 0
        if let SignalValue::Scalar(v) = er.update_bar(&bar("100")).unwrap() {
            assert!(v.abs() < dec!(0.000001), "expected ~0 ratio, got {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_er_rising_market_positive() {
        // Gradually rising prices → fast EMA > slow EMA → ratio > 0
        let mut er = EmaRatio::new("er", 3, 10).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0..20u32 {
            last = er.update_bar(&bar(&format!("{}", 100 + i))).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(0), "rising market should give positive ratio, got {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_er_ready_after_slow_period() {
        let mut er = EmaRatio::new("er", 3, 5).unwrap();
        assert!(!er.is_ready());
        for _ in 0..5 {
            er.update_bar(&bar("100")).unwrap();
        }
        assert!(er.is_ready());
    }

    #[test]
    fn test_er_reset() {
        let mut er = EmaRatio::new("er", 3, 5).unwrap();
        for _ in 0..6 {
            er.update_bar(&bar("100")).unwrap();
        }
        assert!(er.is_ready());
        er.reset();
        assert!(!er.is_ready());
    }

    #[test]
    fn test_er_period_and_name() {
        let er = EmaRatio::new("my_er", 5, 20).unwrap();
        assert_eq!(er.period(), 20);
        assert_eq!(er.fast_period(), 5);
        assert_eq!(er.slow_period(), 20);
        assert_eq!(er.name(), "my_er");
    }
}
