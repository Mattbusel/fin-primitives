//! Price Momentum Oscillator (PMO).

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Price Momentum Oscillator — a double-smoothed rate-of-change oscillator.
///
/// ```text
/// roc_scaled[i]  = (close[i] / close[i-1] - 1) × 10 × 100
/// EMA1[i]        = EMA(roc_smooth, roc_scaled)[i]
/// PMO[i]         = EMA(pmo_smooth, EMA1)[i]
/// signal[i]      = EMA(signal_smooth, PMO)[i]
/// ```
///
/// Default parameters: `roc_smooth = 35`, `pmo_smooth = 20`, `signal_smooth = 10`.
///
/// Returns [`SignalValue::Unavailable`] until both smoothing EMAs have initialised
/// (requires at least `roc_smooth` bars for the first EMA seed).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Pmo;
/// use fin_primitives::signals::Signal;
///
/// let pmo = Pmo::new("pmo", 35, 20, 10).unwrap();
/// assert_eq!(pmo.period(), 35);
/// ```
pub struct Pmo {
    name: String,
    roc_smooth: usize,
    pmo_smooth: usize,
    signal_smooth: usize,
    prev_close: Option<Decimal>,
    // EMA1: smoothed roc_scaled
    ema1_k: Decimal,
    ema1: Option<Decimal>,
    ema1_seed_sum: Decimal,
    ema1_seed_count: usize,
    // PMO: smoothed EMA1
    pmo_k: Decimal,
    pmo: Option<Decimal>,
    pmo_seed_sum: Decimal,
    pmo_seed_count: usize,
    // Signal: smoothed PMO
    sig_k: Decimal,
    signal: Option<Decimal>,
    sig_seed_sum: Decimal,
    sig_seed_count: usize,
}

impl Pmo {
    /// Creates a new `Pmo` with the given smoothing periods.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if any period is zero.
    pub fn new(
        name: impl Into<String>,
        roc_smooth: usize,
        pmo_smooth: usize,
        signal_smooth: usize,
    ) -> Result<Self, FinError> {
        if roc_smooth == 0 { return Err(FinError::InvalidPeriod(roc_smooth)); }
        if pmo_smooth == 0 { return Err(FinError::InvalidPeriod(pmo_smooth)); }
        if signal_smooth == 0 { return Err(FinError::InvalidPeriod(signal_smooth)); }
        #[allow(clippy::cast_possible_truncation)]
        let ema1_k = Decimal::TWO / Decimal::from((roc_smooth + 1) as u32);
        #[allow(clippy::cast_possible_truncation)]
        let pmo_k = Decimal::TWO / Decimal::from((pmo_smooth + 1) as u32);
        #[allow(clippy::cast_possible_truncation)]
        let sig_k = Decimal::TWO / Decimal::from((signal_smooth + 1) as u32);
        Ok(Self {
            name: name.into(),
            roc_smooth,
            pmo_smooth,
            signal_smooth,
            prev_close: None,
            ema1_k,
            ema1: None,
            ema1_seed_sum: Decimal::ZERO,
            ema1_seed_count: 0,
            pmo_k,
            pmo: None,
            pmo_seed_sum: Decimal::ZERO,
            pmo_seed_count: 0,
            sig_k,
            signal: None,
            sig_seed_sum: Decimal::ZERO,
            sig_seed_count: 0,
        })
    }

    /// Creates a `Pmo` with standard default parameters (35, 20, 10).
    ///
    /// # Errors
    /// Never fails with default periods, but returns `Result` for API consistency.
    pub fn default_params(name: impl Into<String>) -> Result<Self, FinError> {
        Self::new(name, 35, 20, 10)
    }

    /// Returns the current signal line value, if available.
    pub fn signal_line(&self) -> Option<Decimal> {
        self.signal
    }
}

impl Signal for Pmo {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let close = bar.close;

        let Some(prev) = self.prev_close else {
            self.prev_close = Some(close);
            return Ok(SignalValue::Unavailable);
        };
        self.prev_close = Some(close);

        // roc_scaled = (close/prev - 1) * 10 * 100, guard prev==0
        let roc_scaled = if prev.is_zero() {
            Decimal::ZERO
        } else {
            (close / prev - Decimal::ONE) * Decimal::from(1000u32)
        };

