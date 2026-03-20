//! Price Channel Width indicator — N-period channel width as a percentage.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Channel Width — the percentage width of the N-period high/low channel.
///
/// Defined as `(period_high - period_low) / midpoint * 100`, where `midpoint` is
/// `(period_high + period_low) / 2`.
///
/// Useful for measuring how wide the recent trading range is:
/// - **Rising**: the channel is expanding (higher volatility).
/// - **Falling**: the channel is contracting (compression, possible breakout ahead).
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen, or when
/// the midpoint is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceChannelWidth;
/// use fin_primitives::signals::Signal;
/// let pcw = PriceChannelWidth::new("pcw_20", 20).unwrap();
/// assert_eq!(pcw.period(), 20);
/// ```
pub struct PriceChannelWidth {
    name: String,
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
}

impl PriceChannelWidth {
    /// Constructs a new `PriceChannelWidth`.
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
        })
    }
}

impl Signal for PriceChannelWidth {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.highs.len() >= self.period
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

        let max_high = self.highs.iter().copied().fold(Decimal::MIN, Decimal::max);
        let min_low = self.lows.iter().copied().fold(Decimal::MAX, Decimal::min);
        let midpoint = (max_high + min_low)
            .checked_div(Decimal::from(2u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        if midpoint.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let width_pct = (max_high - min_low)
            .checked_div(midpoint)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_mul(Decimal::ONE_HUNDRED)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(width_pct))
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

    fn bar(h: &str, l: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: hp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_pcw_invalid_period() {
        assert!(PriceChannelWidth::new("pcw", 0).is_err());
    }

    #[test]
    fn test_pcw_unavailable_before_period() {
        let mut pcw = PriceChannelWidth::new("pcw", 3).unwrap();
        assert_eq!(pcw.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert_eq!(pcw.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert!(!pcw.is_ready());
    }

    #[test]
    fn test_pcw_known_value() {
        // H=110, L=90 over 3 bars → max_high=110, min_low=90
        // midpoint = (110+90)/2 = 100, width = (110-90)/100*100 = 20%
        let mut pcw = PriceChannelWidth::new("pcw", 3).unwrap();
        for _ in 0..3 {
            pcw.update_bar(&bar("110", "90")).unwrap();
        }
        let v = pcw.update_bar(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(20)));
    }

    #[test]
    fn test_pcw_flat_market_zero_width() {
        let mut pcw = PriceChannelWidth::new("pcw", 3).unwrap();
        for _ in 0..3 {
            pcw.update_bar(&bar("100", "100")).unwrap();
        }
        let v = pcw.update_bar(&bar("100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_pcw_non_negative() {
        let mut pcw = PriceChannelWidth::new("pcw", 5).unwrap();
        let bars = [
            bar("105", "95"), bar("107", "97"), bar("103", "93"),
            bar("108", "98"), bar("106", "96"), bar("104", "94"),
        ];
        for b in &bars {
            if let SignalValue::Scalar(v) = pcw.update_bar(b).unwrap() {
                assert!(v >= dec!(0), "width should be non-negative: {v}");
            }
        }
    }

    #[test]
    fn test_pcw_reset() {
        let mut pcw = PriceChannelWidth::new("pcw", 2).unwrap();
        pcw.update_bar(&bar("110", "90")).unwrap();
        pcw.update_bar(&bar("110", "90")).unwrap();
        assert!(pcw.is_ready());
        pcw.reset();
        assert!(!pcw.is_ready());
    }

    #[test]
    fn test_pcw_period_and_name() {
        let pcw = PriceChannelWidth::new("my_pcw", 20).unwrap();
        assert_eq!(pcw.period(), 20);
        assert_eq!(pcw.name(), "my_pcw");
    }
}
