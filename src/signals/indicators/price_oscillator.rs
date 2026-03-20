//! Price Oscillator indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Oscillator — difference between fast and slow SMA as a percentage of slow SMA.
///
/// ```text
/// fast_sma = mean(close, fast_period)
/// slow_sma = mean(close, slow_period)
/// output   = (fast_sma − slow_sma) / slow_sma × 100
/// ```
///
/// Similar to PPO but uses SMA instead of EMA. Positive output = bullish;
/// negative = bearish. Returns 0 when slow_sma is zero.
///
/// Returns [`SignalValue::Unavailable`] until `slow_period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceOscillator;
/// use fin_primitives::signals::Signal;
///
/// let po = PriceOscillator::new("po", 5, 20).unwrap();
/// assert_eq!(po.period(), 20);
/// ```
pub struct PriceOscillator {
    name: String,
    fast: usize,
    slow: usize,
    closes: VecDeque<Decimal>,
}

impl PriceOscillator {
    /// Creates a new `PriceOscillator`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `fast == 0`.
    /// Returns [`FinError::InvalidInput`] if `fast >= slow`.
    pub fn new(name: impl Into<String>, fast: usize, slow: usize) -> Result<Self, FinError> {
        if fast == 0 { return Err(FinError::InvalidPeriod(fast)); }
        if fast >= slow {
            return Err(FinError::InvalidInput("fast must be less than slow".into()));
        }
        Ok(Self {
            name: name.into(),
            fast,
            slow,
            closes: VecDeque::with_capacity(slow),
        })
    }
}

impl Signal for PriceOscillator {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.slow { self.closes.pop_front(); }
        if self.closes.len() < self.slow { return Ok(SignalValue::Unavailable); }

        let slow_sma = self.closes.iter().sum::<Decimal>() / Decimal::from(self.slow as u32);
        let fast_sma = self.closes.iter().rev().take(self.fast).sum::<Decimal>()
            / Decimal::from(self.fast as u32);

        if slow_sma.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        Ok(SignalValue::Scalar((fast_sma - slow_sma) / slow_sma * Decimal::from(100u32)))
    }

    fn is_ready(&self) -> bool { self.closes.len() >= self.slow }
    fn period(&self) -> usize { self.slow }

    fn reset(&mut self) {
        self.closes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
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
    fn test_po_invalid() {
        assert!(PriceOscillator::new("p", 0, 20).is_err());
        assert!(PriceOscillator::new("p", 20, 10).is_err());
        assert!(PriceOscillator::new("p", 10, 10).is_err());
    }

    #[test]
    fn test_po_unavailable_before_warmup() {
        let mut p = PriceOscillator::new("p", 3, 5).unwrap();
        for _ in 0..4 {
            assert_eq!(p.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_po_flat_is_zero() {
        let mut p = PriceOscillator::new("p", 3, 5).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..8 { last = p.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_po_uptrend_positive() {
        // Rising: recent bars (fast) higher than older (slow) → positive
        let mut p = PriceOscillator::new("p", 3, 5).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0u32..10 {
            let price = format!("{}", 100 + i);
            last = p.update_bar(&bar(&price)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(0), "expected positive, got {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_po_downtrend_negative() {
        let mut p = PriceOscillator::new("p", 3, 5).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0u32..10 {
            let price = format!("{}", 200 - i);
            last = p.update_bar(&bar(&price)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v < dec!(0), "expected negative, got {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_po_reset() {
        let mut p = PriceOscillator::new("p", 3, 5).unwrap();
        for _ in 0..8 { p.update_bar(&bar("100")).unwrap(); }
        assert!(p.is_ready());
        p.reset();
        assert!(!p.is_ready());
    }
}
