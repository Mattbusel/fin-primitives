//! Return Percent Rank — percentile rank of the current log return within the last N returns.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Return Percent Rank — percentile rank of the current log return in [0, 1].
///
/// On each bar, the log return `ln(close / prev_close)` is computed and ranked
/// against the previous `period - 1` log returns. A value of `1.0` means the
/// current return is the highest in the window; `0.0` means it is the lowest.
///
/// Formally: `rank = count(r in window[:-1] where r < current_return) / (period - 1)`.
///
/// Useful as a normalized momentum signal that is robust to regime changes in
/// volatility.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen
/// (one extra bar for the first log return), or when `period < 2`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ReturnPercentRank;
/// use fin_primitives::signals::Signal;
/// let rpr = ReturnPercentRank::new("rpr_20", 20).unwrap();
/// assert_eq!(rpr.period(), 20);
/// ```
pub struct ReturnPercentRank {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    returns: VecDeque<f64>,
}

impl ReturnPercentRank {
    /// Constructs a new `ReturnPercentRank`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            returns: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for ReturnPercentRank {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.returns.len() >= self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        use rust_decimal::prelude::ToPrimitive;

        let Some(prev) = self.prev_close else {
            self.prev_close = Some(bar.close);
            return Ok(SignalValue::Unavailable);
        };
        self.prev_close = Some(bar.close);

        if prev <= Decimal::ZERO || bar.close <= Decimal::ZERO {
            return Ok(SignalValue::Unavailable);
        }

        let prev_f = prev.to_f64().unwrap_or(0.0);
        let curr_f = bar.close.to_f64().unwrap_or(0.0);
        if prev_f <= 0.0 || curr_f <= 0.0 {
            return Ok(SignalValue::Unavailable);
        }

        let log_ret = (curr_f / prev_f).ln();

        self.returns.push_back(log_ret);
        if self.returns.len() > self.period {
            self.returns.pop_front();
        }

        if self.returns.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        // current return is the last element; compare against the previous (period - 1) returns
        let current = *self.returns.back().unwrap();
        let history = self.returns.iter().take(self.period - 1);
        let count_below = history.filter(|&&r| r < current).count();
        let rank = count_below as f64 / (self.period - 1) as f64;

        let result = Decimal::try_from(rank).unwrap_or(Decimal::ZERO);
        Ok(SignalValue::Scalar(result))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.returns.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(close: &str) -> OhlcvBar {
        let p = Price::new(close.parse().unwrap()).unwrap();
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
    fn test_rpr_invalid_period() {
        assert!(ReturnPercentRank::new("rpr", 0).is_err());
        assert!(ReturnPercentRank::new("rpr", 1).is_err());
    }

    #[test]
    fn test_rpr_unavailable_before_period() {
        let mut rpr = ReturnPercentRank::new("rpr", 3).unwrap();
        // Need period + 1 = 4 bars total before first Scalar
        for p in &["100", "101", "102"] {
            assert_eq!(rpr.update_bar(&bar(p)).unwrap(), SignalValue::Unavailable);
        }
        assert!(!rpr.is_ready());
    }

    #[test]
    fn test_rpr_output_in_unit_interval() {
        let mut rpr = ReturnPercentRank::new("rpr", 5).unwrap();
        let prices = ["100", "102", "101", "103", "99", "104", "100", "102"];
        for p in &prices {
            if let SignalValue::Scalar(v) = rpr.update_bar(&bar(p)).unwrap() {
                assert!(v >= dec!(0), "rank must be >= 0, got {v}");
                assert!(v <= dec!(1), "rank must be <= 1, got {v}");
            }
        }
    }

    #[test]
    fn test_rpr_highest_return_gives_one() {
        // Feed monotonically increasing prices, then a huge jump → rank should be 1.0
        let mut rpr = ReturnPercentRank::new("rpr", 4).unwrap();
        // Seed with small returns
        rpr.update_bar(&bar("100")).unwrap();
        rpr.update_bar(&bar("100.1")).unwrap();
        rpr.update_bar(&bar("100.2")).unwrap();
        rpr.update_bar(&bar("100.3")).unwrap();
        // Massive jump: far larger return than any previous
        let v = rpr.update_bar(&bar("200")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_rpr_lowest_return_gives_zero() {
        // Feed monotonically increasing prices, then a huge drop → rank should be 0.0
        let mut rpr = ReturnPercentRank::new("rpr", 4).unwrap();
        rpr.update_bar(&bar("100")).unwrap();
        rpr.update_bar(&bar("100.1")).unwrap();
        rpr.update_bar(&bar("100.2")).unwrap();
        rpr.update_bar(&bar("100.3")).unwrap();
        // Massive drop
        let v = rpr.update_bar(&bar("1")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rpr_reset() {
        let mut rpr = ReturnPercentRank::new("rpr", 3).unwrap();
        for p in &["100", "101", "102", "103"] {
            rpr.update_bar(&bar(p)).unwrap();
        }
        assert!(rpr.is_ready());
        rpr.reset();
        assert!(!rpr.is_ready());
    }

    #[test]
    fn test_rpr_period_and_name() {
        let rpr = ReturnPercentRank::new("my_rpr", 20).unwrap();
        assert_eq!(rpr.period(), 20);
        assert_eq!(rpr.name(), "my_rpr");
    }
}
