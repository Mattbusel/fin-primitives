//! Open Above Prev Close — rolling fraction of bars that gap up at the open.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Open Above Prev Close — rolling fraction of bars where `open > prev_close`.
///
/// Measures how often the market gaps up on the open over the last `period` bars:
/// - **Near 1.0**: consistently gapping up — sustained buying interest at opens.
/// - **Near 0.5**: balanced gap-up and gap-down opens.
/// - **Near 0.0**: consistently gapping down — persistent selling at opens.
///
/// Returns [`SignalValue::Unavailable`] until `period` bar-pairs have been seen.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::OpenAbovePrevClose;
/// use fin_primitives::signals::Signal;
/// let oapc = OpenAbovePrevClose::new("oapc_10", 10).unwrap();
/// assert_eq!(oapc.period(), 10);
/// ```
pub struct OpenAbovePrevClose {
    name: String,
    period: usize,
    // Store (is_gap_up: bool) per bar
    gaps: VecDeque<bool>,
    gap_up_count: usize,
    prev_close: Option<Decimal>,
}

impl OpenAbovePrevClose {
    /// Constructs a new `OpenAbovePrevClose`.
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
            gaps: VecDeque::with_capacity(period),
            gap_up_count: 0,
            prev_close: None,
        })
    }
}

impl Signal for OpenAbovePrevClose {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.gaps.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let gap_up = bar.open > pc;

            if gap_up { self.gap_up_count += 1; }
            self.gaps.push_back(gap_up);

            if self.gaps.len() > self.period {
                let removed = self.gaps.pop_front().unwrap();
                if removed { self.gap_up_count -= 1; }
            }
        }

        self.prev_close = Some(bar.close);

        if self.gaps.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let frac = Decimal::from(self.gap_up_count as u32)
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(frac))
    }

    fn reset(&mut self) {
        self.gaps.clear();
        self.gap_up_count = 0;
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
    fn test_oapc_invalid_period() {
        assert!(OpenAbovePrevClose::new("oapc", 0).is_err());
    }

    #[test]
    fn test_oapc_unavailable_before_period() {
        let mut s = OpenAbovePrevClose::new("oapc", 3).unwrap();
        // First bar: no prev_close, gaps is empty → Unavailable
        assert_eq!(s.update_bar(&bar("100","102")).unwrap(), SignalValue::Unavailable);
        // Second bar: 1 gap pair recorded, but < period → Unavailable
        assert_eq!(s.update_bar(&bar("103","105")).unwrap(), SignalValue::Unavailable);
        assert!(!s.is_ready());
    }

    #[test]
    fn test_oapc_all_gap_up_gives_one() {
        let mut s = OpenAbovePrevClose::new("oapc", 2).unwrap();
        // Bar 1: close=100
        s.update_bar(&bar("100","100")).unwrap();
        // Bar 2: open=105 > 100 → gap up
        s.update_bar(&bar("105","108")).unwrap();
        // Bar 3: open=110 > 108 → gap up; window=[true,true]
        if let SignalValue::Scalar(v) = s.update_bar(&bar("110","112")).unwrap() {
            assert_eq!(v, dec!(1), "all gap-up should give 1.0: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_oapc_no_gap_up_gives_zero() {
        let mut s = OpenAbovePrevClose::new("oapc", 2).unwrap();
        s.update_bar(&bar("105","105")).unwrap();
        s.update_bar(&bar("103","103")).unwrap(); // open 103 < close 105 → gap down
        if let SignalValue::Scalar(v) = s.update_bar(&bar("101","101")).unwrap() { // open 101 < close 103 → gap down
            assert_eq!(v, dec!(0), "all gap-down should give 0.0: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_oapc_in_range_zero_to_one() {
        let mut s = OpenAbovePrevClose::new("oapc", 3).unwrap();
        let bars = [("100","102"),("103","101"),("99","100"),("101","103"),("102","100")];
        for (o,c) in &bars {
            if let SignalValue::Scalar(v) = s.update_bar(&bar(o,c)).unwrap() {
                assert!(v >= dec!(0) && v <= dec!(1), "fraction out of [0,1]: {v}");
            }
        }
    }

    #[test]
    fn test_oapc_reset() {
        let mut s = OpenAbovePrevClose::new("oapc", 2).unwrap();
        for _ in 0..4 { s.update_bar(&bar("100","102")).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