        // --- EMA1 ---
        let ema1_val = match self.ema1 {
            None => {
                self.ema1_seed_sum += roc_scaled;
                self.ema1_seed_count += 1;
                if self.ema1_seed_count < self.roc_smooth {
                    return Ok(SignalValue::Unavailable);
                }
                let seed = self.ema1_seed_sum / Decimal::from(self.roc_smooth as u32);
                self.ema1 = Some(seed);
                seed
            }
            Some(prev_ema1) => {
                let v = roc_scaled * self.ema1_k + prev_ema1 * (Decimal::ONE - self.ema1_k);
                self.ema1 = Some(v);
                v
            }
        };

        // --- PMO ---
        let pmo_val = match self.pmo {
            None => {
                self.pmo_seed_sum += ema1_val;
                self.pmo_seed_count += 1;
                if self.pmo_seed_count < self.pmo_smooth {
                    return Ok(SignalValue::Unavailable);
                }
                let seed = self.pmo_seed_sum / Decimal::from(self.pmo_smooth as u32);
                self.pmo = Some(seed);
                seed
            }
            Some(prev_pmo) => {
                let v = ema1_val * self.pmo_k + prev_pmo * (Decimal::ONE - self.pmo_k);
                self.pmo = Some(v);
                v
            }
        };

        // --- Signal ---
        let sig_val = match self.signal {
            None => {
                self.sig_seed_sum += pmo_val;
                self.sig_seed_count += 1;
                if self.sig_seed_count < self.signal_smooth {
                    return Ok(SignalValue::Unavailable);
                }
                let seed = self.sig_seed_sum / Decimal::from(self.signal_smooth as u32);
                self.signal = Some(seed);
                seed
            }
            Some(prev_sig) => {
                let v = pmo_val * self.sig_k + prev_sig * (Decimal::ONE - self.sig_k);
                self.signal = Some(v);
                v
            }
        };

        let _ = sig_val; // signal is accessible via signal_line(); PMO is the primary output
        Ok(SignalValue::Scalar(pmo_val))
    }

    fn is_ready(&self) -> bool {
        self.pmo.is_some()
    }

    fn period(&self) -> usize {
        self.roc_smooth
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.ema1 = None;
        self.ema1_seed_sum = Decimal::ZERO;
        self.ema1_seed_count = 0;
        self.pmo = None;
        self.pmo_seed_sum = Decimal::ZERO;
        self.pmo_seed_count = 0;
        self.signal = None;
        self.sig_seed_sum = Decimal::ZERO;
        self.sig_seed_count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};

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
    fn test_pmo_invalid_period() {
        assert!(Pmo::new("p", 0, 20, 10).is_err());
        assert!(Pmo::new("p", 35, 0, 10).is_err());
        assert!(Pmo::new("p", 35, 20, 0).is_err());
    }

    #[test]
    fn test_pmo_unavailable_before_ready() {
        let mut pmo = Pmo::new("p", 3, 2, 2).unwrap();
        // needs 3+2+2 - some bars before producing scalar
        for _ in 0..5 {
            let v = pmo.update_bar(&bar("100")).unwrap();
            assert_eq!(v, SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_pmo_produces_scalar_after_warmup() {
        let mut pmo = Pmo::new("p", 3, 2, 2).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..20 {
            last = pmo.update_bar(&bar("100")).unwrap();
        }
        assert!(matches!(last, SignalValue::Scalar(_)));
        assert!(pmo.is_ready());
    }

    #[test]
    fn test_pmo_flat_price_near_zero() {
        let mut pmo = Pmo::new("p", 3, 2, 2).unwrap();
        for _ in 0..30 {
            pmo.update_bar(&bar("100")).unwrap();
        }
        if let SignalValue::Scalar(v) = pmo.update_bar(&bar("100")).unwrap() {
            // constant price → roc=0 → PMO should converge toward 0
            assert!(v.abs() < rust_decimal_macros::dec!(0.01));
        }
    }

    #[test]
    fn test_pmo_reset() {
        let mut pmo = Pmo::new("p", 3, 2, 2).unwrap();
        for _ in 0..20 { pmo.update_bar(&bar("100")).unwrap(); }
        assert!(pmo.is_ready());
        pmo.reset();
        assert!(!pmo.is_ready());
        assert_eq!(pmo.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_pmo_default_params_period() {
        let pmo = Pmo::default_params("pmo").unwrap();
        assert_eq!(pmo.period(), 35);
    }
}
