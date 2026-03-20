//! Bull/Bear Power indicator (Elder's).

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Bull/Bear Power — Alexander Elder's measure of buying and selling pressure.
///
/// ```text
/// EMA_t        = EMA(close, period)
/// bull_power   = high - EMA_t   (bulls' ability to push price above EMA)
/// bear_power   = low  - EMA_t   (bears' drag below EMA; typically negative)
/// bbp          = bull_power + bear_power
///              = (high + low) - 2 × EMA_t
/// ```
///
/// * Positive `bbp` → bulls are in control
/// * Negative `bbp` → bears are in control
///
/// Returns [`SignalValue::Unavailable`] until the EMA has seeded (`period` bars).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BullBearPower;
/// use fin_primitives::signals::Signal;
///
/// let bbp = BullBearPower::new("bbp", 13).unwrap();
/// assert_eq!(bbp.period(), 13);
/// ```
pub struct BullBearPower {
    name: String,
    period: usize,
    k: Decimal,
    seed: VecDeque<Decimal>,
    ema: Option<Decimal>,
}

impl BullBearPower {
    /// Creates a new `BullBearPower`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        #[allow(clippy::cast_possible_truncation)]
        let k = Decimal::TWO / Decimal::from((period + 1) as u32);
        Ok(Self {
            name: name.into(),
            period,
            k,
            seed: VecDeque::with_capacity(period),
            ema: None,
        })
    }

    /// Returns the current EMA value (the midline).
    pub fn ema(&self) -> Option<Decimal> { self.ema }
}

impl Signal for BullBearPower {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let ema = match self.ema {
            None => {
                self.seed.push_back(bar.close);
                if self.seed.len() < self.period {
                    return Ok(SignalValue::Unavailable);
                }
                let sma = self.seed.iter().sum::<Decimal>()
                    / Decimal::from(self.period as u32);
                self.ema = Some(sma);
                sma
            }
            Some(prev) => {
                let v = bar.close * self.k + prev * (Decimal::ONE - self.k);
                self.ema = Some(v);
                v
            }
        };

        let bbp = (bar.high + bar.low) - (Decimal::TWO * ema);
        Ok(SignalValue::Scalar(bbp))
    }

    fn is_ready(&self) -> bool {
        self.ema.is_some()
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.seed.clear();
        self.ema = None;
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
    fn test_bbp_invalid() {
        assert!(BullBearPower::new("b", 0).is_err());
    }

    #[test]
    fn test_bbp_unavailable_before_period() {
        let mut b = BullBearPower::new("b", 5).unwrap();
        for _ in 0..4 {
            assert_eq!(b.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_bbp_flat_is_zero() {
        // Flat price: high=low=close=EMA → BBP = (c+c) - 2c = 0
        let mut b = BullBearPower::new("b", 5).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..20 {
            last = b.update_bar(&bar("100")).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v.abs() < dec!(0.001), "flat BBP should be ~0: {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_bbp_reset() {
        let mut b = BullBearPower::new("b", 5).unwrap();
        for _ in 0..10 { b.update_bar(&bar("100")).unwrap(); }
        assert!(b.is_ready());
        b.reset();
        assert!(!b.is_ready());
    }
}
