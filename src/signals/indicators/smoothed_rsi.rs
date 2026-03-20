//! Smoothed RSI indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Smoothed RSI — RSI smoothed by a second EMA pass to reduce noise.
///
/// ```text
/// Step 1: compute standard RSI(rsi_period) using Wilder smoothing
/// Step 2: apply EMA(smooth_period) to the RSI values
/// ```
///
/// Values range 0-100. The EMA smoothing reduces false signals at the cost
/// of additional lag compared to standard RSI.
///
/// Returns [`SignalValue::Unavailable`] until both the RSI and its EMA are warm.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::SmoothedRsi;
/// use fin_primitives::signals::Signal;
///
/// let sr = SmoothedRsi::new("sr", 14, 3).unwrap();
/// assert_eq!(sr.period(), 14);
/// ```
pub struct SmoothedRsi {
    name: String,
    rsi_period: usize,
    smooth_period: usize,
    // RSI state
    prev_close: Option<Decimal>,
    avg_gain: Option<Decimal>,
    avg_loss: Option<Decimal>,
    seed_gains: Vec<Decimal>,
    seed_losses: Vec<Decimal>,
    // Smoothing EMA state
    ema: Option<Decimal>,
    ema_seed: Vec<Decimal>,
}

impl SmoothedRsi {
    /// Creates a new `SmoothedRsi`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `rsi_period < 2`.
    /// Returns [`FinError::InvalidInput`] if `smooth_period == 0`.
    pub fn new(
        name: impl Into<String>,
        rsi_period: usize,
        smooth_period: usize,
    ) -> Result<Self, FinError> {
        if rsi_period < 2 { return Err(FinError::InvalidPeriod(rsi_period)); }
        if smooth_period == 0 {
            return Err(FinError::InvalidInput("smooth_period must be > 0".into()));
        }
        Ok(Self {
            name: name.into(),
            rsi_period,
            smooth_period,
            prev_close: None,
            avg_gain: None,
            avg_loss: None,
            seed_gains: Vec::with_capacity(rsi_period),
            seed_losses: Vec::with_capacity(rsi_period),
            ema: None,
            ema_seed: Vec::with_capacity(smooth_period),
        })
    }

    fn compute_rsi(avg_gain: Decimal, avg_loss: Decimal) -> Decimal {
        if avg_loss.is_zero() {
            return Decimal::from(100u32);
        }
        let rs = avg_gain / avg_loss;
        Decimal::from(100u32) - Decimal::from(100u32) / (Decimal::ONE + rs)
    }
}

impl Signal for SmoothedRsi {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
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

        // RSI seeding phase
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

        // Wilder smoothing
        let k = Decimal::ONE / Decimal::from(self.rsi_period as u32);
        let ag = self.avg_gain.unwrap() * (Decimal::ONE - k) + gain * k;
        let al = self.avg_loss.unwrap() * (Decimal::ONE - k) + loss * k;
        self.avg_gain = Some(ag);
        self.avg_loss = Some(al);

        let rsi = Self::compute_rsi(ag, al);

        // Smoothing EMA
        let ema_k = Decimal::from(2u32) / Decimal::from((self.smooth_period + 1) as u32);

        if self.ema.is_none() {
            self.ema_seed.push(rsi);
            if self.ema_seed.len() == self.smooth_period {
                let sma = self.ema_seed.iter().sum::<Decimal>()
                    / Decimal::from(self.smooth_period as u32);
                self.ema = Some(sma);
            }
            return Ok(SignalValue::Unavailable);
        }

        let new_ema = self.ema.unwrap() + ema_k * (rsi - self.ema.unwrap());
        self.ema = Some(new_ema);
        Ok(SignalValue::Scalar(new_ema))
    }

    fn is_ready(&self) -> bool { self.ema.is_some() }
    fn period(&self) -> usize { self.rsi_period }

    fn reset(&mut self) {
        self.prev_close = None;
        self.avg_gain = None;
        self.avg_loss = None;
        self.seed_gains.clear();
        self.seed_losses.clear();
        self.ema = None;
        self.ema_seed.clear();
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
    fn test_srsi_invalid() {
        assert!(SmoothedRsi::new("s", 1, 3).is_err());
        assert!(SmoothedRsi::new("s", 14, 0).is_err());
    }

    #[test]
    fn test_srsi_unavailable_before_warmup() {
        let mut s = SmoothedRsi::new("s", 3, 3).unwrap();
        assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_srsi_range_0_to_100() {
        let mut s = SmoothedRsi::new("s", 5, 3).unwrap();
        for price in ["100", "102", "101", "104", "103", "106", "105", "108", "107", "110",
                       "109", "112", "111", "114", "113"] {
            if let SignalValue::Scalar(v) = s.update_bar(&bar(price)).unwrap() {
                assert!(v >= dec!(0) && v <= dec!(100), "out of range: {v}");
            }
        }
    }

    #[test]
    fn test_srsi_all_up_near_100() {
        // All rising bars → RSI ≈ 100 → SmoothedRSI ≈ 100
        let mut s = SmoothedRsi::new("s", 5, 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0u32..20 {
            let p = format!("{}", 100 + i);
            last = s.update_bar(&bar(&p)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(90), "expected near 100, got {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_srsi_all_down_near_zero() {
        let mut s = SmoothedRsi::new("s", 5, 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0u32..20 {
            let p = format!("{}", 200 - i);
            last = s.update_bar(&bar(&p)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v < dec!(10), "expected near 0, got {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_srsi_reset() {
        let mut s = SmoothedRsi::new("s", 5, 3).unwrap();
        for i in 0u32..20 {
            let p = format!("{}", 100 + i);
            s.update_bar(&bar(&p)).unwrap();
        }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
