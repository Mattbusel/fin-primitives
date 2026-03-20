//! Rolling Low Break indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling Low Break — measures how far the current low is below the rolling
/// `period`-bar low minimum, expressed as a multiple of ATR.
///
/// ```text
/// rolling_low = min(low, last n bars)
/// break_size  = max(0, rolling_low_prev - current_low) / ATR(n)
/// ```
///
/// A value of 0 means no new low; a positive value indicates how many ATRs
/// below the prior rolling low the current bar has pierced.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RollingLowBreak;
/// use fin_primitives::signals::Signal;
///
/// let rlb = RollingLowBreak::new("rlb", 14).unwrap();
/// assert_eq!(rlb.period(), 14);
/// ```
pub struct RollingLowBreak {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    lows: VecDeque<Decimal>,
    trs: VecDeque<Decimal>,
    tr_sum: Decimal,
}

impl RollingLowBreak {
    /// Constructs a new `RollingLowBreak`.
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
            prev_close: None,
            lows: VecDeque::with_capacity(period),
            trs: VecDeque::with_capacity(period),
            tr_sum: Decimal::ZERO,
        })
    }
}

impl Signal for RollingLowBreak {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.lows.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = bar.true_range(self.prev_close);
        self.prev_close = Some(bar.close);

        self.trs.push_back(tr);
        self.tr_sum += tr;
        if self.trs.len() > self.period {
            self.tr_sum -= self.trs.pop_front().unwrap();
        }

        // Record the low BEFORE this bar for comparison, then add current
        let prev_min = self.lows.iter().copied().fold(None::<Decimal>, |acc, v| {
            Some(acc.map_or(v, |a: Decimal| a.min(v)))
        });

        self.lows.push_back(bar.low);
        if self.lows.len() > self.period {
            self.lows.pop_front();
        }

        if self.lows.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let nd = Decimal::from(self.period as u32);
        let atr = self.tr_sum / nd;
        if atr.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let break_size = match prev_min {
            Some(pm) if bar.low < pm => (pm - bar.low) / atr,
            _ => Decimal::ZERO,
        };

        Ok(SignalValue::Scalar(break_size))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.lows.clear();
        self.trs.clear();
        self.tr_sum = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str, c: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_rlb_invalid_period() {
        assert!(RollingLowBreak::new("rlb", 0).is_err());
    }

    #[test]
    fn test_rlb_unavailable_before_warm_up() {
        let mut rlb = RollingLowBreak::new("rlb", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(rlb.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_rlb_no_break_gives_zero() {
        let mut rlb = RollingLowBreak::new("rlb", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        // Ascending lows → no break
        for i in 0u32..3 {
            last = rlb.update_bar(&bar("110", &(90 + i).to_string(), "100")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rlb_break_gives_positive() {
        let mut rlb = RollingLowBreak::new("rlb", 3).unwrap();
        rlb.update_bar(&bar("110", "90", "100")).unwrap();
        rlb.update_bar(&bar("110", "90", "100")).unwrap();
        // Now break below 90
        let result = rlb.update_bar(&bar("110", "80", "100")).unwrap();
        if let SignalValue::Scalar(v) = result {
            assert!(v > dec!(0), "break below rolling low should give positive value: {}", v);
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rlb_reset() {
        let mut rlb = RollingLowBreak::new("rlb", 3).unwrap();
        for _ in 0..3 { rlb.update_bar(&bar("110", "90", "100")).unwrap(); }
        assert!(rlb.is_ready());
        rlb.reset();
        assert!(!rlb.is_ready());
    }
}
