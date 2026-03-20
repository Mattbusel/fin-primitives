//! RSI Divergence indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// RSI Divergence — measures how far the current RSI value deviates from
/// a rolling simple moving average of RSI values.
///
/// ```text
/// RSI(rsi_period) computed each bar
/// divergence = RSI[now] - SMA(RSI, sma_period)
/// ```
///
/// Positive divergence means RSI is running above its own mean (momentum building up);
/// negative means RSI is below its mean (momentum waning).
///
/// Returns [`SignalValue::Unavailable`] until `rsi_period + sma_period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RsiDivergence;
/// use fin_primitives::signals::Signal;
///
/// let rd = RsiDivergence::new("rd", 14, 9).unwrap();
/// assert_eq!(rd.period(), 14);
/// ```
pub struct RsiDivergence {
    name: String,
    rsi_period: usize,
    sma_period: usize,
    // RSI state
    gains: VecDeque<Decimal>,
    losses: VecDeque<Decimal>,
    prev_close: Option<Decimal>,
    // RSI SMA buffer
    rsi_history: VecDeque<Decimal>,
}

impl RsiDivergence {
    /// Creates a new `RsiDivergence`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if either period is zero.
    pub fn new(name: impl Into<String>, rsi_period: usize, sma_period: usize) -> Result<Self, FinError> {
        if rsi_period == 0 {
            return Err(FinError::InvalidPeriod(rsi_period));
        }
        if sma_period == 0 {
            return Err(FinError::InvalidPeriod(sma_period));
        }
        Ok(Self {
            name: name.into(),
            rsi_period,
            sma_period,
            gains: VecDeque::with_capacity(rsi_period),
            losses: VecDeque::with_capacity(rsi_period),
            prev_close: None,
            rsi_history: VecDeque::with_capacity(sma_period),
        })
    }

    fn compute_rsi(&self) -> Option<Decimal> {
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

impl Signal for RsiDivergence {
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
        let gain = if change > Decimal::ZERO { change } else { Decimal::ZERO };
        let loss = if change < Decimal::ZERO { -change } else { Decimal::ZERO };

        self.gains.push_back(gain);
        self.losses.push_back(loss);
        if self.gains.len() > self.rsi_period {
            self.gains.pop_front();
            self.losses.pop_front();
        }

        let rsi = match self.compute_rsi() {
            None => return Ok(SignalValue::Unavailable),
            Some(r) => r,
        };

        self.rsi_history.push_back(rsi);
        if self.rsi_history.len() > self.sma_period {
            self.rsi_history.pop_front();
        }
        if self.rsi_history.len() < self.sma_period {
            return Ok(SignalValue::Unavailable);
        }

        let sma = self.rsi_history.iter().sum::<Decimal>() / Decimal::from(self.sma_period as u32);
        Ok(SignalValue::Scalar(rsi - sma))
    }

    fn is_ready(&self) -> bool {
        self.rsi_history.len() >= self.sma_period
    }

    fn period(&self) -> usize {
        self.rsi_period
    }

    fn reset(&mut self) {
        self.gains.clear();
        self.losses.clear();
        self.prev_close = None;
        self.rsi_history.clear();
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
    fn test_rsi_div_invalid_period() {
        assert!(RsiDivergence::new("r", 0, 9).is_err());
        assert!(RsiDivergence::new("r", 14, 0).is_err());
    }

    #[test]
    fn test_rsi_div_unavailable_early() {
        let mut rd = RsiDivergence::new("r", 3, 3).unwrap();
        // Need rsi_period + sma_period bars (at minimum) to produce first value
        for _ in 0..5 {
            assert_eq!(rd.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_rsi_div_scalar_after_warmup() {
        let mut rd = RsiDivergence::new("r", 3, 3).unwrap();
        let prices: Vec<String> = (100..115).map(|i| i.to_string()).collect();
        let mut last = SignalValue::Unavailable;
        for p in &prices { last = rd.update_bar(&bar(p)).unwrap(); }
        assert!(matches!(last, SignalValue::Scalar(_)), "expected Scalar, got {last:?}");
    }

    #[test]
    fn test_rsi_div_reset() {
        let mut rd = RsiDivergence::new("r", 3, 3).unwrap();
        for p in &["100", "101", "102", "103", "104", "105", "106", "107"] {
            rd.update_bar(&bar(p)).unwrap();
        }
        assert!(rd.is_ready());
        rd.reset();
        assert!(!rd.is_ready());
        assert_eq!(rd.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }
}
