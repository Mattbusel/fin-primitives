//! Price Channel indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Channel — tracks the highest high and lowest low over a rolling window.
///
/// Returns the **midpoint** `(highest_high + lowest_low) / 2` as the scalar value.
/// Use [`PriceChannel::channel_high`] and [`PriceChannel::channel_low`] for the bands.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceChannel;
/// use fin_primitives::signals::Signal;
///
/// let pc = PriceChannel::new("pc20", 20).unwrap();
/// assert_eq!(pc.period(), 20);
/// assert!(!pc.is_ready());
/// ```
pub struct PriceChannel {
    name: String,
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
    channel_high: Option<Decimal>,
    channel_low: Option<Decimal>,
}

impl PriceChannel {
    /// Constructs a new `PriceChannel`.
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
            highs: VecDeque::with_capacity(period),
            lows: VecDeque::with_capacity(period),
            channel_high: None,
            channel_low: None,
        })
    }

    /// Returns the current channel high (highest high over `period` bars), or `None` if not ready.
    pub fn channel_high(&self) -> Option<Decimal> {
        self.channel_high
    }

    /// Returns the current channel low (lowest low over `period` bars), or `None` if not ready.
    pub fn channel_low(&self) -> Option<Decimal> {
        self.channel_low
    }

    /// Returns the channel width `(high - low)`, or `None` if not ready.
    pub fn channel_width(&self) -> Option<Decimal> {
        Some(self.channel_high? - self.channel_low?)
    }
}

impl Signal for PriceChannel {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);
        if self.highs.len() > self.period {
            self.highs.pop_front();
            self.lows.pop_front();
        }
        if self.highs.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let hi = self.highs.iter().copied().fold(Decimal::MIN, Decimal::max);
        let lo = self.lows.iter().copied().fold(Decimal::MAX, Decimal::min);
        self.channel_high = Some(hi);
        self.channel_low = Some(lo);
        let mid = (hi + lo) / Decimal::TWO;
        Ok(SignalValue::Scalar(mid))
    }

    fn is_ready(&self) -> bool {
        self.channel_high.is_some()
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.highs.clear();
        self.lows.clear();
        self.channel_high = None;
        self.channel_low = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: lp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_pc_invalid_period() {
        assert!(PriceChannel::new("pc", 0).is_err());
    }

    #[test]
    fn test_pc_unavailable_before_period() {
        let mut pc = PriceChannel::new("pc", 3).unwrap();
        assert_eq!(pc.update_bar(&bar("105", "95")).unwrap(), SignalValue::Unavailable);
        assert!(!pc.is_ready());
    }

    #[test]
    fn test_pc_midpoint_correct() {
        let mut pc = PriceChannel::new("pc", 3).unwrap();
        pc.update_bar(&bar("110", "90")).unwrap();
        pc.update_bar(&bar("115", "95")).unwrap();
        let v = pc.update_bar(&bar("108", "92")).unwrap();
        // high = 115, low = 90 → mid = 102.5
        assert_eq!(v, SignalValue::Scalar(dec!(102.5)));
        assert_eq!(pc.channel_high(), Some(dec!(115)));
        assert_eq!(pc.channel_low(), Some(dec!(90)));
        assert_eq!(pc.channel_width(), Some(dec!(25)));
    }

    #[test]
    fn test_pc_rolling_window() {
        // After adding a 4th bar that replaces the bar with the max high
        let mut pc = PriceChannel::new("pc", 3).unwrap();
        pc.update_bar(&bar("110", "90")).unwrap();
        pc.update_bar(&bar("105", "95")).unwrap();
        pc.update_bar(&bar("108", "92")).unwrap();
        // Now push bar with lower high — old max 110 slides out
        let v = pc.update_bar(&bar("107", "94")).unwrap();
        // window: bars 2,3,4 → highs [105,108,107] → max=108; lows [95,92,94] → min=92
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_pc_reset() {
        let mut pc = PriceChannel::new("pc", 3).unwrap();
        for _ in 0..3 { pc.update_bar(&bar("110", "90")).unwrap(); }
        assert!(pc.is_ready());
        pc.reset();
        assert!(!pc.is_ready());
        assert!(pc.channel_high().is_none());
        assert!(pc.channel_low().is_none());
    }
}
