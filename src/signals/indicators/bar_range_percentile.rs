//! Bar Range Percentile indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Bar Range Percentile — the exact percentile of the current bar's range within
/// a rolling window, computed by linear interpolation between sorted values.
///
/// ```text
/// range[i]   = high[i] - low[i]
/// sorted     = sort(range[t-period+1 .. t])
/// percentile = interpolated rank of range[t] in sorted (0–100)
/// ```
///
/// Unlike `PercentRankRange` (which uses a strict count-below rank), this indicator
/// uses fractional interpolation for smoother output on small windows.
///
/// - **100**: current range is at or above the maximum in the window.
/// - **0**: current range is at or below the minimum.
/// - **50**: current range is at the median of the window.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period < 2`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BarRangePercentile;
/// use fin_primitives::signals::Signal;
/// let brp = BarRangePercentile::new("brp_20", 20).unwrap();
/// assert_eq!(brp.period(), 20);
/// ```
pub struct BarRangePercentile {
    name: String,
    period: usize,
    ranges: VecDeque<Decimal>,
}

impl BarRangePercentile {
    /// Constructs a new `BarRangePercentile`.
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
            ranges: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for BarRangePercentile {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.ranges.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        use rust_decimal::prelude::ToPrimitive;

        let r = bar.range();
        self.ranges.push_back(r);
        if self.ranges.len() > self.period {
            self.ranges.pop_front();
        }
        if self.ranges.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let mut sorted: Vec<f64> = self.ranges
            .iter()
            .filter_map(|v| v.to_f64())
            .collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let current = r.to_f64().unwrap_or(0.0);
        let n = sorted.len() as f64;

        // Count how many values are strictly below current
        let below = sorted.iter().filter(|&&v| v < current).count() as f64;
        // Interpolate: if current equals sorted[i], it's at percentile i/(n-1)*100
        let pct = below / (n - 1.0) * 100.0;
        let pct = pct.min(100.0).max(0.0);

        Decimal::try_from(pct)
            .map(SignalValue::Scalar)
            .map_err(|_| FinError::ArithmeticOverflow)
    }

    fn reset(&mut self) {
        self.ranges.clear();
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
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: hp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_brp_invalid_period() {
        assert!(BarRangePercentile::new("brp", 0).is_err());
        assert!(BarRangePercentile::new("brp", 1).is_err());
    }

    #[test]
    fn test_brp_unavailable_during_warmup() {
        let mut brp = BarRangePercentile::new("brp", 3).unwrap();
        assert_eq!(brp.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert_eq!(brp.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert!(!brp.is_ready());
    }

    #[test]
    fn test_brp_uniform_ranges_fifty() {
        // All equal ranges → percentile = 0 (below / (n-1) * 100 = 0/(2)*100 = 0 for n=3)
        // Actually: for 3 identical values sorted=[r,r,r], current=r, below=0, pct=0/(3-1)*100=0
        let mut brp = BarRangePercentile::new("brp", 3).unwrap();
        for _ in 0..4 {
            brp.update_bar(&bar("110", "90")).unwrap(); // range=20
        }
        if let SignalValue::Scalar(v) = brp.update_bar(&bar("110", "90")).unwrap() {
            assert_eq!(v, dec!(0)); // all same → below=0
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_brp_max_range_is_100() {
        // Increasing ranges: 10, 20, 30 → current (30) is the max → percentile=100
        let mut brp = BarRangePercentile::new("brp", 3).unwrap();
        brp.update_bar(&bar("110", "100")).unwrap(); // range=10
        brp.update_bar(&bar("120", "100")).unwrap(); // range=20
        if let SignalValue::Scalar(v) = brp.update_bar(&bar("130", "100")).unwrap() {
            assert!(v > dec!(90), "largest range in window → near 100%: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_brp_min_range_is_0() {
        // Decreasing ranges: 30, 20, 10 → current (10) is the min → percentile=0
        let mut brp = BarRangePercentile::new("brp", 3).unwrap();
        brp.update_bar(&bar("130", "100")).unwrap(); // range=30
        brp.update_bar(&bar("120", "100")).unwrap(); // range=20
        if let SignalValue::Scalar(v) = brp.update_bar(&bar("110", "100")).unwrap() {
            assert_eq!(v, dec!(0), "smallest range in window → 0%");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_brp_reset() {
        let mut brp = BarRangePercentile::new("brp", 3).unwrap();
        for _ in 0..3 { brp.update_bar(&bar("110", "90")).unwrap(); }
        assert!(brp.is_ready());
        brp.reset();
        assert!(!brp.is_ready());
    }
}
