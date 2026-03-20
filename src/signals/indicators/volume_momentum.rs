//! Volume Momentum — volume-weighted signed price move, normalized by ATR.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Volume Momentum — `volume × (close - open) / ATR(period)`.
///
/// Combines price direction and volume into a single signed score:
/// - **Large positive**: strong bullish bar with high volume.
/// - **Large negative**: strong bearish bar with high volume.
/// - **Near zero**: indecisive or low-volume bar.
///
/// Normalizing by ATR makes the output scale-independent across price levels
/// and volatility regimes.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen,
/// or when ATR is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeMomentum;
/// use fin_primitives::signals::Signal;
/// let vm = VolumeMomentum::new("vm_14", 14).unwrap();
/// assert_eq!(vm.period(), 14);
/// ```
pub struct VolumeMomentum {
    name: String,
    period: usize,
    atr: Option<Decimal>,
    prev_close: Option<Decimal>,
    bars_seen: usize,
}

impl VolumeMomentum {
    /// Constructs a new `VolumeMomentum`.
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
        })
    }
}

impl Signal for VolumeMomentum {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.bars_seen >= self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = bar.true_range(self.prev_close);
        self.prev_close = Some(bar.close);
        self.bars_seen += 1;

        let period_d = Decimal::from(self.period as u32);
        self.atr = Some(match self.atr {
            None => tr,
            Some(prev) => (prev * (period_d - Decimal::ONE) + tr) / period_d,
        });

        if self.bars_seen < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let atr = self.atr.unwrap();
        if atr.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let body = bar.net_move();
        let vm = bar.volume
            .checked_mul(body)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_div(atr)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(vm))
    }

    fn reset(&mut self) {
        self.atr = None;
        self.prev_close = None;
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

    fn bar(o: &str, h: &str, l: &str, c: &str, vol: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hp, low: lp, close: cp,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vm_invalid_period() {
        assert!(VolumeMomentum::new("vm", 0).is_err());
    }

    #[test]
    fn test_vm_unavailable_before_period() {
        let mut vm = VolumeMomentum::new("vm", 3).unwrap();
        assert_eq!(vm.update_bar(&bar("100", "110", "90", "105", "1000")).unwrap(), SignalValue::Unavailable);
        assert_eq!(vm.update_bar(&bar("105", "115", "95", "110", "1000")).unwrap(), SignalValue::Unavailable);
        assert!(!vm.is_ready());
    }

    #[test]
    fn test_vm_bullish_bar_positive() {
        let mut vm = VolumeMomentum::new("vm", 2).unwrap();
        vm.update_bar(&bar("100", "110", "90", "100", "1000")).unwrap();
        // Bullish bar: close > open → positive VM
        let v = vm.update_bar(&bar("100", "115", "95", "110", "2000")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!(r > dec!(0), "bullish bar should give positive VM: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vm_bearish_bar_negative() {
        let mut vm = VolumeMomentum::new("vm", 2).unwrap();
        vm.update_bar(&bar("100", "110", "90", "100", "1000")).unwrap();
        // Bearish bar: close < open → negative VM
        let v = vm.update_bar(&bar("110", "115", "85", "90", "2000")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!(r < dec!(0), "bearish bar should give negative VM: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vm_doji_zero() {
        let mut vm = VolumeMomentum::new("vm", 2).unwrap();
        vm.update_bar(&bar("100", "110", "90", "100", "1000")).unwrap();
        // Doji: open=close → VM=0
        let v = vm.update_bar(&bar("100", "110", "90", "100", "5000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vm_reset() {
        let mut vm = VolumeMomentum::new("vm", 2).unwrap();
        for _ in 0..3 {
            vm.update_bar(&bar("100", "110", "90", "105", "1000")).unwrap();
        }
        assert!(vm.is_ready());
        vm.reset();
        assert!(!vm.is_ready());
    }

    #[test]
    fn test_vm_period_and_name() {
        let vm = VolumeMomentum::new("my_vm", 14).unwrap();
        assert_eq!(vm.period(), 14);
        assert_eq!(vm.name(), "my_vm");
    }
}
