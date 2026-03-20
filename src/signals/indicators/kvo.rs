//! Klinger Volume Oscillator (KVO) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Klinger Volume Oscillator — `EMA(fast, VF) - EMA(slow, VF)`.
///
/// Volume Force (VF) = `volume × sign(typical_price_change) × dm × cm` where:
/// - `dm = high - low`
/// - `cm` is a running cumulative measure
///
/// The oscillator uses two EMAs (default 34 and 55 periods) of the VF.
/// Positive values suggest accumulation; negative suggest distribution.
///
/// Returns [`SignalValue::Unavailable`] until the slow EMA is seeded.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Kvo;
/// use fin_primitives::signals::Signal;
///
/// let mut kvo = Kvo::new("kvo", 34, 55).unwrap();
/// ```
pub struct Kvo {
    name: String,
    fast_period: usize,
    slow_period: usize,
    fast_k: Decimal,
    slow_k: Decimal,
    prev_typical: Option<Decimal>,
    prev_cm: Decimal,
    fast_ema: Option<Decimal>,
    slow_ema: Option<Decimal>,
    fast_count: usize,
    slow_count: usize,
    fast_seed: Decimal,
    slow_seed: Decimal,
}

impl Kvo {
    /// Constructs a new `Kvo`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if either period is 0 or `fast >= slow`.
    pub fn new(name: impl Into<String>, fast: usize, slow: usize) -> Result<Self, FinError> {
        if fast == 0 || slow == 0 {
            return Err(FinError::InvalidPeriod(0));
        }
        if fast >= slow {
            return Err(FinError::InvalidPeriod(fast));
        }
        #[allow(clippy::cast_possible_truncation)]
        let fast_k = Decimal::TWO / Decimal::from((fast + 1) as u32);
        #[allow(clippy::cast_possible_truncation)]
        let slow_k = Decimal::TWO / Decimal::from((slow + 1) as u32);
        Ok(Self {
            name: name.into(),
            fast_period: fast,
            slow_period: slow,
            fast_k,
            slow_k,
            prev_typical: None,
            prev_cm: Decimal::ZERO,
            fast_ema: None,
            slow_ema: None,
            fast_count: 0,
            slow_count: 0,
            fast_seed: Decimal::ZERO,
            slow_seed: Decimal::ZERO,
        })
    }

    fn ema_step(count: &mut usize, seed: &mut Decimal, state: &mut Option<Decimal>, period: usize, k: Decimal, value: Decimal) -> Option<Decimal> {
        *count += 1;
        if *count <= period {
            *seed += value;
            if *count == period {
                #[allow(clippy::cast_possible_truncation)]
                let s = *seed / Decimal::from(period as u32);
                *state = Some(s);
                return Some(s);
            }
            return None;
        }
        let prev = state.unwrap_or(value);
        let v = value * k + prev * (Decimal::ONE - k);
        *state = Some(v);
        Some(v)
    }
}

impl Signal for Kvo {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let typical = (bar.high + bar.low + bar.close) / Decimal::from(3u32);
        let dm = bar.range();
        let vol = bar.volume;

        let (trend, cm) = if let Some(pt) = self.prev_typical {
            let trend = if typical > pt { Decimal::ONE } else { Decimal::NEGATIVE_ONE };
            let cm = if trend == (if self.prev_cm >= Decimal::ZERO { Decimal::ONE } else { Decimal::NEGATIVE_ONE }) {
                self.prev_cm + dm
            } else {
                dm
            };
            (trend, cm)
        } else {
            self.prev_typical = Some(typical);
            self.prev_cm = dm;
            return Ok(SignalValue::Unavailable);
        };

        self.prev_typical = Some(typical);
        self.prev_cm = cm;

        let vf = if cm == Decimal::ZERO {
            Decimal::ZERO
        } else {
            vol * trend * (Decimal::TWO * dm / cm - Decimal::ONE) * Decimal::ONE_HUNDRED
        };

        let fast = Self::ema_step(&mut self.fast_count, &mut self.fast_seed, &mut self.fast_ema, self.fast_period, self.fast_k, vf);
        let slow = Self::ema_step(&mut self.slow_count, &mut self.slow_seed, &mut self.slow_ema, self.slow_period, self.slow_k, vf);

        match (fast, slow) {
            (Some(f), Some(s)) => Ok(SignalValue::Scalar(f - s)),
            _ => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool {
        self.fast_ema.is_some() && self.slow_ema.is_some()
    }

    fn period(&self) -> usize {
        self.slow_period
    }

    fn reset(&mut self) {
        self.prev_typical = None;
        self.prev_cm = Decimal::ZERO;
        self.fast_ema = None;
        self.slow_ema = None;
        self.fast_count = 0;
        self.slow_count = 0;
        self.fast_seed = Decimal::ZERO;
        self.slow_seed = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};

    fn bar(h: &str, l: &str, c: &str, v: &str) -> OhlcvBar {
        let hi = Price::new(h.parse().unwrap()).unwrap();
        let lo = Price::new(l.parse().unwrap()).unwrap();
        let cl = Price::new(c.parse().unwrap()).unwrap();
        let vol = Quantity::new(v.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lo, high: hi, low: lo, close: cl,
            volume: vol,
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_kvo_invalid_periods() {
        assert!(Kvo::new("k", 0, 55).is_err());
        assert!(Kvo::new("k", 55, 34).is_err());
        assert!(Kvo::new("k", 34, 34).is_err());
    }

    #[test]
    fn test_kvo_unavailable_before_slow_period() {
        let mut kvo = Kvo::new("kvo", 4, 8).unwrap();
        for _ in 0..8 {
            assert_eq!(kvo.update_bar(&bar("110","90","100","1000")).unwrap(), SignalValue::Unavailable);
        }
        assert!(kvo.update_bar(&bar("110","90","100","1000")).unwrap().is_scalar());
    }

    #[test]
    fn test_kvo_reset() {
        let mut kvo = Kvo::new("kvo", 4, 8).unwrap();
        for _ in 0..20 { kvo.update_bar(&bar("110","90","100","1000")).unwrap(); }
        assert!(kvo.is_ready());
        kvo.reset();
        assert!(!kvo.is_ready());
    }
}
