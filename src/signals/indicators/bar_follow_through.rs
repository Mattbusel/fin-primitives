//! Bar Follow-Through — rolling fraction of bars that continue prior bar's direction.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Bar Follow-Through — rolling fraction of bars where `close > prev_close` on a prior
/// up bar, or `close < prev_close` on a prior down bar.
///
/// Measures directional persistence over the last `period` bar-pairs:
/// - **Near 1.0**: strong trending behavior — price consistently follows through.
/// - **Near 0.5**: random walk — follow-through no better than chance.
/// - **Near 0.0**: strong mean-reversion — price consistently reverses.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BarFollowThrough;
/// use fin_primitives::signals::Signal;
/// let bft = BarFollowThrough::new("bft_10", 10).unwrap();
/// assert_eq!(bft.period(), 10);
/// ```
pub struct BarFollowThrough {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl BarFollowThrough {
    /// Constructs a new `BarFollowThrough`.
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
            closes: VecDeque::with_capacity(period + 1),
        })
    }
}

impl Signal for BarFollowThrough {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.closes.len() > self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() <= self.period {
            return Ok(SignalValue::Unavailable);
        }

        let closes: Vec<Decimal> = self.closes.iter().copied().collect();
        let mut follow_count = 0u32;
        let mut directional_pairs = 0u32;

        // For each consecutive triple (a, b, c): if (b-a) and (c-b) are same sign → follow-through
        for w in closes.windows(3) {
            let ab = w[1] - w[0];
            let bc = w[2] - w[1];
            if ab != Decimal::ZERO && bc != Decimal::ZERO {
                directional_pairs += 1;
                if (ab > Decimal::ZERO) == (bc > Decimal::ZERO) {
                    follow_count += 1;
                }
            }
        }

        if directional_pairs == 0 {
            return Ok(SignalValue::Unavailable);
        }

        let frac = Decimal::from(follow_count)
            .checked_div(Decimal::from(directional_pairs))
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(frac))
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
    fn test_bft_invalid_period() {
        assert!(BarFollowThrough::new("bft", 0).is_err());
    }

    #[test]
    fn test_bft_unavailable_before_warmup() {
        let mut s = BarFollowThrough::new("bft", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!s.is_ready());
    }

    #[test]
    fn test_bft_perfect_trend_gives_one() {
        // Monotone uptrend: every bar follows through
        let mut s = BarFollowThrough::new("bft", 3).unwrap();
        for p in &["100","101","102","103","104"] {
            s.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(v) = s.update_bar(&bar("105")).unwrap() {
            assert_eq!(v, dec!(1), "perfect trend should give 1.0: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_bft_perfect_reversal_gives_zero() {
        // Alternating pattern: 100, 102, 100, 102, 100
        let mut s = BarFollowThrough::new("bft", 3).unwrap();
        for p in &["100","102","100","102"] {
            s.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(v) = s.update_bar(&bar("100")).unwrap() {
            assert_eq!(v, dec!(0), "perfect reversal should give 0.0: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_bft_in_range_zero_to_one() {
        let mut s = BarFollowThrough::new("bft", 4).unwrap();
        let prices = ["100","102","101","103","102","104","103","105"];
        for p in &prices {
            if let SignalValue::Scalar(v) = s.update_bar(&bar(p)).unwrap() {
                assert!(v >= dec!(0) && v <= dec!(1), "value out of [0,1]: {v}");
            }
        }
    }

    #[test]
    fn test_bft_reset() {
        let mut s = BarFollowThrough::new("bft", 3).unwrap();
        for p in &["100","101","102","103","104"] { s.update_bar(&bar(p)).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
