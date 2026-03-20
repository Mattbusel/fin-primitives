//! SuperTrend indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// SuperTrend — ATR-based trailing stop that flips direction when price crosses the band.
///
/// ```text
/// basic_upper = (high + low) / 2 + multiplier × ATR(period)
/// basic_lower = (high + low) / 2 - multiplier × ATR(period)
/// trend = +1 when close > final_upper (bullish) else -1 (bearish)
/// ```
///
/// The scalar value returned is `+1.0` (bullish) or `-1.0` (bearish).
/// Use [`SuperTrend::trend_line`] to get the actual stop-loss level.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::SuperTrend;
/// use fin_primitives::signals::Signal;
/// use rust_decimal_macros::dec;
///
/// let st = SuperTrend::new("st14", 14, dec!(3)).unwrap();
/// assert_eq!(st.period(), 14);
/// assert!(!st.is_ready());
/// ```
pub struct SuperTrend {
    name: String,
    period: usize,
    multiplier: Decimal,
    bars: VecDeque<BarInput>,
    final_upper: Option<Decimal>,
    final_lower: Option<Decimal>,
    trend: Option<i8>,
}

impl SuperTrend {
    /// Constructs a new `SuperTrend`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize, multiplier: Decimal) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            multiplier,
            bars: VecDeque::with_capacity(period + 1),
            final_upper: None,
            final_lower: None,
            trend: None,
        })
    }

    /// Returns the current SuperTrend stop line level, or `None` if not ready.
    pub fn trend_line(&self) -> Option<Decimal> {
        match self.trend? {
            1 => self.final_lower,
            _ => self.final_upper,
        }
    }

    /// Returns `true` if the current trend is bullish (+1).
    pub fn is_bullish(&self) -> bool {
        self.trend == Some(1)
    }

    fn compute_atr(&self) -> Decimal {
        let n = self.bars.len().min(self.period);
        if n < 2 {
            let b = self.bars.back().unwrap();
            return b.high - b.low;
        }
        let len = self.bars.len();
        let start = len.saturating_sub(self.period + 1);
        let slice: Vec<&BarInput> = self.bars.range(start..).collect();
        let mut tr_sum = Decimal::ZERO;
        let mut count = 0u32;
        for w in slice.windows(2) {
            let prev_close = w[0].close;
            let high = w[1].high;
            let low = w[1].low;
            let tr = (high - low)
                .max((high - prev_close).abs())
                .max((low - prev_close).abs());
            tr_sum += tr;
            count += 1;
        }
        if count == 0 { return Decimal::ZERO; }
        tr_sum / Decimal::from(count)
    }
}

impl Signal for SuperTrend {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.bars.push_back(*bar);
        if self.bars.len() > self.period + 1 {
            self.bars.pop_front();
        }
        if self.bars.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let atr = self.compute_atr();
        let hl2 = (bar.high + bar.low) / Decimal::TWO;
        let basic_upper = hl2 + self.multiplier * atr;
        let basic_lower = hl2 - self.multiplier * atr;

        // Adjust bands to not move against the trend
        let prev_upper = self.final_upper.unwrap_or(basic_upper);
        let prev_lower = self.final_lower.unwrap_or(basic_lower);

        let new_upper = if basic_upper < prev_upper {
            basic_upper
        } else {
            prev_upper
        };
        let new_lower = if basic_lower > prev_lower {
            basic_lower
        } else {
            prev_lower
        };

        let prev_trend = self.trend.unwrap_or(1);
        let new_trend = if prev_trend == 1 {
            if bar.close < new_lower { -1i8 } else { 1i8 }
        } else if bar.close > new_upper {
            1i8
        } else {
            -1i8
        };

        self.final_upper = Some(new_upper);
        self.final_lower = Some(new_lower);
        self.trend = Some(new_trend);

        let signal_val = if new_trend == 1 { Decimal::ONE } else { -Decimal::ONE };
        Ok(SignalValue::Scalar(signal_val))
    }

    fn is_ready(&self) -> bool {
        self.trend.is_some()
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.bars.clear();
        self.final_upper = None;
        self.final_lower = None;
        self.trend = None;
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
    fn test_st_invalid_period() {
        assert!(SuperTrend::new("st", 0, dec!(3)).is_err());
    }

    #[test]
    fn test_st_unavailable_before_period() {
        let mut st = SuperTrend::new("st", 3, dec!(3)).unwrap();
        assert_eq!(st.update_bar(&bar("105", "95", "100")).unwrap(), SignalValue::Unavailable);
        assert!(!st.is_ready());
    }

    #[test]
    fn test_st_ready_after_period() {
        let mut st = SuperTrend::new("st", 3, dec!(3)).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 {
            last = st.update_bar(&bar("105", "95", "100")).unwrap();
        }
        assert!(st.is_ready());
        assert!(matches!(last, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_st_bullish_on_rising_prices() {
        let mut st = SuperTrend::new("st", 3, dec!(1)).unwrap();
        for i in 0..10 {
            let p = 100 + i * 5;
            let h = format!("{}", p + 2);
            let l = format!("{}", p - 2);
            let c = format!("{}", p);
            st.update_bar(&bar(&h, &l, &c)).unwrap();
        }
        assert!(st.is_bullish());
    }

    #[test]
    fn test_st_trend_line_present_when_ready() {
        let mut st = SuperTrend::new("st", 3, dec!(3)).unwrap();
        for _ in 0..5 { st.update_bar(&bar("105", "95", "100")).unwrap(); }
        assert!(st.trend_line().is_some());
    }

    #[test]
    fn test_st_reset() {
        let mut st = SuperTrend::new("st", 3, dec!(3)).unwrap();
        for _ in 0..5 { st.update_bar(&bar("105", "95", "100")).unwrap(); }
        assert!(st.is_ready());
        st.reset();
        assert!(!st.is_ready());
    }
}
