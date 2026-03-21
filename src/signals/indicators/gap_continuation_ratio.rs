//! Gap Continuation Ratio indicator.
//!
//! Measures how often the opening gap is in alignment with the intraday
//! direction — i.e., gap-up AND close > open, or gap-down AND close < open.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Gap Continuation Ratio — rolling fraction of bars where the gap direction
/// matches the intraday session direction.
///
/// For each bar (after the first):
/// ```text
/// gap_up     = open > prev_close
/// gap_down   = open < prev_close
/// session_up = close > open
/// session_dn = close < open
///
/// continuation = 1  if (gap_up AND session_up) OR (gap_down AND session_dn)
///              = 0  otherwise (flat gap, flat session, or reversal)
/// ```
///
/// A high value (near 1) indicates the market tends to follow through in the
/// direction of the opening gap — momentum-aligned opens. A low value (near 0)
/// means gaps tend to fade intraday — mean-reversion opens.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars are collected
/// after the first bar (`period + 1` total bars).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::GapContinuationRatio;
/// use fin_primitives::signals::Signal;
/// let gcr = GapContinuationRatio::new("gcr_20", 20).unwrap();
/// assert_eq!(gcr.period(), 20);
/// ```
pub struct GapContinuationRatio {
    name: String,
    period: usize,
    flags: VecDeque<Decimal>,
    sum: Decimal,
    prev_close: Option<Decimal>,
}

impl GapContinuationRatio {
    /// Constructs a new `GapContinuationRatio`.
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
            flags: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
            prev_close: None,
        })
    }
}

impl Signal for GapContinuationRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.flags.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let pc = self.prev_close;
        self.prev_close = Some(bar.close);

        let Some(prev_close) = pc else {
            return Ok(SignalValue::Unavailable);
        };

        let gap_up = bar.open > prev_close;
        let gap_dn = bar.open < prev_close;
        let sess_up = bar.close > bar.open;
        let sess_dn = bar.close < bar.open;

        let flag = if (gap_up && sess_up) || (gap_dn && sess_dn) {
            Decimal::ONE
        } else {
            Decimal::ZERO
        };

        self.sum += flag;
        self.flags.push_back(flag);
        if self.flags.len() > self.period {
            let removed = self.flags.pop_front().unwrap();
            self.sum -= removed;
        }

        if self.flags.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let ratio = self.sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(ratio))
    }

    fn reset(&mut self) {
        self.flags.clear();
        self.sum = Decimal::ZERO;
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

    fn bar(o: &str, h: &str, l: &str, c: &str) -> OhlcvBar {
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(o.parse().unwrap()).unwrap(),
            high: Price::new(h.parse().unwrap()).unwrap(),
            low: Price::new(l.parse().unwrap()).unwrap(),
            close: Price::new(c.parse().unwrap()).unwrap(),
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_gcr_invalid_period() {
        assert!(GapContinuationRatio::new("gcr", 0).is_err());
    }

    #[test]
    fn test_gcr_first_bar_unavailable() {
        let mut gcr = GapContinuationRatio::new("gcr", 3).unwrap();
        // First bar has no prev_close
        assert_eq!(gcr.update_bar(&bar("100", "105", "98", "103")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_gcr_unavailable_during_warmup() {
        let mut gcr = GapContinuationRatio::new("gcr", 3).unwrap();
        // Bar 1: seed prev_close
        gcr.update_bar(&bar("100", "105", "98", "102")).unwrap();
        // Bars 2-3: flag computed but not enough for period=3
        gcr.update_bar(&bar("103", "108", "101", "107")).unwrap(); // gap_up+sess_up → 1
        gcr.update_bar(&bar("106", "110", "104", "109")).unwrap(); // gap_up+sess_up → 1
        assert!(!gcr.is_ready());
    }

    #[test]
    fn test_gcr_all_continuation_one() {
        // All bars: gap up AND close > open (continuation)
        let mut gcr = GapContinuationRatio::new("gcr", 3).unwrap();
        gcr.update_bar(&bar("100", "105", "98", "102")).unwrap();
        gcr.update_bar(&bar("103", "108", "101", "107")).unwrap(); // gap_up+sess_up
        gcr.update_bar(&bar("108", "113", "106", "112")).unwrap(); // gap_up+sess_up
        if let SignalValue::Scalar(v) = gcr.update_bar(&bar("113", "118", "111", "117")).unwrap() {
            assert_eq!(v, dec!(1));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_gcr_no_gaps_zero() {
        // No gap (open == prev_close): never gap_up or gap_dn → always 0
        let mut gcr = GapContinuationRatio::new("gcr", 3).unwrap();
        gcr.update_bar(&bar("100", "105", "98", "102")).unwrap();
        for _ in 0..4 {
            // open == prev_close in each case (simulate flat open)
            gcr.update_bar(&bar("102", "108", "100", "102")).unwrap();
        }
        if let SignalValue::Scalar(v) = gcr.update_bar(&bar("102", "108", "100", "102")).unwrap() {
            assert_eq!(v, dec!(0));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_gcr_reversal_zero() {
        // Gap up but session reverses (close < open) → no continuation
        let mut gcr = GapContinuationRatio::new("gcr", 3).unwrap();
        gcr.update_bar(&bar("100", "105", "98", "102")).unwrap();
        for _ in 0..4 {
            // gap_up: open=105 > prev_close=102, but sess_dn: close=103 < open=105
            gcr.update_bar(&bar("105", "108", "100", "103")).unwrap();
        }
        if let SignalValue::Scalar(v) = gcr.update_bar(&bar("105", "108", "100", "103")).unwrap() {
            assert_eq!(v, dec!(0));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_gcr_reset() {
        let mut gcr = GapContinuationRatio::new("gcr", 2).unwrap();
        gcr.update_bar(&bar("100", "105", "98", "103")).unwrap();
        gcr.update_bar(&bar("104", "108", "102", "107")).unwrap();
        gcr.update_bar(&bar("108", "112", "106", "110")).unwrap();
        assert!(gcr.is_ready());
        gcr.reset();
        assert!(!gcr.is_ready());
    }
}
