//! Median Volume indicator -- rolling median of bar volume.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Median Volume -- rolling median of bar volume over the last `period` bars.
///
/// Unlike the simple average, the median is robust to volume spikes. Useful for
/// detecting anomalous bars whose volume deviates significantly from the typical level.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::MedianVolume;
/// use fin_primitives::signals::Signal;
/// let mv = MedianVolume::new("mv", 20).unwrap();
/// assert_eq!(mv.period(), 20);
/// ```
pub struct MedianVolume {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
}

impl MedianVolume {
    /// Constructs a new `MedianVolume`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            window: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for MedianVolume {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.volume);
        if self.window.len() > self.period {
            self.window.pop_front();
        }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        let mut sorted: Vec<Decimal> = self.window.iter().copied().collect();
        sorted.sort();
        let mid = sorted.len() / 2;
        let median = if sorted.len() % 2 == 1 {
            sorted[mid]
        } else {
            (sorted[mid - 1] + sorted[mid]) / Decimal::TWO
        };
        Ok(SignalValue::Scalar(median))
    }

    fn reset(&mut self) {
        self.window.clear();
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
    fn test_mv_period_0_error() { assert!(MedianVolume::new("mv", 0).is_err()); }

    #[test]
    fn test_mv_unavailable_before_period() {
        let mut mv = MedianVolume::new("mv", 3).unwrap();
        assert_eq!(mv.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_mv_odd_period_median() {
        let mut mv = MedianVolume::new("mv", 5).unwrap();
        for v in ["100", "200", "300", "400", "500"] { mv.update_bar(&bar(v)).unwrap(); }
        // window now full [100,200,300,400,500]; push 10000 -> slides to [200,300,400,500,10000]
        let r = mv.update_bar(&bar("10000")).unwrap();
        // sorted: [200,300,400,500,10000]; median = 400
        assert_eq!(r, SignalValue::Scalar(dec!(400)));
    }

    #[test]
    fn test_mv_even_period_median() {
        let mut mv = MedianVolume::new("mv", 4).unwrap();
        mv.update_bar(&bar("100")).unwrap();
        mv.update_bar(&bar("200")).unwrap();
        mv.update_bar(&bar("300")).unwrap();
        let r = mv.update_bar(&bar("400")).unwrap();
        // sorted: [100,200,300,400]; median = (200+300)/2 = 250
        assert_eq!(r, SignalValue::Scalar(dec!(250)));
    }

    #[test]
    fn test_mv_spike_resistant() {
        // Median should be near the typical value, not pulled toward spike
        let mut mv = MedianVolume::new("mv", 5).unwrap();
        mv.update_bar(&bar("100")).unwrap();
        mv.update_bar(&bar("100")).unwrap();
        mv.update_bar(&bar("100")).unwrap();
        mv.update_bar(&bar("100")).unwrap();
        let r = mv.update_bar(&bar("100000")).unwrap();
        // sorted: [100,100,100,100,100000]; median = 100
        assert_eq!(r, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_mv_reset() {
        let mut mv = MedianVolume::new("mv", 3).unwrap();
        for v in ["100", "200", "300"] { mv.update_bar(&bar(v)).unwrap(); }
        assert!(mv.is_ready());
        mv.reset();
        assert!(!mv.is_ready());
    }
}
