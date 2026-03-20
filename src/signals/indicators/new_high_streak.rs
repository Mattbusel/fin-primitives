//! New High Streak — consecutive bars making a new N-period high.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// New High Streak — counts consecutive bars where `close > max(prior N closes)`.
///
/// Tracks how many bars in a row have closed at a new `lookback` period high:
/// - **Increasing**: price in a persistent breakout.
/// - **Resets to 0**: streak broken when close fails to make a new high.
///
/// Returns [`SignalValue::Unavailable`] until `lookback` bars have been accumulated
/// (needed to establish the reference maximum).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `lookback == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::NewHighStreak;
/// use fin_primitives::signals::Signal;
/// let nhs = NewHighStreak::new("nhs_20", 20).unwrap();
/// assert_eq!(nhs.period(), 20);
/// ```
pub struct NewHighStreak {
    name: String,
    lookback: usize,
    closes: VecDeque<Decimal>, // includes current
    streak: u32,
}

impl NewHighStreak {
    /// Constructs a new `NewHighStreak`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `lookback == 0`.
    pub fn new(name: impl Into<String>, lookback: usize) -> Result<Self, FinError> {
        if lookback == 0 {
            return Err(FinError::InvalidPeriod(lookback));
        }
        Ok(Self {
            name: name.into(),
            lookback,
            closes: VecDeque::with_capacity(lookback + 1),
            streak: 0,
        })
    }
}

impl Signal for NewHighStreak {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.lookback }
    fn is_ready(&self) -> bool { self.closes.len() > self.lookback }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.lookback + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() <= self.lookback {
            return Ok(SignalValue::Unavailable);
        }

        // Current close is the last; compare against max of prior `lookback` closes
        let current = *self.closes.back().unwrap();
        let prior_max = self.closes
            .iter()
            .take(self.lookback)
            .copied()
            .fold(Decimal::ZERO, Decimal::max);

        if current > prior_max {
            self.streak += 1;
        } else {
            self.streak = 0;
        }

        Ok(SignalValue::Scalar(Decimal::from(self.streak)))
    }

    fn reset(&mut self) {
        self.closes.clear();
        self.streak = 0;
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
    fn test_nhs_invalid_period() {
        assert!(NewHighStreak::new("nhs", 0).is_err());
    }

    #[test]
    fn test_nhs_unavailable_before_warmup() {
        let mut s = NewHighStreak::new("nhs", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!s.is_ready());
    }

    #[test]
    fn test_nhs_consecutive_highs() {
        // lookback=2: need 2 prior bars then current > max(prior 2)
        let mut s = NewHighStreak::new("nhs", 2).unwrap();
        s.update_bar(&bar("100")).unwrap();
        s.update_bar(&bar("101")).unwrap();
        // streak=0 initially after warmup
        assert_eq!(s.update_bar(&bar("102")).unwrap(), SignalValue::Scalar(dec!(1)));
        assert_eq!(s.update_bar(&bar("103")).unwrap(), SignalValue::Scalar(dec!(2)));
        assert_eq!(s.update_bar(&bar("104")).unwrap(), SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_nhs_streak_resets_on_pullback() {
        let mut s = NewHighStreak::new("nhs", 2).unwrap();
        s.update_bar(&bar("100")).unwrap();
        s.update_bar(&bar("101")).unwrap();
        s.update_bar(&bar("102")).unwrap(); // streak=1
        assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_nhs_no_negative() {
        let mut s = NewHighStreak::new("nhs", 2).unwrap();
        for p in &["105","100","102","99","103","98","104","97"] {
            if let SignalValue::Scalar(v) = s.update_bar(&bar(p)).unwrap() {
                assert!(v >= dec!(0), "streak cannot be negative: {v}");
            }
        }
    }

    #[test]
    fn test_nhs_reset() {
        let mut s = NewHighStreak::new("nhs", 2).unwrap();
        for p in &["100","101","102","103","104"] { s.update_bar(&bar(p)).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
