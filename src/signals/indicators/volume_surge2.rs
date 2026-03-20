//! Volume Surge indicator -- flags bars where volume exceeds a threshold multiple of its SMA.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume Surge -- returns 1 when the current bar's volume exceeds `threshold` times
/// the rolling N-period SMA of volume, and 0 otherwise.
///
/// ```text
/// avg_vol[t] = SMA(volume, period)
/// surge[t]   = 1 if volume[t] > threshold * avg_vol[t], else 0
/// ```
///
/// A surge (value=1) signals abnormally high volume relative to recent norms,
/// which often accompanies significant price moves or institutional activity.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated
/// or if average volume is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeSurge2;
/// use fin_primitives::signals::Signal;
/// use rust_decimal_macros::dec;
/// let vs = VolumeSurge2::new("vs", 20, dec!(2.0)).unwrap();
/// assert_eq!(vs.period(), 20);
/// ```
pub struct VolumeSurge2 {
    name: String,
    period: usize,
    threshold: Decimal,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl VolumeSurge2 {
    /// Constructs a new `VolumeSurge2`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0` or threshold <= 0.
    pub fn new(name: impl Into<String>, period: usize, threshold: Decimal) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        if threshold <= Decimal::ZERO { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            threshold,
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }

    /// Returns the surge threshold multiplier.
    pub fn threshold(&self) -> Decimal { self.threshold }
}

impl Signal for VolumeSurge2 {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.volume);
        self.sum += bar.volume;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() { self.sum -= old; }
        }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        #[allow(clippy::cast_possible_truncation)]
        let avg = self.sum / Decimal::from(self.period as u32);
        if avg.is_zero() { return Ok(SignalValue::Unavailable); }
        let surge = if bar.volume > self.threshold * avg { Decimal::ONE } else { Decimal::ZERO };
        Ok(SignalValue::Scalar(surge))
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
        let p = Price::new(dec!(100)).unwrap();
        let v = Quantity::new(vol.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p, volume: v,
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vs_period_0_error() { assert!(VolumeSurge2::new("vs", 0, dec!(2)).is_err()); }
    #[test]
    fn test_vs_negative_threshold_error() { assert!(VolumeSurge2::new("vs", 5, dec!(-1)).is_err()); }

    #[test]
    fn test_vs_unavailable_before_period() {
        let mut vs = VolumeSurge2::new("vs", 3, dec!(2)).unwrap();
        assert_eq!(vs.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_vs_no_surge() {
        let mut vs = VolumeSurge2::new("vs", 3, dec!(2)).unwrap();
        for _ in 0..3 { vs.update_bar(&bar("100")).unwrap(); }
        // avg=100, threshold=2, volume=100 -> 100 > 200? No
        let v = vs.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vs_surge_detected() {
        let mut vs = VolumeSurge2::new("vs", 3, dec!(2)).unwrap();
        for _ in 0..3 { vs.update_bar(&bar("100")).unwrap(); }
        // After 3 bars of 100, push 600.
        // Window slides to [100, 100, 600], avg = 266.67
        // 600 > 2 * 266.67 = 533.33? Yes -> surge
        let v = vs.update_bar(&bar("600")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_vs_reset() {
        let mut vs = VolumeSurge2::new("vs", 2, dec!(2)).unwrap();
        vs.update_bar(&bar("100")).unwrap();
        vs.update_bar(&bar("100")).unwrap();
        assert!(vs.is_ready());
        vs.reset();
        assert!(!vs.is_ready());
    }
}
