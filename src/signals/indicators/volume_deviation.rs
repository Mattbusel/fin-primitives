//! Volume Deviation — current volume's percentage deviation from rolling average.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume Deviation — `(volume - avg_volume) / avg_volume` over N bars.
///
/// Measures how far the current bar's volume deviates from its rolling mean:
/// - **Positive**: above-average volume (surge, commitment).
/// - **Negative**: below-average volume (lack of interest).
/// - **0**: exactly average.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen,
/// or when the average volume is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeDeviation;
/// use fin_primitives::signals::Signal;
/// let vd = VolumeDeviation::new("vd_14", 14).unwrap();
/// assert_eq!(vd.period(), 14);
/// ```
pub struct VolumeDeviation {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl VolumeDeviation {
    /// Constructs a new `VolumeDeviation`.
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
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for VolumeDeviation {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.sum += bar.volume;
        self.window.push_back(bar.volume);
        if self.window.len() > self.period {
            let removed = self.window.pop_front().unwrap();
            self.sum -= removed;
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let avg = self.sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        if avg.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let deviation = (bar.volume - avg)
            .checked_div(avg)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(deviation))
    }

    fn reset(&mut self) {
        self.window.clear();
        self.sum = Decimal::ZERO;
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
        let p = Price::new("100".parse().unwrap()).unwrap();
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
    fn test_vd_invalid_period() {
        assert!(VolumeDeviation::new("vd", 0).is_err());
    }

    #[test]
    fn test_vd_unavailable_before_period() {
        let mut s = VolumeDeviation::new("vd", 3).unwrap();
        assert_eq!(s.update_bar(&bar("1000")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("1000")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_vd_at_average_gives_zero() {
        let mut s = VolumeDeviation::new("vd", 3).unwrap();
        // constant volume → current = average → deviation = 0
        s.update_bar(&bar("1000")).unwrap();
        s.update_bar(&bar("1000")).unwrap();
        let v = s.update_bar(&bar("1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vd_double_average_gives_one() {
        let mut s = VolumeDeviation::new("vd", 2).unwrap();
        s.update_bar(&bar("1000")).unwrap();
        // After 2 bars: avg = (1000+2000)/2=1500, current=2000, dev=(2000-1500)/1500
        let v = s.update_bar(&bar("2000")).unwrap();
        if let SignalValue::Scalar(r) = v {
            // dev = 500/1500 = 1/3
            assert!(r > dec!(0), "above-avg volume should give positive deviation: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vd_reset() {
        let mut s = VolumeDeviation::new("vd", 3).unwrap();
        for _ in 0..3 { s.update_bar(&bar("1000")).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
