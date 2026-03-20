//! Close Above SMA Streak — consecutive bar count where close exceeds the rolling SMA.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Close Above SMA Streak — count of consecutive bars where `close > SMA(period)`.
///
/// Tracks how long price has stayed above (or below) its moving average:
/// - **Positive N**: close has been above the SMA for N consecutive bars — sustained uptrend.
/// - **0**: close is at or below the SMA (streak just broke or has not yet formed).
///
/// The SMA is computed using a running sum for efficiency.
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseAboveSmaStreak;
/// use fin_primitives::signals::Signal;
/// let s = CloseAboveSmaStreak::new("css_20", 20).unwrap();
/// assert_eq!(s.period(), 20);
/// ```
pub struct CloseAboveSmaStreak {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
    streak: u32,
}

impl CloseAboveSmaStreak {
    /// Constructs a new `CloseAboveSmaStreak`.
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
            sum: Decimal::ZERO,
            streak: 0,
        })
    }
}

impl Signal for CloseAboveSmaStreak {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.sum += bar.close;
        self.window.push_back(bar.close);

        if self.window.len() > self.period {
            let removed = self.window.pop_front().unwrap();
            self.sum -= removed;
        }

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sma = self.sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        if bar.close > sma {
            self.streak += 1;
        } else {
            self.streak = 0;
        }

        Ok(SignalValue::Scalar(Decimal::from(self.streak)))
    }

    fn reset(&mut self) {
        self.window.clear();
        self.sum = Decimal::ZERO;
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
    fn test_css_invalid_period() {
        assert!(CloseAboveSmaStreak::new("css", 0).is_err());
    }

    #[test]
    fn test_css_unavailable_during_warmup() {
        let mut s = CloseAboveSmaStreak::new("css", 3).unwrap();
        assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
        assert!(!s.is_ready());
    }

    #[test]
    fn test_css_rising_streak() {
        // SMA(3) of [100,100,100] = 100; close=101 > 100 → streak=1
        // SMA(3) of [100,100,101] = 100.33; close=102 → streak=2
        let mut s = CloseAboveSmaStreak::new("css", 3).unwrap();
        s.update_bar(&bar("100")).unwrap();
        s.update_bar(&bar("100")).unwrap();
        let v1 = s.update_bar(&bar("101")).unwrap();
        let v2 = s.update_bar(&bar("102")).unwrap();
        if let (SignalValue::Scalar(s1), SignalValue::Scalar(s2)) = (v1, v2) {
            assert_eq!(s1, dec!(1), "first bar above SMA → streak=1");
            assert_eq!(s2, dec!(2), "second consecutive bar above SMA → streak=2");
        } else {
            panic!("expected Scalar values");
        }
    }

    #[test]
    fn test_css_streak_resets_on_drop() {
        let mut s = CloseAboveSmaStreak::new("css", 2).unwrap();
        s.update_bar(&bar("100")).unwrap();
        s.update_bar(&bar("110")).unwrap(); // SMA=105, close=110 → streak=1
        s.update_bar(&bar("115")).unwrap(); // SMA=112.5, close=115 → streak=2
        if let SignalValue::Scalar(v) = s.update_bar(&bar("100")).unwrap() {
            // SMA=(115+100)/2=107.5, close=100 < 107.5 → streak=0
            assert_eq!(v, dec!(0), "close below SMA resets streak to 0");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_css_reset() {
        let mut s = CloseAboveSmaStreak::new("css", 2).unwrap();
        for c in &["100","105","110","115"] { s.update_bar(&bar(c)).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
