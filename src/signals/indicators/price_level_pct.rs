//! Price Level Percent indicator -- close position within rolling period high-low range.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Level Percent -- close position within the rolling period's high-low range (0-100%).
///
/// ```text
/// period_high = max(high, period)
/// period_low  = min(low, period)
/// level[t]    = (close - period_low) / (period_high - period_low) * 100
/// ```
///
/// - 0%   → close at the lowest low of the period (bearish)
/// - 100% → close at the highest high of the period (bullish)
/// - 50%  → close at midpoint of the period range
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated
/// or if the period high equals the period low (flat market).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceLevelPct;
/// use fin_primitives::signals::Signal;
/// let pl = PriceLevelPct::new("pl", 20).unwrap();
/// assert_eq!(pl.period(), 20);
/// ```
pub struct PriceLevelPct {
    name: String,
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
}

impl PriceLevelPct {
    /// Constructs a new `PriceLevelPct`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            highs: VecDeque::with_capacity(period),
            lows: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for PriceLevelPct {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.highs.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);
        if self.highs.len() > self.period {
            self.highs.pop_front();
            self.lows.pop_front();
        }
        if self.highs.len() < self.period { return Ok(SignalValue::Unavailable); }
        let period_high = self.highs.iter().copied().fold(Decimal::MIN, Decimal::max);
        let period_low  = self.lows.iter().copied().fold(Decimal::MAX, Decimal::min);
        let range = period_high - period_low;
        if range.is_zero() { return Ok(SignalValue::Unavailable); }
        Ok(SignalValue::Scalar((bar.close - period_low) / range * Decimal::ONE_HUNDRED))
    }

    fn reset(&mut self) {
        self.highs.clear();
        self.lows.clear();
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
            open: cp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_pl_period_0_error() { assert!(PriceLevelPct::new("pl", 0).is_err()); }

    #[test]
    fn test_pl_unavailable_before_period() {
        let mut pl = PriceLevelPct::new("pl", 3).unwrap();
        assert_eq!(pl.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_pl_close_at_top_is_100() {
        let mut pl = PriceLevelPct::new("pl", 3).unwrap();
        pl.update_bar(&bar("110", "90", "100")).unwrap();
        pl.update_bar(&bar("110", "90", "100")).unwrap();
        // Period high=110, low=90; close=110 -> (110-90)/(110-90)*100 = 100
        let v = pl.update_bar(&bar("110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_pl_close_at_bottom_is_0() {
        let mut pl = PriceLevelPct::new("pl", 3).unwrap();
        pl.update_bar(&bar("110", "90", "100")).unwrap();
        pl.update_bar(&bar("110", "90", "100")).unwrap();
        let v = pl.update_bar(&bar("110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_pl_midpoint_is_50() {
        let mut pl = PriceLevelPct::new("pl", 2).unwrap();
        pl.update_bar(&bar("110", "90", "100")).unwrap();
        let v = pl.update_bar(&bar("110", "90", "100")).unwrap();
        // period_high=110, period_low=90, close=100 -> (100-90)/20*100 = 50
        assert_eq!(v, SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_pl_flat_market_unavailable() {
        let mut pl = PriceLevelPct::new("pl", 2).unwrap();
        pl.update_bar(&bar("100", "100", "100")).unwrap();
        let v = pl.update_bar(&bar("100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_pl_reset() {
        let mut pl = PriceLevelPct::new("pl", 2).unwrap();
        pl.update_bar(&bar("110", "90", "100")).unwrap();
        pl.update_bar(&bar("110", "90", "100")).unwrap();
        assert!(pl.is_ready());
        pl.reset();
        assert!(!pl.is_ready());
    }
}
