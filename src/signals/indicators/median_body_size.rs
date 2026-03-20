//! Median Body Size — rolling median of bar body sizes.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Median Body Size — rolling median of `|close - open|` over `period` bars.
///
/// A robust measure of typical bar body magnitude that is resistant to outliers:
/// - **High**: bars are typically large-bodied — strong directional moves.
/// - **Low**: bars are typically small-bodied — doji-like or indecisive price action.
///
/// Unlike mean body size, the median is not distorted by occasional extreme bars
/// (e.g., earnings gaps or stop-hunts).
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::MedianBodySize;
/// use fin_primitives::signals::Signal;
/// let mbs = MedianBodySize::new("mbs_14", 14).unwrap();
/// assert_eq!(mbs.period(), 14);
/// ```
pub struct MedianBodySize {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
}

impl MedianBodySize {
    /// Constructs a new `MedianBodySize`.
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
        })
    }
}

impl Signal for MedianBodySize {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.body_size());
        if self.window.len() > self.period {
            self.window.pop_front();
        }

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let mut sorted: Vec<Decimal> = self.window.iter().copied().collect();
        sorted.sort();

        let n = sorted.len();
        let median = if n % 2 == 1 {
            sorted[n / 2]
        } else {
            (sorted[n / 2 - 1] + sorted[n / 2])
                .checked_div(Decimal::TWO)
                .ok_or(FinError::ArithmeticOverflow)?
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

    fn bar(o: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        let hp = if cp > op { cp } else { op };
        let lp = if cp < op { cp } else { op };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_mbs_invalid_period() {
        assert!(MedianBodySize::new("mbs", 0).is_err());
    }

    #[test]
    fn test_mbs_unavailable_during_warmup() {
        let mut s = MedianBodySize::new("mbs", 3).unwrap();
        assert_eq!(s.update_bar(&bar("100","103")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("103","101")).unwrap(), SignalValue::Unavailable);
        assert!(!s.is_ready());
    }

    #[test]
    fn test_mbs_known_median() {
        // Bodies: 5, 10, 15 → median = 10
        let mut s = MedianBodySize::new("mbs", 3).unwrap();
        s.update_bar(&bar("100","105")).unwrap();  // body=5
        s.update_bar(&bar("100","110")).unwrap();  // body=10
        if let SignalValue::Scalar(v) = s.update_bar(&bar("100","115")).unwrap() {
            // body=15, median of [5,10,15] = 10
            assert!((v - dec!(10)).abs() < dec!(0.001), "median of [5,10,15] = 10: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_mbs_outlier_resistant() {
        // Bodies: 5, 5, 100 → median = 5 (not 36.7 mean)
        let mut s = MedianBodySize::new("mbs", 3).unwrap();
        s.update_bar(&bar("100","105")).unwrap();   // body=5
        s.update_bar(&bar("100","105")).unwrap();   // body=5
        if let SignalValue::Scalar(v) = s.update_bar(&bar("100","200")).unwrap() {
            // body=100, median = 5
            assert!((v - dec!(5)).abs() < dec!(0.001), "median resists outlier: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_mbs_non_negative() {
        let mut s = MedianBodySize::new("mbs", 2).unwrap();
        for (o, c) in &[("100","98"),("98","102"),("102","101"),("101","105")] {
            if let SignalValue::Scalar(v) = s.update_bar(&bar(o, c)).unwrap() {
                assert!(v >= dec!(0), "median body must be non-negative: {v}");
            }
        }
    }

    #[test]
    fn test_mbs_reset() {
        let mut s = MedianBodySize::new("mbs", 2).unwrap();
        for (o, c) in &[("100","105"),("105","103"),("103","107")] {
            s.update_bar(&bar(o, c)).unwrap();
        }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
