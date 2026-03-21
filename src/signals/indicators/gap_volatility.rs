//! Gap Volatility indicator.
//!
//! Rolling standard deviation of opening gap percentages, measuring how
//! erratic overnight price moves are.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use std::collections::VecDeque;
use rust_decimal::Decimal;

/// Gap Volatility — rolling standard deviation of `(open - prev_close) / prev_close × 100`.
///
/// Each bar's gap pct is:
/// ```text
/// gap_pct[i] = (open[i] - close[i-1]) / close[i-1] × 100
/// ```
///
/// The standard deviation over `period` gaps measures how variable overnight
/// moves are:
/// - **High value**: erratic gapping — news-driven or low-liquidity overnight sessions.
/// - **Low value**: consistent, small gaps — orderly overnight price behaviour.
///
/// Returns [`SignalValue::Unavailable`] until `period` gaps are collected
/// (`period + 1` bars).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period < 2`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::GapVolatility;
/// use fin_primitives::signals::Signal;
/// let gv = GapVolatility::new("gv_20", 20).unwrap();
/// assert_eq!(gv.period(), 20);
/// ```
pub struct GapVolatility {
    name: String,
    period: usize,
    gaps: VecDeque<f64>,
    prev_close: Option<f64>,
}

impl GapVolatility {
    /// Constructs a new `GapVolatility`.
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
            gaps: VecDeque::with_capacity(period),
            prev_close: None,
        })
    }
}

impl Signal for GapVolatility {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.gaps.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        use rust_decimal::prelude::ToPrimitive;

        let open = bar.open.to_f64().unwrap_or(0.0);
        let close = bar.close.to_f64().unwrap_or(0.0);

        if let Some(pc) = self.prev_close {
            if pc > 0.0 {
                let gap_pct = (open - pc) / pc * 100.0;
                self.gaps.push_back(gap_pct);
                if self.gaps.len() > self.period {
                    self.gaps.pop_front();
                }
            }
        }
        self.prev_close = Some(close);

        if self.gaps.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = self.gaps.len() as f64;
        let mean = self.gaps.iter().sum::<f64>() / n;
        let variance = self.gaps.iter().map(|g| (g - mean).powi(2)).sum::<f64>() / n;
        let std_dev = variance.sqrt();

        Decimal::try_from(std_dev)
            .map(SignalValue::Scalar)
            .map_err(|_| FinError::ArithmeticOverflow)
    }

    fn reset(&mut self) {
        self.gaps.clear();
        self.prev_close = None;
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
        let high = if cp > op { cp } else { op };
        let low = if cp < op { cp } else { op };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high, low, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_gv_invalid_period() {
        assert!(GapVolatility::new("gv", 0).is_err());
        assert!(GapVolatility::new("gv", 1).is_err());
    }

    #[test]
    fn test_gv_unavailable_during_warmup() {
        let mut gv = GapVolatility::new("gv", 3).unwrap();
        gv.update_bar(&bar("100", "102")).unwrap();
        gv.update_bar(&bar("102", "104")).unwrap();
        gv.update_bar(&bar("104", "106")).unwrap();
        assert!(!gv.is_ready());
    }

    #[test]
    fn test_gv_zero_gaps_zero_std() {
        // All opens exactly equal prior close → all gaps = 0 → std = 0
        let mut gv = GapVolatility::new("gv", 3).unwrap();
        gv.update_bar(&bar("100", "102")).unwrap();
        gv.update_bar(&bar("102", "104")).unwrap();
        gv.update_bar(&bar("104", "106")).unwrap();
        if let SignalValue::Scalar(v) = gv.update_bar(&bar("106", "108")).unwrap() {
            assert_eq!(v, dec!(0));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_gv_varied_gaps_positive() {
        // Mix of up and down gaps → non-zero std dev
        let mut gv = GapVolatility::new("gv", 3).unwrap();
        gv.update_bar(&bar("100", "100")).unwrap();
        gv.update_bar(&bar("105", "103")).unwrap(); // gap +5%
        gv.update_bar(&bar("98", "102")).unwrap();  // gap -4.85%
        if let SignalValue::Scalar(v) = gv.update_bar(&bar("110", "108")).unwrap() {
            // gap +7.84%; std of [5, -4.85, 7.84] > 0
            assert!(v > dec!(0), "varied gaps → positive std dev: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_gv_reset() {
        let mut gv = GapVolatility::new("gv", 2).unwrap();
        gv.update_bar(&bar("100", "100")).unwrap();
        gv.update_bar(&bar("100", "100")).unwrap();
        gv.update_bar(&bar("100", "100")).unwrap();
        assert!(gv.is_ready());
        gv.reset();
        assert!(!gv.is_ready());
    }
}
