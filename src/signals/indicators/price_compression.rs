//! Price Compression indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Compression — ratio of the current bar's range to the N-period average range.
///
/// ```text
/// bar_range   = high_t − low_t
/// avg_range   = mean(high − low, period)
/// compression = bar_range / avg_range
/// ```
///
/// Values < 1 indicate a compressed (inside) bar relative to recent history.
/// Values > 1 indicate an expanded (outside) bar — potential breakout signal.
/// Returns 1 when all bars have the same range.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceCompression;
/// use fin_primitives::signals::Signal;
///
/// let pc = PriceCompression::new("pc", 20).unwrap();
/// assert_eq!(pc.period(), 20);
/// ```
pub struct PriceCompression {
    name: String,
    period: usize,
    ranges: VecDeque<Decimal>,
}

impl PriceCompression {
    /// Creates a new `PriceCompression`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            ranges: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for PriceCompression {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        self.ranges.push_back(range);
        if self.ranges.len() > self.period { self.ranges.pop_front(); }
        if self.ranges.len() < self.period { return Ok(SignalValue::Unavailable); }

        let avg = self.ranges.iter().sum::<Decimal>() / Decimal::from(self.period as u32);
        if avg.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ONE));
        }
        Ok(SignalValue::Scalar(range / avg))
    }

    fn is_ready(&self) -> bool { self.ranges.len() >= self.period }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.ranges.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar_hl(h: &str, l: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: lp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_pc_invalid() {
        assert!(PriceCompression::new("p", 0).is_err());
    }

    #[test]
    fn test_pc_unavailable_before_warmup() {
        let mut p = PriceCompression::new("p", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(p.update_bar(&bar_hl("105", "95")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_pc_uniform_is_one() {
        // All bars same range → current range = avg → compression = 1
        let mut p = PriceCompression::new("p", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = p.update_bar(&bar_hl("110", "90")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(1));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_pc_small_bar_below_one() {
        // Feed wide bars then a narrow bar → compression < 1
        let mut p = PriceCompression::new("p", 3).unwrap();
        p.update_bar(&bar_hl("120", "80")).unwrap(); // range=40
        p.update_bar(&bar_hl("120", "80")).unwrap(); // range=40
        if let SignalValue::Scalar(v) = p.update_bar(&bar_hl("102", "100")).unwrap() {
            // range=2, avg=(40+40+2)/3=27.33 → comp=2/27.33≈0.073
            assert!(v < dec!(1), "expected < 1, got {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_pc_large_bar_above_one() {
        // Feed narrow bars then a wide bar → compression > 1
        let mut p = PriceCompression::new("p", 3).unwrap();
        p.update_bar(&bar_hl("101", "99")).unwrap(); // range=2
        p.update_bar(&bar_hl("101", "99")).unwrap(); // range=2
        if let SignalValue::Scalar(v) = p.update_bar(&bar_hl("120", "80")).unwrap() {
            // range=40, avg=(2+2+40)/3=14.67 → comp≈2.73
            assert!(v > dec!(1), "expected > 1, got {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_pc_reset() {
        let mut p = PriceCompression::new("p", 3).unwrap();
        for _ in 0..5 { p.update_bar(&bar_hl("105", "95")).unwrap(); }
        assert!(p.is_ready());
        p.reset();
        assert!(!p.is_ready());
    }
}
