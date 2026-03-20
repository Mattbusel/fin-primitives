//! Detrended Synthetic Price (DSP) indicator.
//!
//! Computes a band-pass filtered price using a simple Ehlers-style approach:
//! the Hilbert Transform approximation to isolate a dominant cycle component.
//! This simplified version uses a fixed 7-bar smoothing.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Detrended Synthetic Price — isolates the dominant cycle by removing the trend.
///
/// Computes a 7-bar EMA of price, then subtracts a second 7-bar EMA of the first EMA.
/// The result oscillates around zero and represents the cycle component of price.
///
/// ```text
/// ema1[i]  = EMA(period, close)[i]
/// ema2[i]  = EMA(period, ema1)[i]
/// DSP[i]   = ema1[i] - ema2[i]
/// ```
///
/// Returns [`SignalValue::Unavailable`] until both EMAs have initialised.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Dsp;
/// use fin_primitives::signals::Signal;
///
/// let dsp = Dsp::new("dsp7", 7).unwrap();
/// assert_eq!(dsp.period(), 7);
/// ```
pub struct Dsp {
    name: String,
    period: usize,
    k: Decimal,
    ema1_seed: VecDeque<Decimal>,
    ema1: Option<Decimal>,
    ema2_seed: VecDeque<Decimal>,
    ema2: Option<Decimal>,
}

impl Dsp {
    /// Creates a new `Dsp` with the given smoothing period.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        #[allow(clippy::cast_possible_truncation)]
        let k = Decimal::TWO / Decimal::from((period + 1) as u32);
        Ok(Self {
            name: name.into(),
            period,
            k,
            ema1_seed: VecDeque::with_capacity(period),
            ema1: None,
            ema2_seed: VecDeque::with_capacity(period),
            ema2: None,
        })
    }
}

impl Signal for Dsp {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let close = bar.close;

        // --- EMA1 ---
        let ema1_val = match self.ema1 {
            None => {
                self.ema1_seed.push_back(close);
                if self.ema1_seed.len() < self.period {
                    return Ok(SignalValue::Unavailable);
                }
                let seed: Decimal = self.ema1_seed.iter().sum::<Decimal>()
                    / Decimal::from(self.period as u32);
                self.ema1 = Some(seed);
                seed
            }
            Some(prev) => {
                let v = close * self.k + prev * (Decimal::ONE - self.k);
                self.ema1 = Some(v);
                v
            }
        };

        // --- EMA2 (of EMA1) ---
        let ema2_val = match self.ema2 {
            None => {
                self.ema2_seed.push_back(ema1_val);
                if self.ema2_seed.len() < self.period {
                    return Ok(SignalValue::Unavailable);
                }
                let seed: Decimal = self.ema2_seed.iter().sum::<Decimal>()
                    / Decimal::from(self.period as u32);
                self.ema2 = Some(seed);
                seed
            }
            Some(prev) => {
                let v = ema1_val * self.k + prev * (Decimal::ONE - self.k);
                self.ema2 = Some(v);
                v
            }
        };

        Ok(SignalValue::Scalar(ema1_val - ema2_val))
    }

    fn is_ready(&self) -> bool {
        self.ema2.is_some()
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.ema1_seed.clear();
        self.ema1 = None;
        self.ema2_seed.clear();
        self.ema2 = None;
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
    fn test_dsp_invalid_period() {
        assert!(Dsp::new("d", 0).is_err());
    }

    #[test]
    fn test_dsp_unavailable_before_warmup() {
        // period=3 needs 3 bars to seed ema1 + 2 more for ema2 → first 4 are Unavailable
        let mut dsp = Dsp::new("d", 3).unwrap();
        for _ in 0..4 {
            assert_eq!(dsp.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!dsp.is_ready());
    }

    #[test]
    fn test_dsp_scalar_after_warmup() {
        let mut dsp = Dsp::new("d", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..10 {
            last = dsp.update_bar(&bar("100")).unwrap();
        }
        assert!(matches!(last, SignalValue::Scalar(_)));
        assert!(dsp.is_ready());
    }

    #[test]
    fn test_dsp_flat_price_converges_to_zero() {
        let mut dsp = Dsp::new("d", 3).unwrap();
        for _ in 0..30 {
            dsp.update_bar(&bar("100")).unwrap();
        }
        if let SignalValue::Scalar(v) = dsp.update_bar(&bar("100")).unwrap() {
            assert!(v.abs() < dec!(0.001), "expected near-zero, got {v}");
        }
    }

    #[test]
    fn test_dsp_reset() {
        let mut dsp = Dsp::new("d", 3).unwrap();
        for _ in 0..15 { dsp.update_bar(&bar("100")).unwrap(); }
        assert!(dsp.is_ready());
        dsp.reset();
        assert!(!dsp.is_ready());
        assert_eq!(dsp.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }
}
