//! Close-High Frequency indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Close-High Frequency — fraction of the last N bars where close >= midpoint of the bar's range.
///
/// ```text
/// midpoint = (high + low) / 2
/// score    = count(close >= midpoint over N bars) / N * 100
/// ```
///
/// - **Near 100**: closes consistently in the upper half of bars — bullish pressure.
/// - **Near 0**: closes consistently in the lower half — bearish pressure.
/// - **Near 50**: neutral/balanced.
/// - Bars with no range (`high == low`) count as "close at midpoint" (score contribution: 1).
/// - Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseHighFrequency;
/// use fin_primitives::signals::Signal;
///
/// let chf = CloseHighFrequency::new("chf", 10).unwrap();
/// assert_eq!(chf.period(), 10);
/// ```
pub struct CloseHighFrequency {
    name: String,
    period: usize,
    scores: VecDeque<bool>,
}

impl CloseHighFrequency {
    /// Constructs a new `CloseHighFrequency`.
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
            scores: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for CloseHighFrequency {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.scores.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let two = Decimal::TWO;
        let midpoint = (bar.high + bar.low) / two;
        let in_upper = bar.close >= midpoint;

        self.scores.push_back(in_upper);
        if self.scores.len() > self.period {
            self.scores.pop_front();
        }

        if self.scores.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let count = self.scores.iter().filter(|&&s| s).count();
        #[allow(clippy::cast_possible_truncation)]
        let pct = Decimal::from(count as u32)
            / Decimal::from(self.period as u32)
            * Decimal::ONE_HUNDRED;

        Ok(SignalValue::Scalar(pct))
    }

    fn reset(&mut self) {
        self.scores.clear();
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
    fn test_chf_invalid_period() {
        assert!(CloseHighFrequency::new("chf", 0).is_err());
    }

    #[test]
    fn test_chf_unavailable_during_warmup() {
        let mut chf = CloseHighFrequency::new("chf", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(chf.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!chf.is_ready());
    }

    #[test]
    fn test_chf_all_closes_above_mid() {
        // close=109 > midpoint=100, all bars → 100%
        let mut chf = CloseHighFrequency::new("chf", 3).unwrap();
        for _ in 0..3 {
            chf.update_bar(&bar("110", "90", "109")).unwrap();
        }
        let result = chf.update_bar(&bar("110", "90", "109")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_chf_all_closes_below_mid() {
        // close=91 < midpoint=100, all bars → 0%
        let mut chf = CloseHighFrequency::new("chf", 3).unwrap();
        for _ in 0..3 {
            chf.update_bar(&bar("110", "90", "91")).unwrap();
        }
        let result = chf.update_bar(&bar("110", "90", "91")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_chf_half_above_half_below() {
        // Alternating above/below midpoint over 4 bars → 50%
        let mut chf = CloseHighFrequency::new("chf", 4).unwrap();
        chf.update_bar(&bar("110", "90", "109")).unwrap(); // above
        chf.update_bar(&bar("110", "90", "91")).unwrap();  // below
        chf.update_bar(&bar("110", "90", "109")).unwrap(); // above
        let result = chf.update_bar(&bar("110", "90", "91")).unwrap();  // below
        assert_eq!(result, SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_chf_reset() {
        let mut chf = CloseHighFrequency::new("chf", 3).unwrap();
        for _ in 0..3 { chf.update_bar(&bar("110", "90", "105")).unwrap(); }
        assert!(chf.is_ready());
        chf.reset();
        assert!(!chf.is_ready());
    }
}
