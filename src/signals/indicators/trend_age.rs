//! Trend Age indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Trend Age — counts consecutive bars where `close` stays above or below the
/// Simple Moving Average (SMA) of the last `period` bars.
///
/// Outputs:
/// - `+n` → price has been above the SMA for `n` consecutive bars
/// - `-n` → price has been below the SMA for `n` consecutive bars
/// - `0` → price equals the SMA
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::TrendAge;
/// use fin_primitives::signals::Signal;
///
/// let ta = TrendAge::new("ta", 10).unwrap();
/// assert_eq!(ta.period(), 10);
/// ```
pub struct TrendAge {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
    consecutive: i64,
}

impl TrendAge {
    /// Constructs a new `TrendAge`.
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
            closes: VecDeque::with_capacity(period),
            consecutive: 0,
        })
    }

    /// Returns the current consecutive count (positive = above SMA, negative = below).
    pub fn consecutive(&self) -> i64 {
        self.consecutive
    }
}

impl Signal for TrendAge {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.closes.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        #[allow(clippy::cast_possible_truncation)]
        let nd = Decimal::from(self.period as u32);
        let sma: Decimal = self.closes.iter().sum::<Decimal>() / nd;
        let close = bar.close;

        self.consecutive = if close > sma {
            if self.consecutive >= 0 { self.consecutive + 1 } else { 1 }
        } else if close < sma {
            if self.consecutive <= 0 { self.consecutive - 1 } else { -1 }
        } else {
            0
        };

        Ok(SignalValue::Scalar(Decimal::from(self.consecutive)))
    }

    fn reset(&mut self) {
        self.closes.clear();
        self.consecutive = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(c: &str) -> OhlcvBar {
        let p = Price::new(c.parse().unwrap()).unwrap();
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
    fn test_ta_invalid_period() {
        assert!(TrendAge::new("ta", 0).is_err());
    }

    #[test]
    fn test_ta_unavailable_before_warm_up() {
        let mut ta = TrendAge::new("ta", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(ta.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!ta.is_ready());
    }

    #[test]
    fn test_ta_positive_in_uptrend() {
        let mut ta = TrendAge::new("ta", 3).unwrap();
        // Feed rising prices: SMA will lag below close
        for i in 0u32..6 {
            ta.update_bar(&bar(&(100 + i * 10).to_string())).unwrap();
        }
        assert!(ta.consecutive() > 0);
    }

    #[test]
    fn test_ta_negative_in_downtrend() {
        let mut ta = TrendAge::new("ta", 3).unwrap();
        for i in 0u32..6 {
            ta.update_bar(&bar(&(200 - i * 10).to_string())).unwrap();
        }
        assert!(ta.consecutive() < 0);
    }

    #[test]
    fn test_ta_reset() {
        let mut ta = TrendAge::new("ta", 3).unwrap();
        for i in 0u32..6 { ta.update_bar(&bar(&(100 + i).to_string())).unwrap(); }
        ta.reset();
        assert!(!ta.is_ready());
        assert_eq!(ta.consecutive(), 0);
    }
}
