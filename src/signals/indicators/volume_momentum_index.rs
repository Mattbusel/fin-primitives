//! Volume Momentum Index indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume Momentum Index (VMI).
///
/// Combines price momentum (close-to-close return) with volume momentum
/// (volume relative to average) into a single composite indicator.
///
/// Per-bar formula:
/// - `price_mom = (close_t - close_{t-1}) / close_{t-1}` (one-bar return)
/// - `vol_ratio = volume_t / mean_volume` (volume relative to rolling mean)
/// - `vmi = price_mom * vol_ratio`
///
/// Rolling: `sum(vmi, period)`
///
/// Positive accumulation: price gains on above-average volume (healthy uptrend).
/// Negative accumulation: price losses on above-average volume (healthy downtrend).
/// Near zero: choppy or volume-lite moves.
///
/// Returns `SignalValue::Unavailable` until `period + 1` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeMomentumIndex;
/// use fin_primitives::signals::Signal;
/// let vmi = VolumeMomentumIndex::new("vmi_14", 14).unwrap();
/// assert_eq!(vmi.period(), 14);
/// ```
pub struct VolumeMomentumIndex {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
    volumes: VecDeque<Decimal>,
    vmis: VecDeque<Decimal>,
}

impl VolumeMomentumIndex {
    /// Constructs a new `VolumeMomentumIndex`.
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
            closes: VecDeque::with_capacity(2),
            volumes: VecDeque::with_capacity(period),
            vmis: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for VolumeMomentumIndex {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        self.volumes.push_back(bar.volume);

        if self.closes.len() > 2 {
            self.closes.pop_front();
        }
        if self.volumes.len() > self.period {
            self.volumes.pop_front();
        }

        if self.closes.len() < 2 {
            return Ok(SignalValue::Unavailable);
        }

        let prev_close = self.closes[0];
        let curr_close = self.closes[1];

        if prev_close.is_zero() {
            self.vmis.push_back(Decimal::ZERO);
        } else {
            let price_mom = (curr_close - prev_close)
                .checked_div(prev_close)
                .ok_or(FinError::ArithmeticOverflow)?;

            // Compute mean volume for the current window
            let vol_sum: Decimal = self.volumes.iter().copied().sum();
            #[allow(clippy::cast_possible_truncation)]
            let mean_vol = vol_sum
                .checked_div(Decimal::from(self.volumes.len() as u32))
                .ok_or(FinError::ArithmeticOverflow)?;

            let vol_ratio = if mean_vol.is_zero() {
                Decimal::ZERO
            } else {
                bar.volume.checked_div(mean_vol).ok_or(FinError::ArithmeticOverflow)?
            };

            let vmi = price_mom.checked_mul(vol_ratio).ok_or(FinError::ArithmeticOverflow)?;
            self.vmis.push_back(vmi);
        }

        if self.vmis.len() > self.period {
            self.vmis.pop_front();
        }
        if self.vmis.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sum: Decimal = self.vmis.iter().copied().sum();
        Ok(SignalValue::Scalar(sum))
    }

    fn is_ready(&self) -> bool {
        self.vmis.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.closes.clear();
        self.volumes.clear();
        self.vmis.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(close: &str, vol: &str) -> OhlcvBar {
        let p = Price::new(close.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_period_zero_fails() {
        assert!(matches!(VolumeMomentumIndex::new("vmi", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut vmi = VolumeMomentumIndex::new("vmi", 3).unwrap();
        assert_eq!(vmi.update_bar(&bar("100", "1000")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_flat_price_zero_vmi() {
        let mut vmi = VolumeMomentumIndex::new("vmi", 3).unwrap();
        for _ in 0..4 {
            vmi.update_bar(&bar("100", "1000")).unwrap();
        }
        let v = vmi.update_bar(&bar("100", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_reset() {
        let mut vmi = VolumeMomentumIndex::new("vmi", 2).unwrap();
        for _ in 0..3 {
            vmi.update_bar(&bar("100", "1000")).unwrap();
        }
        assert!(vmi.is_ready());
        vmi.reset();
        assert!(!vmi.is_ready());
    }
}
