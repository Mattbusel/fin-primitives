//! RSI Moving Average indicator (smoothed RSI).

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// RSI Moving Average — applies a simple moving average to raw RSI values,
/// reducing whipsaws compared to using RSI directly.
///
/// ```text
/// RSI(rsi_period) computed each bar
/// RsiMa = SMA(RSI, ma_period)
/// ```
///
/// Returns [`SignalValue::Unavailable`] until enough bars have accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RsiMa;
/// use fin_primitives::signals::Signal;
///
/// let r = RsiMa::new("rsima", 14, 9).unwrap();
/// assert_eq!(r.period(), 14);
/// ```
pub struct RsiMa {
    name: String,
    rsi_period: usize,
    ma_period: usize,
    gains: VecDeque<Decimal>,
    losses: VecDeque<Decimal>,
    prev_close: Option<Decimal>,
    rsi_buf: VecDeque<Decimal>,
}

impl RsiMa {
    /// Creates a new `RsiMa`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if either period is zero.
    pub fn new(name: impl Into<String>, rsi_period: usize, ma_period: usize) -> Result<Self, FinError> {
        if rsi_period == 0 {
            return Err(FinError::InvalidPeriod(rsi_period));
        }
        if ma_period == 0 {
            return Err(FinError::InvalidPeriod(ma_period));
        }
        Ok(Self {
            name: name.into(),
            rsi_period,
            ma_period,
            gains: VecDeque::with_capacity(rsi_period),
            losses: VecDeque::with_capacity(rsi_period),
            prev_close: None,
            rsi_buf: VecDeque::with_capacity(ma_period),
        })
    }

    fn current_rsi(&self) -> Option<Decimal> {
        if self.gains.len() < self.rsi_period {
            return None;
        }
        let n = Decimal::from(self.rsi_period as u32);
        let avg_gain = self.gains.iter().sum::<Decimal>() / n;
        let avg_loss = self.losses.iter().sum::<Decimal>() / n;
        if avg_loss.is_zero() {
            return Some(Decimal::from(100u32));
        }
        let rs = avg_gain / avg_loss;
        Some(Decimal::from(100u32) - Decimal::from(100u32) / (Decimal::ONE + rs))
    }
}

impl Signal for RsiMa {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let close = bar.close;

        let prev = match self.prev_close {
            None => {
                self.prev_close = Some(close);
                return Ok(SignalValue::Unavailable);
            }
            Some(p) => p,
        };
        self.prev_close = Some(close);

        let change = close - prev;
        self.gains.push_back(if change > Decimal::ZERO { change } else { Decimal::ZERO });
        self.losses.push_back(if change < Decimal::ZERO { -change } else { Decimal::ZERO });
        if self.gains.len() > self.rsi_period {
            self.gains.pop_front();
            self.losses.pop_front();
        }

        let rsi = match self.current_rsi() {
            None => return Ok(SignalValue::Unavailable),
            Some(r) => r,
        };

        self.rsi_buf.push_back(rsi);
        if self.rsi_buf.len() > self.ma_period {
            self.rsi_buf.pop_front();
        }
        if self.rsi_buf.len() < self.ma_period {
            return Ok(SignalValue::Unavailable);
        }

        let sma = self.rsi_buf.iter().sum::<Decimal>() / Decimal::from(self.ma_period as u32);
        Ok(SignalValue::Scalar(sma))
    }

    fn is_ready(&self) -> bool {
        self.rsi_buf.len() >= self.ma_period
    }

    fn period(&self) -> usize {
        self.rsi_period
    }

    fn reset(&mut self) {
        self.gains.clear();
        self.losses.clear();
        self.prev_close = None;
        self.rsi_buf.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
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
    fn test_rsi_ma_invalid() {
        assert!(RsiMa::new("r", 0, 9).is_err());
        assert!(RsiMa::new("r", 14, 0).is_err());
    }

    #[test]
    fn test_rsi_ma_produces_scalar_after_warmup() {
        let mut r = RsiMa::new("r", 3, 3).unwrap();
        let prices: Vec<String> = (100..115).map(|i| i.to_string()).collect();
        let mut last = SignalValue::Unavailable;
        for p in &prices { last = r.update_bar(&bar(p)).unwrap(); }
        assert!(matches!(last, SignalValue::Scalar(_)), "expected Scalar, got {last:?}");
    }

    #[test]
    fn test_rsi_ma_uptrend_above_50() {
        let mut r = RsiMa::new("r", 3, 3).unwrap();
        let prices: Vec<String> = (100..120).map(|i| i.to_string()).collect();
        let mut last = SignalValue::Unavailable;
        for p in &prices { last = r.update_bar(&bar(p)).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(50), "uptrend RSI MA should be above 50: {v}");
        }
    }

    #[test]
    fn test_rsi_ma_range_0_100() {
        let mut r = RsiMa::new("r", 3, 3).unwrap();
        let prices = ["100", "105", "103", "108", "106", "110", "109", "112", "111", "115"];
        let mut last = SignalValue::Unavailable;
        for p in &prices { last = r.update_bar(&bar(p)).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert!(v >= dec!(0) && v <= dec!(100), "RSI MA out of range: {v}");
        }
    }

    #[test]
    fn test_rsi_ma_reset() {
        let mut r = RsiMa::new("r", 3, 3).unwrap();
        for p in &["100", "101", "102", "103", "104", "105", "106", "107"] {
            r.update_bar(&bar(p)).unwrap();
        }
        assert!(r.is_ready());
        r.reset();
        assert!(!r.is_ready());
        assert_eq!(r.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }
}
