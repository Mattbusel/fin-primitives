//! Dual RSI Divergence indicator (fast RSI minus slow RSI).

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Dual RSI Divergence — the difference between a fast RSI and a slow RSI.
///
/// ```text
/// dual_rsi = RSI(fast_period) − RSI(slow_period)
/// ```
///
/// Positive values indicate the fast RSI is running ahead of the slow RSI (momentum building);
/// negative values indicate the fast RSI is lagging (momentum fading).  The zero-line crossing
/// can serve as a trend-change signal.
///
/// Returns [`SignalValue::Unavailable`] until both RSIs have warmed up (`slow_period + 1` bars).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::DualRsi;
/// use fin_primitives::signals::Signal;
///
/// let d = DualRsi::new("dr", 6, 14).unwrap();
/// assert_eq!(d.period(), 14);
/// ```
pub struct DualRsi {
    name: String,
    fast_period: usize,
    slow_period: usize,
    fast_gains: VecDeque<Decimal>,
    fast_losses: VecDeque<Decimal>,
    slow_gains: VecDeque<Decimal>,
    slow_losses: VecDeque<Decimal>,
    prev_close: Option<Decimal>,
}

impl DualRsi {
    /// Creates a new `DualRsi`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if either period is zero or `fast_period >= slow_period`.
    pub fn new(name: impl Into<String>, fast_period: usize, slow_period: usize) -> Result<Self, FinError> {
        if fast_period == 0 || slow_period == 0 {
            return Err(FinError::InvalidPeriod(fast_period.min(slow_period)));
        }
        if fast_period >= slow_period {
            return Err(FinError::InvalidPeriod(fast_period));
        }
        Ok(Self {
            name: name.into(),
            fast_period,
            slow_period,
            fast_gains: VecDeque::with_capacity(slow_period),
            fast_losses: VecDeque::with_capacity(slow_period),
            slow_gains: VecDeque::with_capacity(slow_period),
            slow_losses: VecDeque::with_capacity(slow_period),
            prev_close: None,
        })
    }

    fn rsi(gains: &VecDeque<Decimal>, losses: &VecDeque<Decimal>, period: usize) -> Option<Decimal> {
        if gains.len() < period { return None; }
        let n = Decimal::from(period as u32);
        let avg_gain = gains.iter().sum::<Decimal>() / n;
        let avg_loss = losses.iter().sum::<Decimal>() / n;
        if avg_loss.is_zero() {
            return Some(Decimal::from(100u32));
        }
        let rs = avg_gain / avg_loss;
        Some(Decimal::from(100u32) - Decimal::from(100u32) / (Decimal::ONE + rs))
    }
}

impl Signal for DualRsi {
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

        // Push to both buffers
        self.fast_gains.push_back(gain);
        self.fast_losses.push_back(loss);
        self.slow_gains.push_back(gain);
        self.slow_losses.push_back(loss);

        if self.fast_gains.len() > self.fast_period {
            self.fast_gains.pop_front();
            self.fast_losses.pop_front();
        }
        if self.slow_gains.len() > self.slow_period {
            self.slow_gains.pop_front();
            self.slow_losses.pop_front();
        }

        let fast_rsi = match Self::rsi(&self.fast_gains, &self.fast_losses, self.fast_period) {
            None => return Ok(SignalValue::Unavailable),
            Some(r) => r,
        };
        let slow_rsi = match Self::rsi(&self.slow_gains, &self.slow_losses, self.slow_period) {
            None => return Ok(SignalValue::Unavailable),
            Some(r) => r,
        };

        Ok(SignalValue::Scalar(fast_rsi - slow_rsi))
    }

    fn is_ready(&self) -> bool {
        self.slow_gains.len() >= self.slow_period
    }

    fn period(&self) -> usize {
        self.slow_period
    }

    fn reset(&mut self) {
        self.fast_gains.clear();
        self.fast_losses.clear();
        self.slow_gains.clear();
        self.slow_losses.clear();
        self.prev_close = None;
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
    fn test_dual_rsi_invalid() {
        assert!(DualRsi::new("d", 0, 14).is_err());
        assert!(DualRsi::new("d", 14, 14).is_err()); // fast must be < slow
        assert!(DualRsi::new("d", 20, 14).is_err());
    }

    #[test]
    fn test_dual_rsi_unavailable_early() {
        let mut d = DualRsi::new("d", 3, 6).unwrap();
        for _ in 0..6 {
            assert_eq!(d.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_dual_rsi_produces_scalar_after_warmup() {
        let mut d = DualRsi::new("d", 3, 6).unwrap();
        let prices: Vec<String> = (100..115).map(|i| i.to_string()).collect();
        let mut last = SignalValue::Unavailable;
        for p in &prices { last = d.update_bar(&bar(p)).unwrap(); }
        assert!(matches!(last, SignalValue::Scalar(_)), "expected Scalar, got {last:?}");
    }

    #[test]
    fn test_dual_rsi_value_in_range() {
        // Result is always in [-100, 100]
        let mut d = DualRsi::new("d", 3, 6).unwrap();
        let prices = ["100", "102", "101", "104", "103", "106", "104", "107", "108", "110"];
        let mut last = SignalValue::Unavailable;
        for p in &prices { last = d.update_bar(&bar(p)).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert!(v >= dec!(-100) && v <= dec!(100), "out of range: {v}");
        }
    }

    #[test]
    fn test_dual_rsi_reset() {
        let mut d = DualRsi::new("d", 3, 6).unwrap();
        for p in &["100", "101", "102", "103", "104", "105", "106", "107"] {
            d.update_bar(&bar(p)).unwrap();
        }
        assert!(d.is_ready());
        d.reset();
        assert!(!d.is_ready());
        assert_eq!(d.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }
}
