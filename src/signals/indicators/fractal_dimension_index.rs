//! Fractal Dimension Index (FDI) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Fractal Dimension Index (Sevcik method).
///
/// Measures whether price is trending or moving randomly. FDI quantifies the
/// complexity of the price path: a value near 1.0 indicates a strongly trending
/// market; values near 1.5 indicate a random walk; values near 2.0 indicate a
/// highly oscillatory, noisy market.
///
/// Formula:
/// - `L1` = Σ |close[i] − close[i−1]|   (path length)
/// - `L2` = max_close − min_close        (straight-line end-to-end range)
/// - `FDI = 1 + (ln(L1) − ln(L2)) / ln(period − 1)`
///
/// Returns `SignalValue::Unavailable` until `period` bars have been accumulated.
/// Returns `SignalValue::Scalar(1.0)` (pure trend) when `L2 == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::FractalDimensionIndex;
/// use fin_primitives::signals::Signal;
/// let fdi = FractalDimensionIndex::new("fdi_30", 30).unwrap();
/// assert_eq!(fdi.period(), 30);
/// ```
pub struct FractalDimensionIndex {
    name: String,
    period: usize,
    closes: VecDeque<f64>,
}

impl FractalDimensionIndex {
    /// Constructs a new `FractalDimensionIndex` with the given name and period.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { name: name.into(), period, closes: VecDeque::with_capacity(period) })
    }
}

impl Signal for FractalDimensionIndex {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        use rust_decimal::prelude::ToPrimitive;
        let c = bar.close.to_f64().unwrap_or(0.0);
        self.closes.push_back(c);
        if self.closes.len() > self.period {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        // Path length: sum of absolute consecutive differences
        let mut l1 = 0.0_f64;
        let mut iter = self.closes.iter().peekable();
        let mut prev = *iter.next().unwrap();
        for &cur in iter {
            l1 += (cur - prev).abs();
            prev = cur;
        }

        // Straight-line range
        let max_c = self.closes.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let min_c = self.closes.iter().cloned().fold(f64::INFINITY, f64::min);
        let l2 = max_c - min_c;

        // Degenerate cases
        if l1 <= 0.0 {
            // Flat price — treat as perfect trend (FDI = 1.0)
            return Decimal::try_from(1.0_f64)
                .map(SignalValue::Scalar)
                .map_err(|_| FinError::ArithmeticOverflow);
        }
        if l2 <= 0.0 {
            // Oscillating around a constant — FDI = 1.0 (degenerate trend)
            return Decimal::try_from(1.0_f64)
                .map(SignalValue::Scalar)
                .map_err(|_| FinError::ArithmeticOverflow);
        }

        let fdi = 1.0 + (l1.ln() - l2.ln()) / (self.period as f64 - 1.0).ln();
        // Clamp to [1.0, 2.0]
        let fdi = fdi.clamp(1.0, 2.0);
        Decimal::try_from(fdi)
            .map(SignalValue::Scalar)
            .map_err(|_| FinError::ArithmeticOverflow)
    }

    fn is_ready(&self) -> bool {
        self.closes.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
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
            open: p,
            high: p,
            low: p,
            close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_period_too_small_fails() {
        assert!(matches!(
            FractalDimensionIndex::new("fdi", 1),
            Err(FinError::InvalidPeriod(1))
        ));
        assert!(matches!(
            FractalDimensionIndex::new("fdi", 0),
            Err(FinError::InvalidPeriod(0))
        ));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut fdi = FractalDimensionIndex::new("fdi", 5).unwrap();
        let v = fdi.update_bar(&bar("10")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
        assert!(!fdi.is_ready());
    }

    #[test]
    fn test_ready_after_period() {
        let mut fdi = FractalDimensionIndex::new("fdi", 3).unwrap();
        fdi.update_bar(&bar("10")).unwrap();
        fdi.update_bar(&bar("11")).unwrap();
        let v = fdi.update_bar(&bar("12")).unwrap();
        assert!(fdi.is_ready());
        assert!(matches!(v, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_trending_series_low_fdi() {
        // Strongly trending prices should give FDI closer to 1.0
        let mut fdi = FractalDimensionIndex::new("fdi", 10).unwrap();
        for i in 1..=10 {
            fdi.update_bar(&bar(&i.to_string())).unwrap();
        }
        let v = fdi.update_bar(&bar("11")).unwrap();
        if let SignalValue::Scalar(s) = v {
            // Trending: FDI should be < 1.5
            assert!(s < dec!(1.5), "FDI = {} should be < 1.5 for trending data", s);
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_value_in_range() {
        let mut fdi = FractalDimensionIndex::new("fdi", 5).unwrap();
        let prices = ["10", "12", "10", "12", "10"];
        for p in prices {
            fdi.update_bar(&bar(p)).unwrap();
        }
        let v = fdi.update_bar(&bar("12")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s >= dec!(1), "FDI must be >= 1");
            assert!(s <= dec!(2), "FDI must be <= 2");
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_reset_clears_state() {
        let mut fdi = FractalDimensionIndex::new("fdi", 3).unwrap();
        fdi.update_bar(&bar("10")).unwrap();
        fdi.update_bar(&bar("11")).unwrap();
        fdi.update_bar(&bar("12")).unwrap();
        assert!(fdi.is_ready());
        fdi.reset();
        assert!(!fdi.is_ready());
    }
}
