//! Body Size Rank — percentile rank of current bar body size among the last N bars.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Body Size Rank — percentile rank of `|close - open|` relative to the prior `period` bars.
///
/// Returns how large the current bar's body is compared to recent history:
/// - **Near 1.0**: unusually large body — strong conviction move.
/// - **= 0.5**: median body size — typical bar.
/// - **Near 0.0**: tiny body / doji — indecision or low-volatility bar.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BodySizeRank;
/// use fin_primitives::signals::Signal;
/// let bsr = BodySizeRank::new("bsr_20", 20).unwrap();
/// assert_eq!(bsr.period(), 20);
/// ```
pub struct BodySizeRank {
    name: String,
    period: usize,
    bodies: VecDeque<Decimal>,
}

impl BodySizeRank {
    /// Constructs a new `BodySizeRank`.
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
            bodies: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for BodySizeRank {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.bodies.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let body = (bar.net_move()).abs();
        self.bodies.push_back(body);
        if self.bodies.len() > self.period {
            self.bodies.pop_front();
        }
        if self.bodies.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        // Rank: fraction of historical bodies strictly less than current
        let current = *self.bodies.back().unwrap();
        let n = (self.period - 1) as u32; // compare against prior period-1 bodies

        // Exclude the current bar from the comparison window
        let below = self.bodies
            .iter()
            .take(self.period - 1)
            .filter(|&&b| b < current)
            .count() as u32;

        if n == 0 {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let rank = Decimal::from(below)
            .checked_div(Decimal::from(n))
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(rank))
    }

    fn reset(&mut self) {
        self.bodies.clear();
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
    fn test_bsr_invalid_period() {
        assert!(BodySizeRank::new("bsr", 0).is_err());
    }

    #[test]
    fn test_bsr_unavailable_before_period() {
        let mut s = BodySizeRank::new("bsr", 3).unwrap();
        assert_eq!(s.update_bar(&bar("100","105")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("100","103")).unwrap(), SignalValue::Unavailable);
        assert!(!s.is_ready());
    }

    #[test]
    fn test_bsr_largest_body_gives_one() {
        // Prior bodies: 1, 2, 3. Current = 10 → all prior are below → rank = 1.0
        let mut s = BodySizeRank::new("bsr", 4).unwrap();
        s.update_bar(&bar("100","101")).unwrap(); // body=1
        s.update_bar(&bar("100","102")).unwrap(); // body=2
        s.update_bar(&bar("100","103")).unwrap(); // body=3
        if let SignalValue::Scalar(v) = s.update_bar(&bar("100","110")).unwrap() { // body=10
            assert_eq!(v, dec!(1), "largest body should rank 1.0: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_bsr_smallest_body_gives_zero() {
        // Prior bodies: 5, 8, 10. Current = 1 → none are below → rank = 0.0
        let mut s = BodySizeRank::new("bsr", 4).unwrap();
        s.update_bar(&bar("100","105")).unwrap();
        s.update_bar(&bar("100","108")).unwrap();
        s.update_bar(&bar("100","110")).unwrap();
        if let SignalValue::Scalar(v) = s.update_bar(&bar("100","101")).unwrap() {
            assert_eq!(v, dec!(0), "smallest body should rank 0.0: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_bsr_in_range() {
        let mut s = BodySizeRank::new("bsr", 3).unwrap();
        let bars = [("100","103"), ("100","107"), ("100","105"), ("100","102")];
        for (o, c) in &bars {
            if let SignalValue::Scalar(v) = s.update_bar(&bar(o, c)).unwrap() {
                assert!(v >= dec!(0) && v <= dec!(1), "rank out of [0,1]: {v}");
            }
        }
    }

    #[test]
    fn test_bsr_reset() {
        let mut s = BodySizeRank::new("bsr", 2).unwrap();
        s.update_bar(&bar("100","105")).unwrap();
        s.update_bar(&bar("100","108")).unwrap();
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
