//! Price Channel Position indicator -- where close sits in the N-period Donchian channel.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Channel Position -- close's percentile position in the N-period Donchian channel.
///
/// ```text
/// channel_high[t] = max(high, period)
/// channel_low[t]  = min(low,  period)
/// position[t]     = (close - channel_low) / (channel_high - channel_low) x 100
/// ```
///
/// 100 means close is at the top of the channel; 0 means at the bottom.
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen or when
/// the channel width is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceChannelPosition;
/// use fin_primitives::signals::Signal;
/// let pcp = PriceChannelPosition::new("pcp", 20).unwrap();
/// assert_eq!(pcp.period(), 20);
/// ```
pub struct PriceChannelPosition {
    name: String,
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
}

impl PriceChannelPosition {
    /// Constructs a new `PriceChannelPosition`.
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

impl Signal for PriceChannelPosition {
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

        let ch = self.highs.iter().copied().fold(Decimal::MIN, Decimal::max);
        let cl = self.lows.iter().copied().fold(Decimal::MAX, Decimal::min);
        let width = ch - cl;
        if width.is_zero() { return Ok(SignalValue::Unavailable); }
        let pos = (bar.close - cl) / width * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(pos))
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
    fn test_pcp_period_0_error() { assert!(PriceChannelPosition::new("p", 0).is_err()); }

    #[test]
    fn test_pcp_unavailable_before_period() {
        let mut p = PriceChannelPosition::new("p", 3).unwrap();
        assert_eq!(p.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_pcp_close_at_channel_high() {
        let mut p = PriceChannelPosition::new("p", 3).unwrap();
        p.update_bar(&bar("110", "90", "100")).unwrap();
        p.update_bar(&bar("110", "90", "100")).unwrap();
        // close=110 = channel_high -> position=100
        let v = p.update_bar(&bar("110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_pcp_close_at_channel_low() {
        let mut p = PriceChannelPosition::new("p", 3).unwrap();
        p.update_bar(&bar("110", "90", "100")).unwrap();
        p.update_bar(&bar("110", "90", "100")).unwrap();
        // close=90 = channel_low -> position=0
        let v = p.update_bar(&bar("110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_pcp_close_at_midpoint() {
        let mut p = PriceChannelPosition::new("p", 3).unwrap();
        p.update_bar(&bar("110", "90", "100")).unwrap();
        p.update_bar(&bar("110", "90", "100")).unwrap();
        // close=100, channel=[90,110], width=20 -> position=50
        let v = p.update_bar(&bar("110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_pcp_reset() {
        let mut p = PriceChannelPosition::new("p", 2).unwrap();
        p.update_bar(&bar("110", "90", "100")).unwrap();
        p.update_bar(&bar("110", "90", "100")).unwrap();
        assert!(p.is_ready());
        p.reset();
        assert!(!p.is_ready());
    }
}
