//! Volume Consistency indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume Consistency — the fraction of the last `period` bars where volume
/// increased compared to the immediately preceding bar.
///
/// ```text
/// up_count  = count(volume[i] > volume[i-1], i in [t-period+1, t])
/// output    = up_count / period × 100
/// ```
///
/// - **100**: volume has risen every single bar in the window.
/// - **0**: volume has fallen every single bar in the window.
/// - **50**: volume is alternating — no consistent trend in participation.
///
/// Useful for detecting whether volume is in an accumulation phase (consistently
/// growing) or distribution phase (consistently declining).
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeConsistency;
/// use fin_primitives::signals::Signal;
/// let vc = VolumeConsistency::new("vc_10", 10).unwrap();
/// assert_eq!(vc.period(), 10);
/// ```
pub struct VolumeConsistency {
    name: String,
    period: usize,
    // Store whether volume[i] > volume[i-1] for the last `period` transitions
    ups: VecDeque<bool>,
    prev_volume: Option<Decimal>,
}

impl VolumeConsistency {
    /// Constructs a new `VolumeConsistency`.
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
            ups: VecDeque::with_capacity(period),
            prev_volume: None,
        })
    }
}

impl Signal for VolumeConsistency {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.ups.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pv) = self.prev_volume {
            let up = bar.volume > pv;
            self.ups.push_back(up);
            if self.ups.len() > self.period {
                self.ups.pop_front();
            }
        }
        self.prev_volume = Some(bar.volume);

        if self.ups.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let count = self.ups.iter().filter(|&&u| u).count();
        let pct = Decimal::from(count as u32)
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?
            * Decimal::ONE_HUNDRED;

        Ok(SignalValue::Scalar(pct))
    }

    fn reset(&mut self) {
        self.ups.clear();
        self.prev_volume = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(vol: &str) -> OhlcvBar {
        let p = Price::new(dec!(100)).unwrap();
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
    fn test_vc_invalid_period() {
        assert!(VolumeConsistency::new("vc", 0).is_err());
    }

    #[test]
    fn test_vc_unavailable_during_warmup() {
        let mut vc = VolumeConsistency::new("vc", 3).unwrap();
        for v in &["100", "200", "300"] {
            assert_eq!(vc.update_bar(&bar(v)).unwrap(), SignalValue::Unavailable);
        }
        assert!(!vc.is_ready());
    }

    #[test]
    fn test_vc_always_increasing_is_100() {
        let mut vc = VolumeConsistency::new("vc", 3).unwrap();
        vc.update_bar(&bar("100")).unwrap();
        vc.update_bar(&bar("200")).unwrap();
        vc.update_bar(&bar("300")).unwrap();
        if let SignalValue::Scalar(v) = vc.update_bar(&bar("400")).unwrap() {
            assert_eq!(v, dec!(100), "always up volume → 100%");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vc_always_decreasing_is_zero() {
        let mut vc = VolumeConsistency::new("vc", 3).unwrap();
        vc.update_bar(&bar("400")).unwrap();
        vc.update_bar(&bar("300")).unwrap();
        vc.update_bar(&bar("200")).unwrap();
        if let SignalValue::Scalar(v) = vc.update_bar(&bar("100")).unwrap() {
            assert_eq!(v, dec!(0), "always down volume → 0%");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vc_alternating_fifty() {
        // up/down/up → 2/3 ≈ 66.7%; down/up/down → 1/3 ≈ 33.3%; mixed
        let mut vc = VolumeConsistency::new("vc", 4).unwrap();
        // up, down, up, down → 2 ups / 4 = 50%
        vc.update_bar(&bar("100")).unwrap();
        vc.update_bar(&bar("200")).unwrap(); // up
        vc.update_bar(&bar("100")).unwrap(); // down
        vc.update_bar(&bar("200")).unwrap(); // up
        if let SignalValue::Scalar(v) = vc.update_bar(&bar("100")).unwrap() { // down
            assert_eq!(v, dec!(50), "alternating → 50%: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vc_reset() {
        let mut vc = VolumeConsistency::new("vc", 3).unwrap();
        for v in &["100","200","300","400"] { vc.update_bar(&bar(v)).unwrap(); }
        assert!(vc.is_ready());
        vc.reset();
        assert!(!vc.is_ready());
    }
}
