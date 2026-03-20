//! EMA Band Width indicator -- fast/slow EMA spread as a percentage.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// EMA Band Width -- width between a fast EMA and a slow EMA expressed as a
/// percentage of the slow EMA.
///
/// ```text
/// band_width[t] = (fast_ema - slow_ema) / slow_ema x 100
/// ```
///
/// Positive values indicate the fast EMA is above the slow EMA (uptrend);
/// negative values indicate it is below (downtrend). The magnitude indicates
/// how wide the spread is relative to the slow baseline.
///
/// Returns [`SignalValue::Unavailable`] until the slow (larger) EMA has warmed up.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::EmaBandWidth;
/// use fin_primitives::signals::Signal;
/// let ebw = EmaBandWidth::new("ebw", 12, 26).unwrap();
/// assert_eq!(ebw.period(), 26);
/// ```
pub struct EmaBandWidth {
    name: String,
    fast_period: usize,
    slow_period: usize,
    fast_ema: Option<Decimal>,
    slow_ema: Option<Decimal>,
    fast_k: Decimal,
    slow_k: Decimal,
    bars: usize,
}

impl EmaBandWidth {
    /// Constructs a new `EmaBandWidth`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if either period is 0 or `fast >= slow`.
    pub fn new(name: impl Into<String>, fast: usize, slow: usize) -> Result<Self, FinError> {
        if fast == 0 { return Err(FinError::InvalidPeriod(fast)); }
        if slow == 0 || fast >= slow { return Err(FinError::InvalidPeriod(slow)); }
        #[allow(clippy::cast_possible_truncation)]
        let fast_k = Decimal::from(2u32) / (Decimal::from(fast as u32) + Decimal::ONE);
        #[allow(clippy::cast_possible_truncation)]
        let slow_k = Decimal::from(2u32) / (Decimal::from(slow as u32) + Decimal::ONE);
        Ok(Self {
            name: name.into(),
            fast_period: fast,
            slow_period: slow,
            fast_ema: None,
            slow_ema: None,
            fast_k,
            slow_k,
            bars: 0,
        })
    }

    /// Returns the fast EMA period.
    pub fn fast_period(&self) -> usize { self.fast_period }
    /// Returns the slow EMA period.
    pub fn slow_period(&self) -> usize { self.slow_period }
}

impl Signal for EmaBandWidth {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.slow_period }
    fn is_ready(&self) -> bool { self.bars > self.slow_period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.bars += 1;
        let close = bar.close;

        self.fast_ema = Some(match self.fast_ema {
            None => close,
            Some(prev) => prev + self.fast_k * (close - prev),
        });
        self.slow_ema = Some(match self.slow_ema {
            None => close,
            Some(prev) => prev + self.slow_k * (close - prev),
        });

        if self.bars <= self.slow_period {
            return Ok(SignalValue::Unavailable);
        }

        let fast = self.fast_ema.unwrap_or(Decimal::ZERO);
        let slow = self.slow_ema.unwrap_or(Decimal::ZERO);
        if slow.is_zero() { return Ok(SignalValue::Unavailable); }
        Ok(SignalValue::Scalar((fast - slow) / slow * Decimal::ONE_HUNDRED))
    }

    fn reset(&mut self) {
        self.fast_ema = None;
        self.slow_ema = None;
        self.bars = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

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
    fn test_ebw_invalid_periods() {
        assert!(EmaBandWidth::new("e", 0, 26).is_err());
        assert!(EmaBandWidth::new("e", 26, 12).is_err()); // fast >= slow
        assert!(EmaBandWidth::new("e", 26, 26).is_err()); // fast == slow
    }

    #[test]
    fn test_ebw_unavailable_before_slow_period() {
        let mut e = EmaBandWidth::new("e", 3, 5).unwrap();
        for _ in 0..5 {
            assert_eq!(e.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_ebw_constant_price_zero_width() {
        let mut e = EmaBandWidth::new("e", 3, 5).unwrap();
        // With constant price, both EMAs converge to same value -> width = 0
        for _ in 0..20 {
            e.update_bar(&bar("100")).unwrap();
        }
        if let SignalValue::Scalar(v) = e.update_bar(&bar("100")).unwrap() {
            assert!(v.abs() < dec!(0.0001), "expected ~0, got {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ebw_uptrend_positive() {
        let mut e = EmaBandWidth::new("e", 3, 5).unwrap();
        // Rising prices: fast EMA responds faster, so fast > slow
        for i in 0u32..20 {
            e.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        if let SignalValue::Scalar(v) = e.update_bar(&bar("120")).unwrap() {
            assert!(v > dec!(0), "expected positive band width in uptrend, got {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ebw_reset() {
        let mut e = EmaBandWidth::new("e", 3, 5).unwrap();
        for _ in 0..20 { e.update_bar(&bar("100")).unwrap(); }
        assert!(e.is_ready());
        e.reset();
        assert!(!e.is_ready());
    }
}
