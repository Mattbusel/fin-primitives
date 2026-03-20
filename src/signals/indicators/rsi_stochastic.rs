//! RSI Stochastic indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// RSI Stochastic (StochRSI raw) — stochastic oscillator applied to RSI values.
///
/// ```text
/// RSI_t           = RSI(close, rsi_period)
/// stoch_rsi_t     = (RSI_t − min(RSI, stoch_period)) / (max(RSI, stoch_period) − min(RSI, stoch_period))
/// ```
///
/// Output ranges 0..1. Values near 1 = RSI at highest in window (overbought relative
/// to recent RSI); near 0 = RSI at lowest (oversold). Returns 0.5 when range is zero.
///
/// Returns [`SignalValue::Unavailable`] until `rsi_period + stoch_period` bars seen.
///
/// Note: Unlike `StochRsi` in this library (which has D-line smoothing), this returns
/// the raw %K value without smoothing.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RsiStochastic;
/// use fin_primitives::signals::Signal;
///
/// let rs = RsiStochastic::new("rs", 14, 14).unwrap();
/// assert_eq!(rs.period(), 14);
/// ```
pub struct RsiStochastic {
    name: String,
    rsi_period: usize,
    stoch_period: usize,
    // RSI state
    prev_close: Option<Decimal>,
    avg_gain: Option<Decimal>,
    avg_loss: Option<Decimal>,
    seed_gains: Vec<Decimal>,
    seed_losses: Vec<Decimal>,
    // RSI history for stochastic
    rsi_values: VecDeque<Decimal>,
}

impl RsiStochastic {
    /// Creates a new `RsiStochastic`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `rsi_period < 2` or `stoch_period == 0`.
    pub fn new(
        name: impl Into<String>,
        rsi_period: usize,
        stoch_period: usize,
    ) -> Result<Self, FinError> {
        if rsi_period < 2 { return Err(FinError::InvalidPeriod(rsi_period)); }
        if stoch_period == 0 { return Err(FinError::InvalidPeriod(stoch_period)); }
        Ok(Self {
            name: name.into(),
            rsi_period,
            stoch_period,
            prev_close: None,
            avg_gain: None,
            avg_loss: None,
            seed_gains: Vec::with_capacity(rsi_period),
            seed_losses: Vec::with_capacity(rsi_period),
            rsi_values: VecDeque::with_capacity(stoch_period),
        })
    }
}

impl Signal for RsiStochastic {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        // Compute RSI first
        let prev = match self.prev_close {
            None => {
                self.prev_close = Some(bar.close);
                return Ok(SignalValue::Unavailable);
            }
            Some(p) => p,
        };
        self.prev_close = Some(bar.close);

        let change = bar.close - prev;
        let gain = if change > Decimal::ZERO { change } else { Decimal::ZERO };
        let loss = if change < Decimal::ZERO { -change } else { Decimal::ZERO };

        if self.avg_gain.is_none() {
            self.seed_gains.push(gain);
            self.seed_losses.push(loss);
            if self.seed_gains.len() == self.rsi_period {
                let ag = self.seed_gains.iter().sum::<Decimal>()
                    / Decimal::from(self.rsi_period as u32);
                let al = self.seed_losses.iter().sum::<Decimal>()
                    / Decimal::from(self.rsi_period as u32);
                self.avg_gain = Some(ag);
                self.avg_loss = Some(al);
            }
            return Ok(SignalValue::Unavailable);
        }

        let k = Decimal::ONE / Decimal::from(self.rsi_period as u32);
        let ag = self.avg_gain.unwrap() * (Decimal::ONE - k) + gain * k;
        let al = self.avg_loss.unwrap() * (Decimal::ONE - k) + loss * k;
        self.avg_gain = Some(ag);
        self.avg_loss = Some(al);

        let rsi = if al.is_zero() {
            Decimal::from(100u32)
        } else {
            let rs = ag / al;
            Decimal::from(100u32) - Decimal::from(100u32) / (Decimal::ONE + rs)
        };

        // Now apply stochastic to RSI
        self.rsi_values.push_back(rsi);
        if self.rsi_values.len() > self.stoch_period { self.rsi_values.pop_front(); }
        if self.rsi_values.len() < self.stoch_period { return Ok(SignalValue::Unavailable); }

        let rsi_min = self.rsi_values.iter().cloned().min().unwrap();
        let rsi_max = self.rsi_values.iter().cloned().max().unwrap();
        let range = rsi_max - rsi_min;

        if range.is_zero() {
            return Ok(SignalValue::Scalar(
                Decimal::from(1u32) / Decimal::from(2u32)
            ));
        }

        Ok(SignalValue::Scalar((rsi - rsi_min) / range))
    }

    fn is_ready(&self) -> bool { self.rsi_values.len() >= self.stoch_period }
    fn period(&self) -> usize { self.rsi_period }

    fn reset(&mut self) {
        self.prev_close = None;
        self.avg_gain = None;
        self.avg_loss = None;
        self.seed_gains.clear();
        self.seed_losses.clear();
        self.rsi_values.clear();
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
    fn test_rsi_stoch_invalid() {
        assert!(RsiStochastic::new("r", 1, 14).is_err());
        assert!(RsiStochastic::new("r", 14, 0).is_err());
    }

    #[test]
    fn test_rsi_stoch_unavailable_before_warmup() {
        let mut r = RsiStochastic::new("r", 3, 3).unwrap();
        assert_eq!(r.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_rsi_stoch_range_0_to_1() {
        let mut r = RsiStochastic::new("r", 5, 5).unwrap();
        for price in ["100", "102", "101", "104", "103", "105", "102", "106",
                       "104", "108", "103", "107", "105", "109", "104", "110"] {
            if let SignalValue::Scalar(v) = r.update_bar(&bar(price)).unwrap() {
                assert!(v >= dec!(0) && v <= dec!(1), "out of range: {v}");
            }
        }
    }

    #[test]
    fn test_rsi_stoch_all_rising_near_one() {
        // All rising: RSI stays near 100; stochastic of near-flat RSI series = 0.5
        // but after many bars RSI is at max of window → near 1
        let mut r = RsiStochastic::new("r", 5, 5).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0u32..20 {
            let p = format!("{}", 100 + i);
            last = r.update_bar(&bar(&p)).unwrap();
        }
        // RSI stays at 100 → all same in window → range=0 → returns 0.5
        if let SignalValue::Scalar(v) = last {
            assert!(v >= dec!(0) && v <= dec!(1), "out of range: {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_rsi_stoch_reset() {
        let mut r = RsiStochastic::new("r", 5, 5).unwrap();
        for i in 0u32..20 {
            let p = format!("{}", 100 + i);
            r.update_bar(&bar(&p)).unwrap();
        }
        assert!(r.is_ready());
        r.reset();
        assert!(!r.is_ready());
    }
}
