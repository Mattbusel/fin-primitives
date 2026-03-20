//! HalfTrend indicator — ATR-channel-based trend direction signal.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// HalfTrend — tracks trend direction using ATR-based upper/lower channels.
///
/// Maintains a rolling ATR and rolling high/low over `period` bars. The trend is
/// considered **up** when price breaks above the lower channel and **down** when
/// price breaks below the upper channel.
///
/// Outputs `1` when the trend is up, `-1` when the trend is down.
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// `amplitude` scales the ATR channel width (default `2.0` is a common choice).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::HalfTrend;
/// use fin_primitives::signals::Signal;
/// let ht = HalfTrend::new("ht", 14, rust_decimal_macros::dec!(2)).unwrap();
/// assert_eq!(ht.period(), 14);
/// ```
pub struct HalfTrend {
    name: String,
    period: usize,
    amplitude: Decimal,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
    tr_values: VecDeque<Decimal>,
    prev_close: Option<Decimal>,
    /// Current trend: `true` = up, `false` = down.
    trend_up: bool,
    /// Whether the trend has been initialised.
    initialised: bool,
}

impl HalfTrend {
    /// Constructs a new `HalfTrend`.
    ///
    /// `amplitude` is the ATR multiplier for the channel width (e.g. `dec!(2)`).
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    /// Returns [`FinError::InvalidInput`] if `amplitude` is not positive.
    pub fn new(
        name: impl Into<String>,
        period: usize,
        amplitude: Decimal,
    ) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        if amplitude <= Decimal::ZERO {
            return Err(FinError::InvalidInput(
                "amplitude must be positive".to_owned(),
            ));
        }
        Ok(Self {
            name: name.into(),
            period,
            amplitude,
            highs: VecDeque::with_capacity(period),
            lows: VecDeque::with_capacity(period),
            tr_values: VecDeque::with_capacity(period),
            prev_close: None,
            trend_up: true,
            initialised: false,
        })
    }

    fn atr(&self) -> Option<Decimal> {
        if self.tr_values.len() < self.period {
            return None;
        }
        let sum: Decimal = self.tr_values.iter().copied().sum();
        #[allow(clippy::cast_possible_truncation)]
        sum.checked_div(Decimal::from(self.period as u32))
    }
}

impl Signal for HalfTrend {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.initialised
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        // Maintain rolling high/low window.
        self.highs.push_back(bar.high);
        if self.highs.len() > self.period {
            self.highs.pop_front();
        }
        self.lows.push_back(bar.low);
        if self.lows.len() > self.period {
            self.lows.pop_front();
        }

        // Compute true range (needs prev_close).
        let tr = match self.prev_close {
            None => {
                self.prev_close = Some(bar.close);
                return Ok(SignalValue::Unavailable);
            }
            Some(pc) => bar.true_range(Some(pc))
        };
        self.prev_close = Some(bar.close);

        self.tr_values.push_back(tr);
        if self.tr_values.len() > self.period {
            self.tr_values.pop_front();
        }

        let atr = match self.atr() {
            Some(a) => a,
            None => return Ok(SignalValue::Unavailable),
        };

        let rolling_high = self.highs.iter().copied().fold(Decimal::MIN, Decimal::max);
        let rolling_low = self.lows.iter().copied().fold(Decimal::MAX, Decimal::min);

        let upper_channel = rolling_high - atr * self.amplitude;
        let lower_channel = rolling_low + atr * self.amplitude;

        if !self.initialised {
            self.trend_up = bar.close >= upper_channel;
            self.initialised = true;
        } else if self.trend_up {
            // Switch to downtrend when price falls below the upper channel.
            if bar.close < upper_channel {
                self.trend_up = false;
            }
        } else {
            // Switch to uptrend when price rises above the lower channel.
            if bar.close > lower_channel {
                self.trend_up = true;
            }
        }

        let direction = if self.trend_up { Decimal::ONE } else { -Decimal::ONE };
        Ok(SignalValue::Scalar(direction))
    }

    fn reset(&mut self) {
        self.highs.clear();
        self.lows.clear();
        self.tr_values.clear();
        self.prev_close = None;
        self.trend_up = true;
        self.initialised = false;
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
    fn test_half_trend_invalid_period() {
        assert!(HalfTrend::new("ht", 0, dec!(2)).is_err());
    }

    #[test]
    fn test_half_trend_invalid_amplitude() {
        assert!(HalfTrend::new("ht", 3, dec!(0)).is_err());
        assert!(HalfTrend::new("ht", 3, dec!(-1)).is_err());
    }

    #[test]
    fn test_half_trend_unavailable_before_period() {
        let mut ht = HalfTrend::new("ht", 3, dec!(2)).unwrap();
        let v = ht.update_bar(&bar("100", "105", "95", "102")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
        assert!(!ht.is_ready());
    }

    #[test]
    fn test_half_trend_outputs_one_or_neg_one() {
        let mut ht = HalfTrend::new("ht", 3, dec!(2)).unwrap();
        let bars = [
            bar("100", "105", "95", "102"),
            bar("102", "107", "98", "105"),
            bar("105", "110", "102", "108"),
            bar("108", "112", "105", "110"),
        ];
        let mut last = SignalValue::Unavailable;
        for b in &bars {
            last = ht.update_bar(b).unwrap();
        }
        match last {
            SignalValue::Scalar(v) => {
                assert!(v == dec!(1) || v == dec!(-1));
            }
            SignalValue::Unavailable => panic!("expected a value"),
        }
    }

    #[test]
    fn test_half_trend_uptrend_on_rising_prices() {
        let mut ht = HalfTrend::new("ht", 2, dec!(0.5)).unwrap();
        // Feed strongly rising prices to force uptrend.
        for i in 0..5u32 {
            let base = 100 + i * 10;
            let b = bar(
                &base.to_string(),
                &(base + 5).to_string(),
                &(base - 1).to_string(),
                &(base + 4).to_string(),
            );
            ht.update_bar(&b).unwrap();
        }
        assert!(ht.is_ready());
    }

    #[test]
    fn test_half_trend_reset() {
        let mut ht = HalfTrend::new("ht", 2, dec!(2)).unwrap();
        for _ in 0..4 {
            ht.update_bar(&bar("100", "105", "95", "102")).unwrap();
        }
        assert!(ht.is_ready());
        ht.reset();
        assert!(!ht.is_ready());
        let v = ht.update_bar(&bar("100", "105", "95", "102")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_half_trend_period_and_name() {
        let ht = HalfTrend::new("myht", 14, dec!(2)).unwrap();
        assert_eq!(ht.period(), 14);
        assert_eq!(ht.name(), "myht");
    }
}
