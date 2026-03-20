//! Up Momentum Percentage — rolling fraction of bars with meaningful upward price moves.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Up Momentum Percentage — fraction of bars in last `period` where `close > prev_close`.
///
/// A simple directional momentum count:
/// - **Near 1.0**: price consistently advancing — strong uptrend.
/// - **= 0.5**: equal up and down closes — balanced market.
/// - **Near 0.0**: price consistently declining — strong downtrend.
///
/// Returns [`SignalValue::Unavailable`] until `period` bar-pairs have been accumulated.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::UpMomentumPct;
/// use fin_primitives::signals::Signal;
/// let ump = UpMomentumPct::new("ump_10", 10).unwrap();
/// assert_eq!(ump.period(), 10);
/// ```
pub struct UpMomentumPct {
    name: String,
    period: usize,
    up_count: usize,
    directions: VecDeque<bool>, // true = up close
    prev_close: Option<Decimal>,
}

impl UpMomentumPct {
    /// Constructs a new `UpMomentumPct`.
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
            up_count: 0,
            directions: VecDeque::with_capacity(period),
            prev_close: None,
        })
    }
}

impl Signal for UpMomentumPct {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.directions.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let up = bar.close > pc;
            if up { self.up_count += 1; }
            self.directions.push_back(up);
            if self.directions.len() > self.period {
                let removed = self.directions.pop_front().unwrap();
                if removed { self.up_count -= 1; }
            }
        }
        self.prev_close = Some(bar.close);

        if self.directions.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let frac = Decimal::from(self.up_count as u32)
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(frac))
    }

    fn reset(&mut self) {
        self.up_count = 0;
        self.directions.clear();
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
    fn test_ump_invalid_period() {
        assert!(UpMomentumPct::new("ump", 0).is_err());
    }

    #[test]
    fn test_ump_unavailable_before_period() {
        let mut s = UpMomentumPct::new("ump", 3).unwrap();
        // First bar: no prev_close → no direction recorded
        assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
        assert!(!s.is_ready());
    }

    #[test]
    fn test_ump_all_up_gives_one() {
        let mut s = UpMomentumPct::new("ump", 3).unwrap();
        s.update_bar(&bar("100")).unwrap();
        s.update_bar(&bar("101")).unwrap();
        s.update_bar(&bar("102")).unwrap();
        if let SignalValue::Scalar(v) = s.update_bar(&bar("103")).unwrap() {
            assert_eq!(v, dec!(1), "all up bars should give 1.0: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ump_all_down_gives_zero() {
        let mut s = UpMomentumPct::new("ump", 3).unwrap();
        s.update_bar(&bar("103")).unwrap();
        s.update_bar(&bar("102")).unwrap();
        s.update_bar(&bar("101")).unwrap();
        if let SignalValue::Scalar(v) = s.update_bar(&bar("100")).unwrap() {
            assert_eq!(v, dec!(0), "all down bars should give 0.0: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ump_in_range() {
        let mut s = UpMomentumPct::new("ump", 4).unwrap();
        for p in &["100","102","101","103","102","104","101","103"] {
            if let SignalValue::Scalar(v) = s.update_bar(&bar(p)).unwrap() {
                assert!(v >= dec!(0) && v <= dec!(1), "fraction out of [0,1]: {v}");
            }
        }
    }

    #[test]
    fn test_ump_reset() {
        let mut s = UpMomentumPct::new("ump", 3).unwrap();
        for p in &["100","101","102","103","104"] { s.update_bar(&bar(p)).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
