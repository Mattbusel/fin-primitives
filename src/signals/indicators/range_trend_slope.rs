//! Range Trend Slope — OLS linear regression slope of bar ranges over N bars.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Range Trend Slope — linear regression slope of `(high - low)` over `period` bars.
///
/// Fits a least-squares line to the sequence of recent bar ranges and returns its slope:
/// - **Positive**: ranges are trending upward — volatility is expanding.
/// - **Negative**: ranges are trending downward — volatility is contracting.
/// - **Near zero**: ranges are moving sideways with no clear trend.
///
/// Computed using f64 arithmetic for efficiency.
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period < 2`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RangeTrendSlope;
/// use fin_primitives::signals::Signal;
/// let rts = RangeTrendSlope::new("rts_10", 10).unwrap();
/// assert_eq!(rts.period(), 10);
/// ```
pub struct RangeTrendSlope {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
}

impl RangeTrendSlope {
    /// Constructs a new `RangeTrendSlope`.
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
            window: VecDeque::with_capacity(period),
        })
    }
}

fn ols_slope(data: &VecDeque<Decimal>) -> Option<f64> {
    let n = data.len() as f64;
    if n < 2.0 { return None; }

    let mean_t = (n - 1.0) / 2.0;
    let mean_y: f64 = data.iter().filter_map(|d| d.to_f64()).sum::<f64>() / n;

    let mut num = 0.0_f64;
    let mut den = 0.0_f64;

    for (i, d) in data.iter().enumerate() {
        let t = i as f64 - mean_t;
        let y = d.to_f64().unwrap_or(mean_y);
        num += t * (y - mean_y);
        den += t * t;
    }

    if den == 0.0 { None } else { Some(num / den) }
}

impl Signal for RangeTrendSlope {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.range());
        if self.window.len() > self.period {
            self.window.pop_front();
        }

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        match ols_slope(&self.window) {
            Some(slope) => Ok(SignalValue::Scalar(
                Decimal::try_from(slope).unwrap_or(Decimal::ZERO),
            )),
            None => Ok(SignalValue::Unavailable),
        }
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
    fn test_rts_invalid_period() {
        assert!(RangeTrendSlope::new("rts", 0).is_err());
        assert!(RangeTrendSlope::new("rts", 1).is_err());
    }

    #[test]
    fn test_rts_unavailable_during_warmup() {
        let mut s = RangeTrendSlope::new("rts", 3).unwrap();
        assert_eq!(s.update_bar(&bar("110","90")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("112","88")).unwrap(), SignalValue::Unavailable);
        assert!(!s.is_ready());
    }

    #[test]
    fn test_rts_expanding_ranges_positive_slope() {
        // Ranges: 10, 20, 30 — steadily expanding → positive slope
        let mut s = RangeTrendSlope::new("rts", 3).unwrap();
        s.update_bar(&bar("105","95")).unwrap();   // range=10
        s.update_bar(&bar("110","90")).unwrap();   // range=20
        if let SignalValue::Scalar(v) = s.update_bar(&bar("115","85")).unwrap() {
            // range=30 → slope > 0
            assert!(v > dec!(0), "expanding ranges → positive slope: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rts_contracting_ranges_negative_slope() {
        // Ranges: 30, 20, 10 — contracting → negative slope
        let mut s = RangeTrendSlope::new("rts", 3).unwrap();
        s.update_bar(&bar("115","85")).unwrap();   // range=30
        s.update_bar(&bar("110","90")).unwrap();   // range=20
        if let SignalValue::Scalar(v) = s.update_bar(&bar("105","95")).unwrap() {
            // range=10 → slope < 0
            assert!(v < dec!(0), "contracting ranges → negative slope: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rts_flat_ranges_near_zero_slope() {
        // Ranges: 20, 20, 20 — flat → slope ≈ 0
        let mut s = RangeTrendSlope::new("rts", 3).unwrap();
        s.update_bar(&bar("110","90")).unwrap();
        s.update_bar(&bar("110","90")).unwrap();
        if let SignalValue::Scalar(v) = s.update_bar(&bar("110","90")).unwrap() {
            assert!(v.abs() < dec!(0.001), "flat ranges → ~0 slope: {v}");
        } else {
            panic!("expected Scalar or Unavailable");
        }
    }

    #[test]
    fn test_rts_reset() {
        let mut s = RangeTrendSlope::new("rts", 3).unwrap();
        for (h, l) in &[("110","90"),("115","85"),("120","80")] {
            s.update_bar(&bar(h, l)).unwrap();
        }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
