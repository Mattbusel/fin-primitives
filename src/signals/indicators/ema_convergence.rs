//! EMA Convergence indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// EMA Convergence — measures how close the fast EMA is to the slow EMA,
/// expressed as a percentage of the slow EMA.
///
/// ```text
/// output = |fast_EMA − slow_EMA| / slow_EMA × 100
/// ```
///
/// Low values (near 0) indicate EMAs are converging/aligned.
/// High values indicate EMAs are diverging.
///
/// Returns [`SignalValue::Unavailable`] until both EMAs are warm.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::EmaConvergence;
/// use fin_primitives::signals::Signal;
///
/// let ec = EmaConvergence::new("ec", 5, 20).unwrap();
/// assert_eq!(ec.period(), 20);
/// ```
pub struct EmaConvergence {
    name: String,
    fast: usize,
    slow: usize,
    // fast EMA
    fast_ema: Option<Decimal>,
    fast_seed: Vec<Decimal>,
    // slow EMA
    slow_ema: Option<Decimal>,
    slow_seed: Vec<Decimal>,
}

impl EmaConvergence {
    /// Creates a new `EmaConvergence`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `fast == 0`.
    /// Returns [`FinError::InvalidInput`] if `fast >= slow`.
    pub fn new(name: impl Into<String>, fast: usize, slow: usize) -> Result<Self, FinError> {
        if fast == 0 { return Err(FinError::InvalidPeriod(fast)); }
        if fast >= slow {
            return Err(FinError::InvalidInput("fast must be less than slow".into()));
        }
        Ok(Self {
            name: name.into(),
            fast,
            slow,
            fast_ema: None,
            fast_seed: Vec::with_capacity(fast),
            slow_ema: None,
            slow_seed: Vec::with_capacity(slow),
        })
    }
}

impl Signal for EmaConvergence {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let kf = Decimal::from(2u32) / Decimal::from((self.fast + 1) as u32);
        let ks = Decimal::from(2u32) / Decimal::from((self.slow + 1) as u32);

        // Update fast EMA
        if self.fast_ema.is_none() {
            self.fast_seed.push(bar.close);
            if self.fast_seed.len() == self.fast {
                let sma = self.fast_seed.iter().sum::<Decimal>()
                    / Decimal::from(self.fast as u32);
                self.fast_ema = Some(sma);
            }
        } else {
            let fe = self.fast_ema.unwrap() + kf * (bar.close - self.fast_ema.unwrap());
            self.fast_ema = Some(fe);
        }

        // Update slow EMA
        if self.slow_ema.is_none() {
            self.slow_seed.push(bar.close);
            if self.slow_seed.len() == self.slow {
                let sma = self.slow_seed.iter().sum::<Decimal>()
                    / Decimal::from(self.slow as u32);
                self.slow_ema = Some(sma);
            }
        } else {
            let se = self.slow_ema.unwrap() + ks * (bar.close - self.slow_ema.unwrap());
            self.slow_ema = Some(se);
        }

        match (self.fast_ema, self.slow_ema) {
            (Some(f), Some(s)) if !s.is_zero() => {
                let pct = (f - s).abs() / s * Decimal::from(100u32);
                Ok(SignalValue::Scalar(pct))
            }
            (Some(_), Some(_)) => Ok(SignalValue::Scalar(Decimal::ZERO)),
            _ => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool { self.fast_ema.is_some() && self.slow_ema.is_some() }
    fn period(&self) -> usize { self.slow }

    fn reset(&mut self) {
        self.fast_ema = None;
        self.fast_seed.clear();
        self.slow_ema = None;
        self.slow_seed.clear();
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
    fn test_ec_invalid() {
        assert!(EmaConvergence::new("e", 0, 20).is_err());
        assert!(EmaConvergence::new("e", 20, 10).is_err());
        assert!(EmaConvergence::new("e", 10, 10).is_err());
    }

    #[test]
    fn test_ec_unavailable_before_warmup() {
        let mut e = EmaConvergence::new("e", 3, 5).unwrap();
        for _ in 0..4 {
            assert_eq!(e.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_ec_flat_converges_to_zero() {
        // Flat prices: fast_EMA = slow_EMA = price → convergence = 0
        let mut e = EmaConvergence::new("e", 3, 5).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..20 { last = e.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            let diff = v.abs();
            assert!(diff < dec!(0.001), "expected ~0, got {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_ec_diverging_positive() {
        // After step-up, EMAs diverge: fast closer to new price than slow
        let mut e = EmaConvergence::new("e", 3, 5).unwrap();
        for _ in 0..20 { e.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = e.update_bar(&bar("200")).unwrap() {
            assert!(v > dec!(0), "expected positive divergence, got {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_ec_non_negative() {
        let mut e = EmaConvergence::new("e", 3, 5).unwrap();
        for i in 0u32..20 {
            let p = if i % 2 == 0 { "100" } else { "200" };
            if let SignalValue::Scalar(v) = e.update_bar(&bar(p)).unwrap() {
                assert!(v >= dec!(0), "expected non-negative, got {v}");
            }
        }
    }

    #[test]
    fn test_ec_reset() {
        let mut e = EmaConvergence::new("e", 3, 5).unwrap();
        for _ in 0..20 { e.update_bar(&bar("100")).unwrap(); }
        assert!(e.is_ready());
        e.reset();
        assert!(!e.is_ready());
    }
}
