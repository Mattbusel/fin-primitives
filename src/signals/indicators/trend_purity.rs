//! Trend Purity indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Trend Purity — measures how "pure" the trend is by counting the fraction of
/// closes that move in the same direction as the overall period direction.
///
/// ```text
/// overall_direction = sign(close[last] - close[first])
/// purity = count(closes moving in overall_direction) / (period - 1)
/// ```
///
/// Returns a value in [0, 1]:
/// - `1.0` → all individual moves align with the trend  
/// - `0.0` → all individual moves oppose the trend  
/// - `0.5` → random/mixed  
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen,
/// or `0.5` if overall price change is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::TrendPurity;
/// use fin_primitives::signals::Signal;
///
/// let tp = TrendPurity::new("tp", 10).unwrap();
/// assert_eq!(tp.period(), 10);
/// ```
pub struct TrendPurity {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl TrendPurity {
    /// Constructs a new `TrendPurity`.
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
            closes: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for TrendPurity {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.closes.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period { self.closes.pop_front(); }

        if self.closes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let first = self.closes[0];
        let last = *self.closes.back().unwrap();
        let overall_diff = last - first;

        if overall_diff.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::new(5, 1))); // 0.5
        }

        let up_trend = overall_diff > Decimal::ZERO;
        let n = self.closes.len() - 1;
        let aligned = self.closes.iter().zip(self.closes.iter().skip(1))
            .filter(|(&a, &b)| {
                if up_trend { b > a } else { b < a }
            })
            .count();

        Ok(SignalValue::Scalar(Decimal::from(aligned as u32) / Decimal::from(n as u32)))
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

    fn bar(c: &str) -> OhlcvBar {
        let p = Price::new(c.parse().unwrap()).unwrap();
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
    fn test_tp_invalid_period() {
        assert!(TrendPurity::new("tp", 0).is_err());
        assert!(TrendPurity::new("tp", 1).is_err());
    }

    #[test]
    fn test_tp_unavailable_before_warm_up() {
        let mut tp = TrendPurity::new("tp", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(tp.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_tp_perfect_uptrend_gives_one() {
        let mut tp = TrendPurity::new("tp", 4).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0u32..4 {
            last = tp.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_tp_noisy_uptrend_below_one() {
        // 100, 101, 100, 103 → overall up, but bar 2→3 went down → 2/3 aligned
        let mut tp = TrendPurity::new("tp", 4).unwrap();
        let prices = ["100", "101", "100", "103"];
        let mut last = SignalValue::Unavailable;
        for p in &prices {
            last = tp.update_bar(&bar(p)).unwrap();
        }
        // aligned: 100→101 (up ✓), 101→100 (down ✗), 100→103 (up ✓) → 2/3
        if let SignalValue::Scalar(v) = last {
            assert!(v < dec!(1) && v > dec!(0), "noisy uptrend purity should be in (0,1): {}", v);
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_tp_reset() {
        let mut tp = TrendPurity::new("tp", 3).unwrap();
        for p in ["100", "101", "102"] { tp.update_bar(&bar(p)).unwrap(); }
        assert!(tp.is_ready());
        tp.reset();
        assert!(!tp.is_ready());
    }
}
