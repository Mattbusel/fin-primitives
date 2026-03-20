//! Volatility Momentum indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volatility Momentum — standard deviation of bar-to-bar close changes over
/// `period` bars.
///
/// A rising value indicates increasing price velocity variance (explosive
/// momentum); a falling value indicates returns are becoming more uniform.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolatilityMomentum;
/// use fin_primitives::signals::Signal;
///
/// let vm = VolatilityMomentum::new("vm", 10).unwrap();
/// assert_eq!(vm.period(), 10);
/// ```
pub struct VolatilityMomentum {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    changes: VecDeque<Decimal>,
}

impl VolatilityMomentum {
    /// Constructs a new `VolatilityMomentum`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            changes: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for VolatilityMomentum {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.changes.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = match self.prev_close {
            None => SignalValue::Unavailable,
            Some(pc) => {
                let change = bar.close - pc;
                self.changes.push_back(change);
                if self.changes.len() > self.period {
                    self.changes.pop_front();
                }
                if self.changes.len() < self.period {
                    SignalValue::Unavailable
                } else {
                    let n = self.changes.len();
                    let nf = n as f64;
                    let vals: Vec<f64> = self.changes.iter()
                        .filter_map(|c| c.to_f64())
                        .collect();
                    if vals.len() != n {
                        SignalValue::Unavailable
                    } else {
                        let mean = vals.iter().sum::<f64>() / nf;
                        let var = vals.iter().map(|v| { let d = v - mean; d * d }).sum::<f64>() / nf;
                        match Decimal::from_f64(var.sqrt()) {
                            Some(v) => SignalValue::Scalar(v),
                            None => SignalValue::Unavailable,
                        }
                    }
                }
            }
        };
        self.prev_close = Some(bar.close);
        Ok(result)
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.changes.clear();
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
    fn test_vm_invalid_period() {
        assert!(VolatilityMomentum::new("vm", 0).is_err());
        assert!(VolatilityMomentum::new("vm", 1).is_err());
    }

    #[test]
    fn test_vm_unavailable_before_warm_up() {
        let mut vm = VolatilityMomentum::new("vm", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(vm.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_vm_constant_changes_give_zero_stddev() {
        // Constant +1 changes → mean=1, all deviations=0 → std=0
        let mut vm = VolatilityMomentum::new("vm", 3).unwrap();
        let prices = ["100", "101", "102", "103", "104"];
        let mut last = SignalValue::Unavailable;
        for p in &prices {
            last = vm.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v < dec!(0.001), "constant changes should give near-zero std dev: {}", v);
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vm_varying_changes_positive() {
        let mut vm = VolatilityMomentum::new("vm", 4).unwrap();
        let prices = ["100", "105", "103", "110", "104", "115"];
        let mut last = SignalValue::Unavailable;
        for p in &prices {
            last = vm.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(0), "varying changes should give positive std dev: {}", v);
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vm_reset() {
        let mut vm = VolatilityMomentum::new("vm", 3).unwrap();
        for p in ["100", "101", "102", "103"] { vm.update_bar(&bar(p)).unwrap(); }
        assert!(vm.is_ready());
        vm.reset();
        assert!(!vm.is_ready());
    }
}
