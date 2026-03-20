//! Close vs Prior High indicator -- ratio of current close to the N-period prior high.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Close vs Prior High -- how the current close compares to the highest high seen
/// over the prior `period` bars (excluding the current bar).
///
/// ```text
/// prior_high[t] = max(high[t-period..t-1])
/// ratio[t]      = close[t] / prior_high[t]
/// ```
///
/// A ratio above 1 means close broke above the prior-period high (bullish breakout).
/// Below 1 means close remains under the prior high.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen
/// (need to fill the prior-high window before comparing).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseVsPriorHigh;
/// use fin_primitives::signals::Signal;
/// let cvph = CloseVsPriorHigh::new("cvph", 20).unwrap();
/// assert_eq!(cvph.period(), 20);
/// ```
pub struct CloseVsPriorHigh {
    name: String,
    period: usize,
    prior_highs: VecDeque<Decimal>,
}

impl CloseVsPriorHigh {
    /// Constructs a new `CloseVsPriorHigh`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            prior_highs: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for CloseVsPriorHigh {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.prior_highs.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        // Compare close against the current prior-high window BEFORE updating
        let result = if self.prior_highs.len() >= self.period {
            let prior_max = self.prior_highs.iter().copied().fold(Decimal::MIN, Decimal::max);
            if prior_max.is_zero() {
                SignalValue::Unavailable
            } else {
                SignalValue::Scalar(bar.close / prior_max)
            }
        } else {
            SignalValue::Unavailable
        };

        // Then slide the window
        self.prior_highs.push_back(bar.high);
        if self.prior_highs.len() > self.period { self.prior_highs.pop_front(); }

        Ok(result)
    }

    fn reset(&mut self) {
        self.prior_highs.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, c: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        let low = cp.min(hp);
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cp, high: hp, low, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_cvph_period_0_error() { assert!(CloseVsPriorHigh::new("c", 0).is_err()); }

    #[test]
    fn test_cvph_unavailable_during_warmup() {
        let mut c = CloseVsPriorHigh::new("c", 3).unwrap();
        assert_eq!(c.update_bar(&bar("110", "100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(c.update_bar(&bar("110", "100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(c.update_bar(&bar("110", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_cvph_breakout_above_one() {
        let mut c = CloseVsPriorHigh::new("c", 3).unwrap();
        // seed 3 bars with high=100
        c.update_bar(&bar("100", "95")).unwrap();
        c.update_bar(&bar("100", "95")).unwrap();
        c.update_bar(&bar("100", "95")).unwrap();
        // 4th bar: prior high=100, close=110 -> ratio=1.1
        let v = c.update_bar(&bar("115", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(110) / dec!(100)));
    }

    #[test]
    fn test_cvph_below_prior_high() {
        let mut c = CloseVsPriorHigh::new("c", 3).unwrap();
        c.update_bar(&bar("100", "95")).unwrap();
        c.update_bar(&bar("100", "95")).unwrap();
        c.update_bar(&bar("100", "95")).unwrap();
        // close=90, prior high=100 -> ratio=0.9
        let v = c.update_bar(&bar("95", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(90) / dec!(100)));
    }

    #[test]
    fn test_cvph_reset() {
        let mut c = CloseVsPriorHigh::new("c", 2).unwrap();
        c.update_bar(&bar("100", "95")).unwrap();
        c.update_bar(&bar("100", "95")).unwrap();
        assert!(c.is_ready());
        c.reset();
        assert!(!c.is_ready());
    }
}
