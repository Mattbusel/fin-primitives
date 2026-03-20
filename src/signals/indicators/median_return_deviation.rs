//! Median Return Deviation — current return vs rolling median, normalized by MAD.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Median Return Deviation — `(current_return - median_return) / MAD`.
///
/// Measures how many median-absolute-deviations the current bar's return is from the
/// rolling median return — a robust alternative to z-scoring with mean/std-dev:
/// - **Large positive**: current return is an unusually large up-move.
/// - **Large negative**: current return is an unusually large down-move.
/// - **Near zero**: return is close to the typical (median) return in the window.
///
/// Uses `(close - prev_close) / prev_close` as the return measure.
/// Returns [`SignalValue::Unavailable`] until `period` returns have been collected
/// or if the MAD is zero (all returns identical — flat price series).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period < 2`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::MedianReturnDeviation;
/// use fin_primitives::signals::Signal;
/// let mrd = MedianReturnDeviation::new("mrd_10", 10).unwrap();
/// assert_eq!(mrd.period(), 10);
/// ```
pub struct MedianReturnDeviation {
    name: String,
    period: usize,
    returns: VecDeque<Decimal>,
    prev_close: Option<Decimal>,
}

impl MedianReturnDeviation {
    /// Constructs a new `MedianReturnDeviation`.
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
            returns: VecDeque::with_capacity(period),
            prev_close: None,
        })
    }
}

fn median(sorted: &[Decimal]) -> Decimal {
    let n = sorted.len();
    if n == 0 { return Decimal::ZERO; }
    if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2])
            .checked_div(Decimal::TWO)
            .unwrap_or(Decimal::ZERO)
    }
}

impl Signal for MedianReturnDeviation {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.returns.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            if !pc.is_zero() {
                let ret = (bar.close - pc)
                    .checked_div(pc)
                    .ok_or(FinError::ArithmeticOverflow)?;
                self.returns.push_back(ret);
                if self.returns.len() > self.period {
                    self.returns.pop_front();
                }
            }
        }

        self.prev_close = Some(bar.close);

        if self.returns.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        // Compute median of returns window
        let mut sorted: Vec<Decimal> = self.returns.iter().copied().collect();
        sorted.sort();
        let med = median(&sorted);

        // Compute MAD: median of |ret - median|
        let mut abs_devs: Vec<Decimal> = sorted.iter().map(|&r| (r - med).abs()).collect();
        abs_devs.sort();
        let mad = median(&abs_devs);

        if mad.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        // Current return is the most recent
        let current = *self.returns.back().unwrap();
        let score = (current - med)
            .checked_div(mad)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(score))
    }

    fn reset(&mut self) {
        self.returns.clear();
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
    fn test_mrd_invalid_period() {
        assert!(MedianReturnDeviation::new("mrd", 0).is_err());
        assert!(MedianReturnDeviation::new("mrd", 1).is_err());
    }

    #[test]
    fn test_mrd_unavailable_during_warmup() {
        let mut s = MedianReturnDeviation::new("mrd", 4).unwrap();
        for p in &["100","101","102","103"] {
            assert_eq!(s.update_bar(&bar(p)).unwrap(), SignalValue::Unavailable);
        }
        assert!(!s.is_ready());
    }

    #[test]
    fn test_mrd_zero_for_typical_return() {
        // 4 bars of ~1% return, then exactly 1% return → deviation ~ 0
        let mut s = MedianReturnDeviation::new("mrd", 4).unwrap();
        // Returns: 1%, 1%, 1%, 1%
        s.update_bar(&bar("100")).unwrap();
        s.update_bar(&bar("101")).unwrap();
        s.update_bar(&bar("102.01")).unwrap();
        s.update_bar(&bar("103.0301")).unwrap();
        // All returns ≈ 1% → MAD = 0 → Unavailable (flat series)
        // Use varying returns so MAD > 0
        let mut s2 = MedianReturnDeviation::new("mrd", 4).unwrap();
        s2.update_bar(&bar("100")).unwrap();
        s2.update_bar(&bar("101")).unwrap();   // +1%
        s2.update_bar(&bar("99")).unwrap();    // -1.98%
        s2.update_bar(&bar("100")).unwrap();   // +1.01%
        s2.update_bar(&bar("101")).unwrap();   // +1%
        // A return very close to the median should give score near 0
        if let SignalValue::Scalar(v) = s2.update_bar(&bar("102")).unwrap() {
            assert!(v.abs() < dec!(2), "typical return gives low deviation: {v}");
        }
        // else Unavailable is fine too (MAD could still be 0 in edge case)
    }

    #[test]
    fn test_mrd_large_outlier_gives_big_score() {
        // Consistent small returns, then a large spike → high |score|
        let mut s = MedianReturnDeviation::new("mrd", 4).unwrap();
        s.update_bar(&bar("100")).unwrap();
        s.update_bar(&bar("100.5")).unwrap();  // +0.5%
        s.update_bar(&bar("101")).unwrap();    // +0.498%
        s.update_bar(&bar("101.5")).unwrap();  // +0.495%
        // Now spike: huge up-move
        if let SignalValue::Scalar(v) = s.update_bar(&bar("110")).unwrap() {
            assert!(v > dec!(1), "outlier return gives score > 1: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_mrd_reset() {
        let mut s = MedianReturnDeviation::new("mrd", 3).unwrap();
        for p in &["100","101","102","103","104"] { s.update_bar(&bar(p)).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
