//! Trend Consistency — fraction of bar-to-bar moves aligned with the N-bar net direction.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Trend Consistency — fraction of consecutive bar moves in the same direction as
/// the N-bar net move, output in [0, 1].
///
/// For each completed window of `period` bars, the net direction is determined by
/// `close[last] vs close[first]`. Then each bar-over-bar step in the window that
/// matches this direction adds to the count.
///
/// - **1.0**: every step in the window aligned with the net trend (perfectly trending).
/// - **0.0**: no steps aligned with the net trend (maximum choppiness).
/// - **~0.5**: random / no directional consistency.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` closes have been seen
/// (need `period` bar-over-bar steps within a window of `period + 1` closes),
/// or when the net direction is flat (all closes equal).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::TrendConsistency;
/// use fin_primitives::signals::Signal;
/// let tc = TrendConsistency::new("tc_10", 10).unwrap();
/// assert_eq!(tc.period(), 10);
/// ```
pub struct TrendConsistency {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl TrendConsistency {
    /// Constructs a new `TrendConsistency`.
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
            closes: VecDeque::with_capacity(period + 1),
        })
    }
}

impl Signal for TrendConsistency {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.closes.len() > self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() <= self.period {
            return Ok(SignalValue::Unavailable);
        }

        let first = *self.closes.front().unwrap();
        let last = *self.closes.back().unwrap();
        let net = last - first;

        if net.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let is_up_trend = net > Decimal::ZERO;
        let steps = self.closes.len() - 1;
        let aligned = self.closes.iter().zip(self.closes.iter().skip(1)).filter(|(a, b)| {
            if is_up_trend { *b > *a } else { *b < *a }
        }).count();

        let ratio = Decimal::from(aligned as u32)
            .checked_div(Decimal::from(steps as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(ratio))
    }

    fn reset(&mut self) {
        self.closes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(close: &str) -> OhlcvBar {
        let p = Price::new(close.parse().unwrap()).unwrap();
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
    fn test_tc_invalid_period() {
        assert!(TrendConsistency::new("tc", 0).is_err());
        assert!(TrendConsistency::new("tc", 1).is_err());
    }

    #[test]
    fn test_tc_unavailable_before_period_plus_1() {
        let mut tc = TrendConsistency::new("tc", 3).unwrap();
        // Need period+1 = 4 closes for first result
        for p in &["100", "101", "102"] {
            assert_eq!(tc.update_bar(&bar(p)).unwrap(), SignalValue::Unavailable);
        }
        assert!(!tc.is_ready());
    }

    #[test]
    fn test_tc_perfect_uptrend_gives_one() {
        let mut tc = TrendConsistency::new("tc", 3).unwrap();
        tc.update_bar(&bar("100")).unwrap();
        tc.update_bar(&bar("101")).unwrap();
        tc.update_bar(&bar("102")).unwrap();
        let v = tc.update_bar(&bar("103")).unwrap();
        // Net direction = up; all 3 steps are up → consistency = 1.0
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_tc_alternating_with_net_up() {
        // up, down, up → net=up, 2/3 steps are up → consistency=2/3
        let mut tc = TrendConsistency::new("tc", 3).unwrap();
        tc.update_bar(&bar("100")).unwrap();
        tc.update_bar(&bar("102")).unwrap();
        tc.update_bar(&bar("101")).unwrap();
        let v = tc.update_bar(&bar("103")).unwrap();
        if let SignalValue::Scalar(r) = v {
            // steps: 100→102 (up), 102→101 (down), 101→103 (up). net=up. aligned=2/3
            assert!((r - dec!(2) / dec!(3)).abs() < dec!(0.0001), "expected 2/3, got {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_tc_output_in_unit_interval() {
        let mut tc = TrendConsistency::new("tc", 4).unwrap();
        let prices = ["100", "102", "101", "103", "102", "104", "103"];
        for p in &prices {
            if let SignalValue::Scalar(v) = tc.update_bar(&bar(p)).unwrap() {
                assert!(v >= dec!(0), "consistency must be >= 0: {v}");
                assert!(v <= dec!(1), "consistency must be <= 1: {v}");
            }
        }
    }

    #[test]
    fn test_tc_reset() {
        let mut tc = TrendConsistency::new("tc", 3).unwrap();
        for p in &["100", "101", "102", "103"] {
            tc.update_bar(&bar(p)).unwrap();
        }
        assert!(tc.is_ready());
        tc.reset();
        assert!(!tc.is_ready());
    }

    #[test]
    fn test_tc_period_and_name() {
        let tc = TrendConsistency::new("my_tc", 10).unwrap();
        assert_eq!(tc.period(), 10);
        assert_eq!(tc.name(), "my_tc");
    }
}
