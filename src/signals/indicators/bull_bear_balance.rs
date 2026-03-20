//! Bull-Bear Balance — ratio of cumulative bullish body to cumulative bearish body over N bars.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Bull-Bear Balance — `sum(bullish bodies) / sum(bearish bodies)` over the last `period` bars.
///
/// Each bar contributes:
/// - To the bullish sum if `close > open`: contribution = `close - open`.
/// - To the bearish sum if `close < open`: contribution = `open - close`.
///
/// Interpretation:
/// - **> 1.0**: more cumulative bullish body → bullish bias.
/// - **= 1.0**: balanced.
/// - **< 1.0**: more cumulative bearish body → bearish bias.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen, or when
/// the total bearish body sum is zero (no bearish bars in window).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BullBearBalance;
/// use fin_primitives::signals::Signal;
/// let bbb = BullBearBalance::new("bbb", 10).unwrap();
/// assert_eq!(bbb.period(), 10);
/// ```
pub struct BullBearBalance {
    name: String,
    period: usize,
    window: VecDeque<(Decimal, Decimal)>, // (bull_body, bear_body)
    bull_sum: Decimal,
    bear_sum: Decimal,
}

impl BullBearBalance {
    /// Constructs a new `BullBearBalance`.
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
            bull_sum: Decimal::ZERO,
            bear_sum: Decimal::ZERO,
        })
    }
}

impl Signal for BullBearBalance {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let (bull, bear) = if bar.close > bar.open {
            (bar.close - bar.open, Decimal::ZERO)
        } else if bar.open > bar.close {
            (Decimal::ZERO, bar.open - bar.close)
        } else {
            (Decimal::ZERO, Decimal::ZERO)
        };

        self.bull_sum += bull;
        self.bear_sum += bear;
        self.window.push_back((bull, bear));

        if self.window.len() > self.period {
            let (ob, od) = self.window.pop_front().unwrap();
            self.bull_sum -= ob;
            self.bear_sum -= od;
        }

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        if self.bear_sum.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let ratio = self.bull_sum
            .checked_div(self.bear_sum)
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(ratio))
    }

    fn reset(&mut self) {
        self.window.clear();
        self.bull_sum = Decimal::ZERO;
        self.bear_sum = Decimal::ZERO;
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
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: cp.max(op), low: cp.min(op), close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_bbb_invalid_period() {
        assert!(BullBearBalance::new("bbb", 0).is_err());
    }

    #[test]
    fn test_bbb_unavailable_before_period() {
        let mut s = BullBearBalance::new("bbb", 3).unwrap();
        assert_eq!(s.update_bar(&bar("100", "105")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("105", "102")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_bbb_equal_bodies_gives_one() {
        let mut s = BullBearBalance::new("bbb", 2).unwrap();
        // bull=5, bear=5 → ratio=1
        s.update_bar(&bar("100", "105")).unwrap();
        let v = s.update_bar(&bar("105", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_bbb_all_bullish_unavailable() {
        let mut s = BullBearBalance::new("bbb", 3).unwrap();
        s.update_bar(&bar("100", "105")).unwrap();
        s.update_bar(&bar("105", "110")).unwrap();
        let v = s.update_bar(&bar("110", "115")).unwrap();
        // bear_sum=0 → Unavailable
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_bbb_bullish_bias_above_one() {
        let mut s = BullBearBalance::new("bbb", 3).unwrap();
        s.update_bar(&bar("100", "110")).unwrap(); // bull=10
        s.update_bar(&bar("110", "120")).unwrap(); // bull=10
        let v = s.update_bar(&bar("120", "115")).unwrap(); // bear=5; ratio=20/5=4
        if let SignalValue::Scalar(r) = v {
            assert!(r > dec!(1), "bullish bias should give ratio > 1: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_bbb_reset() {
        let mut s = BullBearBalance::new("bbb", 2).unwrap();
        s.update_bar(&bar("100", "105")).unwrap();
        s.update_bar(&bar("105", "100")).unwrap();
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
