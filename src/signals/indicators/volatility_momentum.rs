//! Volatility Momentum — rate of change of ATR over N bars.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volatility Momentum — `ATR(period)[now] / ATR(period)[N bars ago] - 1`.
///
/// Measures whether volatility (ATR) is accelerating or decelerating:
/// - **Positive**: volatility is expanding (current ATR > N-bar-ago ATR).
/// - **Negative**: volatility is contracting (current ATR < N-bar-ago ATR).
/// - **Zero**: volatility is unchanged.
///
/// Useful for anticipating volatility regime changes — a rising `VolatilityMomentum`
/// can precede breakouts; a falling one may indicate consolidation.
///
/// Returns [`SignalValue::Unavailable`] until `period * 2` bars have been seen
/// (one period to seed the ATR, one period to have a reference ATR), or when
/// the lookback ATR is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolatilityMomentum;
/// use fin_primitives::signals::Signal;
/// let vm = VolatilityMomentum::new("vm_14", 14).unwrap();
/// assert_eq!(vm.period(), 14);
/// ```
pub struct VolatilityMomentum {
    name: String,
    period: usize,
    atr: Option<Decimal>,
    prev_close: Option<Decimal>,
    bars_seen: usize,
    atr_history: VecDeque<Decimal>,
}

impl VolatilityMomentum {
    /// Constructs a new `VolatilityMomentum`.
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
            atr: None,
            prev_close: None,
            bars_seen: 0,
            atr_history: VecDeque::with_capacity(period + 1),
        })
    }
}

impl Signal for VolatilityMomentum {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.bars_seen >= self.period * 2
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = bar.true_range(self.prev_close);
        self.prev_close = Some(bar.close);
        self.bars_seen += 1;

        let period_d = Decimal::from(self.period as u32);
        let current_atr = match self.atr {
            None => tr,
            Some(prev) => (prev * (period_d - Decimal::ONE) + tr) / period_d,
        };
        self.atr = Some(current_atr);

        self.atr_history.push_back(current_atr);
        if self.atr_history.len() > self.period + 1 {
            self.atr_history.pop_front();
        }

        if self.bars_seen < self.period * 2 {
            return Ok(SignalValue::Unavailable);
        }

        // Lookback ATR is the oldest in the history (period bars ago)
        let lookback_atr = *self.atr_history.front().unwrap();
        if lookback_atr.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let roc = current_atr
            .checked_div(lookback_atr)
            .ok_or(FinError::ArithmeticOverflow)?
            - Decimal::ONE;

        Ok(SignalValue::Scalar(roc))
    }

    fn reset(&mut self) {
        self.atr = None;
        self.prev_close = None;
        self.bars_seen = 0;
        self.atr_history.clear();
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
        let mid_val: rust_decimal::Decimal = (hp.value() + lp.value()) / Decimal::from(2u32);
        let cp = Price::new(mid_val).unwrap();
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
    fn test_vm_invalid_period() {
        assert!(VolatilityMomentum::new("vm", 0).is_err());
    }

    #[test]
    fn test_vm_unavailable_before_2x_period() {
        let mut vm = VolatilityMomentum::new("vm", 3).unwrap();
        for _ in 0..5 {
            assert_eq!(vm.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!vm.is_ready());
    }

    #[test]
    fn test_vm_constant_range_zero_momentum() {
        // Constant TR → ATR doesn't change → momentum = 0
        let mut vm = VolatilityMomentum::new("vm", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..10 {
            last = vm.update_bar(&bar("110", "90")).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v.abs() < dec!(0.001), "constant volatility should give ~0 momentum: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vm_expanding_volatility_positive() {
        let mut vm = VolatilityMomentum::new("vm", 3).unwrap();
        // Start with narrow bars then switch to wide bars
        for _ in 0..4 {
            vm.update_bar(&bar("101", "99")).unwrap();
        }
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 {
            last = vm.update_bar(&bar("120", "80")).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(0), "expanding volatility should give positive momentum: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vm_reset() {
        let mut vm = VolatilityMomentum::new("vm", 3).unwrap();
        for _ in 0..8 {
            vm.update_bar(&bar("110", "90")).unwrap();
        }
        assert!(vm.is_ready());
        vm.reset();
        assert!(!vm.is_ready());
    }

    #[test]
    fn test_vm_period_and_name() {
        let vm = VolatilityMomentum::new("my_vm", 14).unwrap();
        assert_eq!(vm.period(), 14);
        assert_eq!(vm.name(), "my_vm");
    }
}
