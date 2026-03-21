//! Higher Timeframe Trend indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Higher Timeframe Trend.
///
/// Simulates a higher timeframe view by aggregating every `agg_period` bars into a
/// synthetic "higher timeframe bar" and computing a trend direction from the
/// last `num_htf_bars` synthetic bars.
///
/// Aggregation: every `agg_period` lower-timeframe bars form one HTF bar.
/// Each HTF bar's close is the last close in the group.
///
/// The trend is +1 if all consecutive HTF closes are rising, -1 if all are falling,
/// or the net sign of the HTF close-to-close returns.
///
/// Formula: `trend = sign(sum of htf returns over num_htf_bars - 1 pairs)`
///
/// Returns `SignalValue::Unavailable` until `agg_period * (num_htf_bars + 1)` bars seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::HigherTimeframeTrend;
/// use fin_primitives::signals::Signal;
/// let htt = HigherTimeframeTrend::new("htt_5_4", 5, 4).unwrap();
/// assert_eq!(htt.period(), 25); // 5 * (4+1)
/// ```
pub struct HigherTimeframeTrend {
    name: String,
    agg_period: usize,
    num_htf_bars: usize,
    bar_count: usize,
    current_group_close: Decimal,
    htf_closes: VecDeque<Decimal>,
}

impl HigherTimeframeTrend {
    /// Constructs a new `HigherTimeframeTrend`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `agg_period == 0` or `num_htf_bars < 2`.
    pub fn new(
        name: impl Into<String>,
        agg_period: usize,
        num_htf_bars: usize,
    ) -> Result<Self, FinError> {
        if agg_period == 0 {
            return Err(FinError::InvalidPeriod(agg_period));
        }
        if num_htf_bars < 2 {
            return Err(FinError::InvalidPeriod(num_htf_bars));
        }
        Ok(Self {
            name: name.into(),
            agg_period,
            num_htf_bars,
            bar_count: 0,
            current_group_close: Decimal::ZERO,
            htf_closes: VecDeque::with_capacity(num_htf_bars),
        })
    }
}

impl Signal for HigherTimeframeTrend {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.bar_count += 1;
        self.current_group_close = bar.close;

        if self.bar_count % self.agg_period == 0 {
            self.htf_closes.push_back(self.current_group_close);
            if self.htf_closes.len() > self.num_htf_bars {
                self.htf_closes.pop_front();
            }
        }

        if self.htf_closes.len() < self.num_htf_bars {
            return Ok(SignalValue::Unavailable);
        }

        let mut net: i32 = 0;
        for i in 0..self.htf_closes.len() - 1 {
            let diff = self.htf_closes[i + 1] - self.htf_closes[i];
            if diff > Decimal::ZERO {
                net += 1;
            } else if diff < Decimal::ZERO {
                net -= 1;
            }
        }

        let signal = if net > 0 {
            Decimal::ONE
        } else if net < 0 {
            Decimal::NEGATIVE_ONE
        } else {
            Decimal::ZERO
        };
        Ok(SignalValue::Scalar(signal))
    }

    fn is_ready(&self) -> bool {
        self.htf_closes.len() >= self.num_htf_bars
    }

    fn period(&self) -> usize {
        self.agg_period * (self.num_htf_bars)
    }

    fn reset(&mut self) {
        self.bar_count = 0;
        self.current_group_close = Decimal::ZERO;
        self.htf_closes.clear();
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
    fn test_invalid_params() {
        assert!(HigherTimeframeTrend::new("htt", 0, 3).is_err());
        assert!(HigherTimeframeTrend::new("htt", 3, 0).is_err());
        assert!(HigherTimeframeTrend::new("htt", 3, 1).is_err());
    }

    #[test]
    fn test_unavailable_before_enough_bars() {
        let mut htt = HigherTimeframeTrend::new("htt", 2, 3).unwrap();
        assert_eq!(htt.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_rising_trend_gives_one() {
        let mut htt = HigherTimeframeTrend::new("htt", 2, 3).unwrap();
        // 3 HTF bars, each formed from 2 LTF bars. Rising: 100, 110, 120
        // Bar 1-2 → HTF close=102
        htt.update_bar(&bar("100")).unwrap();
        htt.update_bar(&bar("102")).unwrap();
        // Bar 3-4 → HTF close=110
        htt.update_bar(&bar("105")).unwrap();
        htt.update_bar(&bar("110")).unwrap();
        // Bar 5-6 → HTF close=120
        htt.update_bar(&bar("115")).unwrap();
        let v = htt.update_bar(&bar("120")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_reset() {
        let mut htt = HigherTimeframeTrend::new("htt", 2, 2).unwrap();
        for _ in 0..4 {
            htt.update_bar(&bar("100")).unwrap();
        }
        assert!(htt.is_ready());
        htt.reset();
        assert!(!htt.is_ready());
    }
}
