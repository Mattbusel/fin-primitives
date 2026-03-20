//! Volume-Weighted Average Price (VWAP) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Running VWAP: `Σ(typical_price × volume) / Σ(volume)`.
///
/// Uses the typical price `(high + low + close) / 3` for each bar.
/// VWAP is always ready after the first bar with non-zero volume.
/// If cumulative volume is zero, returns `Unavailable`.
///
/// **Note:** This is a session-cumulative VWAP. Call [`Vwap::reset`] to start
/// a new session/period.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Vwap;
/// use fin_primitives::signals::Signal;
///
/// let vwap = Vwap::new("vwap");
/// assert_eq!(vwap.period(), 1);
/// ```
pub struct Vwap {
    name: String,
    cum_tp_vol: Decimal,
    cum_vol: Decimal,
}

impl Vwap {
    /// Constructs a new session `Vwap`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            cum_tp_vol: Decimal::ZERO,
            cum_vol: Decimal::ZERO,
        }
    }
}

impl Signal for Vwap {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tp = bar.typical_price();
        self.cum_tp_vol += tp * bar.volume;
        self.cum_vol += bar.volume;
        if self.cum_vol.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let vwap = self
            .cum_tp_vol
            .checked_div(self.cum_vol)
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(vwap))
    }

    fn is_ready(&self) -> bool {
        !self.cum_vol.is_zero()
    }

    fn period(&self) -> usize {
        1
    }

    fn reset(&mut self) {
        self.cum_tp_vol = Decimal::ZERO;
        self.cum_vol = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str, c: &str, vol: &str) -> OhlcvBar {
        let hi = Price::new(h.parse().unwrap()).unwrap();
        let lo = Price::new(l.parse().unwrap()).unwrap();
        let cl = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cl,
            high: hi,
            low: lo,
            close: cl,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vwap_zero_volume_returns_unavailable() {
        let mut v = Vwap::new("vwap");
        // volume=0 bar
        let b = {
            let p = Price::new(dec!(100)).unwrap();
            OhlcvBar {
                symbol: Symbol::new("X").unwrap(),
                open: p, high: p, low: p, close: p,
                volume: Quantity::zero(),
                ts_open: NanoTimestamp::new(0),
                ts_close: NanoTimestamp::new(1),
                tick_count: 1,
            }
        };
        assert_eq!(v.update_bar(&b).unwrap(), SignalValue::Unavailable);
        assert!(!v.is_ready());
    }

    #[test]
    fn test_vwap_single_bar_equals_typical_price() {
        // h=12, l=8, c=10 → tp=(12+8+10)/3 = 10, vol=5 → vwap=10
        let mut v = Vwap::new("vwap");
        let result = v.update_bar(&bar("12", "8", "10", "5")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(10)));
        assert!(v.is_ready());
    }

    #[test]
    fn test_vwap_two_bars_weighted() {
        // bar1: tp=10, vol=10 → cum_tv=100, cum_v=10
        // bar2: tp=20, vol=10 → cum_tv=300, cum_v=20 → vwap=15
        let mut v = Vwap::new("vwap");
        v.update_bar(&bar("10", "10", "10", "10")).unwrap();
        let result = v.update_bar(&bar("20", "20", "20", "10")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(15)));
    }

    #[test]
    fn test_vwap_reset_clears_state() {
        let mut v = Vwap::new("vwap");
        v.update_bar(&bar("10", "10", "10", "5")).unwrap();
        assert!(v.is_ready());
        v.reset();
        assert!(!v.is_ready());
    }

    #[test]
    fn test_vwap_constant_price_equals_price() {
        let mut v = Vwap::new("vwap");
        for _ in 0..5 {
            v.update_bar(&bar("100", "100", "100", "10")).unwrap();
        }
        let result = v.update_bar(&bar("100", "100", "100", "10")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(100)));
    }
}
