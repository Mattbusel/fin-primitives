//! Keltner Channel indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Keltner Channel — EMA-based dynamic band indicator.
///
/// This indicator returns the **middle band** (EMA of close).
/// Upper = middle + `multiplier × ATR`, Lower = middle - `multiplier × ATR`.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::KeltnerChannel;
/// use fin_primitives::signals::Signal;
///
/// let mut kc = KeltnerChannel::new("kc20", 20, rust_decimal_macros::dec!(2)).unwrap();
/// ```
pub struct KeltnerChannel {
    name: String,
    period: usize,
    multiplier: Decimal,
    // EMA state
    ema_k: Decimal,
    ema_count: usize,
    ema_seed_sum: Decimal,
    ema: Option<Decimal>,
    // ATR state
    atr_k: Decimal,
    atr_count: usize,
    atr_seed_sum: Decimal,
    atr: Option<Decimal>,
    prev_close: Option<Decimal>,
}

impl KeltnerChannel {
    /// Constructs a new `KeltnerChannel`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize, multiplier: Decimal) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        #[allow(clippy::cast_possible_truncation)]
        let denom = Decimal::from((period + 1) as u32);
        let k = Decimal::TWO.checked_div(denom).unwrap_or(Decimal::ONE);
        Ok(Self {
            name: name.into(),
            period,
            multiplier,
            ema_k: k,
            ema_count: 0,
            ema_seed_sum: Decimal::ZERO,
            ema: None,
            atr_k: k,
            atr_count: 0,
            atr_seed_sum: Decimal::ZERO,
            atr: None,
            prev_close: None,
        })
    }

    /// Returns the ATR multiplier used for upper/lower band calculations.
    pub fn multiplier(&self) -> Decimal {
        self.multiplier
    }

    fn ema_update(
        count: &mut usize,
        seed_sum: &mut Decimal,
        state: &mut Option<Decimal>,
        period: usize,
        k: Decimal,
        value: Decimal,
    ) -> Option<Decimal> {
        *count += 1;
        if *count <= period {
            *seed_sum += value;
            if *count == period {
                #[allow(clippy::cast_possible_truncation)]
                let seed = *seed_sum / Decimal::from(period as u32);
                *state = Some(seed);
                return Some(seed);
            }
            return None;
        }
        let prev = state.unwrap_or(Decimal::ZERO);
        let new_val = value * k + prev * (Decimal::ONE - k);
        *state = Some(new_val);
        Some(new_val)
    }
}

impl Signal for KeltnerChannel {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        // EMA of close
        let ema_val = Self::ema_update(
            &mut self.ema_count, &mut self.ema_seed_sum, &mut self.ema,
            self.period, self.ema_k, bar.close,
        );

        // True range
        let tr = if let Some(pc) = self.prev_close {
            let hl = bar.range();
            let hpc = (bar.high - pc).abs();
            let lpc = (bar.low - pc).abs();
            hl.max(hpc).max(lpc)
        } else {
            bar.range()
        };
        self.prev_close = Some(bar.close);

        let atr_val = Self::ema_update(
            &mut self.atr_count, &mut self.atr_seed_sum, &mut self.atr,
            self.period, self.atr_k, tr,
        );

        match (ema_val, atr_val) {
            (Some(_middle), Some(_atr)) => Ok(SignalValue::Scalar(_middle)),
            _ => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool {
        self.ema.is_some() && self.atr.is_some()
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.ema_count = 0;
        self.ema_seed_sum = Decimal::ZERO;
        self.ema = None;
        self.atr_count = 0;
        self.atr_seed_sum = Decimal::ZERO;
        self.atr = None;
        self.prev_close = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str, c: &str) -> OhlcvBar {
        let h_p = Price::new(h.parse().unwrap()).unwrap();
        let l_p = Price::new(l.parse().unwrap()).unwrap();
        let c_p = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: l_p, high: h_p, low: l_p, close: c_p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_keltner_period_0_error() {
        assert!(KeltnerChannel::new("kc", 0, dec!(2)).is_err());
    }

    #[test]
    fn test_keltner_unavailable_before_period() {
        let mut kc = KeltnerChannel::new("kc3", 3, dec!(2)).unwrap();
        assert_eq!(kc.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(kc.update_bar(&bar("115", "95", "105")).unwrap(), SignalValue::Unavailable);
        assert!(kc.update_bar(&bar("120", "100", "110")).unwrap().is_scalar());
    }

    #[test]
    fn test_keltner_constant_price_equals_price() {
        let mut kc = KeltnerChannel::new("kc3", 3, dec!(2)).unwrap();
        for _ in 0..10 {
            kc.update_bar(&bar("100", "100", "100")).unwrap();
        }
        match kc.update_bar(&bar("100", "100", "100")).unwrap() {
            SignalValue::Scalar(d) => assert_eq!(d, dec!(100)),
            _ => panic!("expected Scalar"),
        }
    }

    #[test]
    fn test_keltner_reset() {
        let mut kc = KeltnerChannel::new("kc3", 3, dec!(2)).unwrap();
        for _ in 0..5 { kc.update_bar(&bar("110", "90", "100")).unwrap(); }
        assert!(kc.is_ready());
        kc.reset();
        assert!(!kc.is_ready());
    }
}
