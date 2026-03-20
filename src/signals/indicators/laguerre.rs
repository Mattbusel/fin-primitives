//! Laguerre Filter RSI indicator (John Ehlers).

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Laguerre Filter RSI — a 4-bar Laguerre transform applied as an RSI-style oscillator.
///
/// The Laguerre filter uses a damping factor `gamma` (0 < gamma < 1) to reduce lag.
/// Higher `gamma` = smoother but more lag; lower `gamma` = noisier but faster.
///
/// ```text
/// L0 = (1 - gamma) * close + gamma * L0[prev]
/// L1 = -gamma * L0 + L0[prev] + gamma * L1[prev]
/// L2 = -gamma * L1 + L1[prev] + gamma * L2[prev]
/// L3 = -gamma * L2 + L2[prev] + gamma * L3[prev]
///
/// cu = (L0 > L1 ? L0 - L1 : 0) + (L1 > L2 ? L1 - L2 : 0) + (L2 > L3 ? L2 - L3 : 0)
/// cd = (L0 < L1 ? L1 - L0 : 0) + (L1 < L2 ? L2 - L1 : 0) + (L2 < L3 ? L3 - L2 : 0)
/// RSI = cu / (cu + cd) × 100    (or 50 when cu + cd == 0)
/// ```
///
/// Returns [`SignalValue::Unavailable`] on the first bar (no prior state); produces values immediately after.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::LaguerreRsi;
/// use fin_primitives::signals::Signal;
/// let l = LaguerreRsi::new("lrsi", 0.5).unwrap();
/// assert_eq!(l.period(), 4);
/// ```
pub struct LaguerreRsi {
    name: String,
    gamma: f64,
    l0_prev: Option<f64>,
    l1_prev: f64,
    l2_prev: f64,
    l3_prev: f64,
    ready: bool,
}

impl LaguerreRsi {
    /// Constructs a new `LaguerreRsi`.
    ///
    /// - `gamma`: damping factor in `(0, 1)`. Typical values: 0.5–0.8.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] (with value `0`) if `gamma` is not in `(0, 1)`.
    pub fn new(name: impl Into<String>, gamma: f64) -> Result<Self, FinError> {
        if gamma <= 0.0 || gamma >= 1.0 {
            return Err(FinError::InvalidPeriod(0));
        }
        Ok(Self {
            name: name.into(),
            gamma,
            l0_prev: None,
            l1_prev: 0.0,
            l2_prev: 0.0,
            l3_prev: 0.0,
            ready: false,
        })
    }
}

impl Signal for LaguerreRsi {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        use rust_decimal::prelude::ToPrimitive;
        let close = bar.close.to_f64().unwrap_or(0.0);

        let l0_p = match self.l0_prev {
            None => {
                // Seed all L values to the first close to prevent large initial transients
                self.l0_prev = Some(close);
                self.l1_prev = close;
                self.l2_prev = close;
                self.l3_prev = close;
                return Ok(SignalValue::Unavailable);
            }
            Some(v) => v,
        };

        let g = self.gamma;
        let l0 = (1.0 - g) * close + g * l0_p;
        let l1 = -g * l0 + l0_p + g * self.l1_prev;
        let l2 = -g * l1 + self.l1_prev + g * self.l2_prev;
        let l3 = -g * l2 + self.l2_prev + g * self.l3_prev;

        let cu = (if l0 > l1 { l0 - l1 } else { 0.0 })
            + (if l1 > l2 { l1 - l2 } else { 0.0 })
            + (if l2 > l3 { l2 - l3 } else { 0.0 });
        let cd = (if l0 < l1 { l1 - l0 } else { 0.0 })
            + (if l1 < l2 { l2 - l1 } else { 0.0 })
            + (if l2 < l3 { l3 - l2 } else { 0.0 });

        let rsi = if cu + cd == 0.0 {
            50.0
        } else {
            cu / (cu + cd) * 100.0
        };

        self.l0_prev = Some(l0);
        self.l1_prev = l1;
        self.l2_prev = l2;
        self.l3_prev = l3;
        self.ready = true;

        Ok(SignalValue::Scalar(
            Decimal::try_from(rsi).unwrap_or(Decimal::ZERO),
        ))
    }

    fn is_ready(&self) -> bool { self.ready }
    fn period(&self) -> usize { 4 }
    fn reset(&mut self) {
        self.l0_prev = None;
        self.l1_prev = 0.0;
        self.l2_prev = 0.0;
        self.l3_prev = 0.0;
        self.ready = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
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
    fn test_laguerre_invalid_gamma() {
        assert!(LaguerreRsi::new("l", 0.0).is_err());
        assert!(LaguerreRsi::new("l", 1.0).is_err());
        assert!(LaguerreRsi::new("l", -0.1).is_err());
    }

    #[test]
    fn test_laguerre_first_bar_unavailable() {
        let mut l = LaguerreRsi::new("l", 0.5).unwrap();
        assert_eq!(l.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert!(!l.is_ready());
    }

    #[test]
    fn test_laguerre_second_bar_produces_scalar() {
        let mut l = LaguerreRsi::new("l", 0.5).unwrap();
        l.update_bar(&bar("100")).unwrap();
        let v = l.update_bar(&bar("105")).unwrap();
        assert!(matches!(v, SignalValue::Scalar(_)));
        assert!(l.is_ready());
    }

    #[test]
    fn test_laguerre_flat_is_50() {
        let mut l = LaguerreRsi::new("l", 0.5).unwrap();
        // All same price → cu=cd=0 → RSI=50
        for _ in 0..20 { l.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = l.update_bar(&bar("100")).unwrap() {
            assert_eq!(v, dec!(50));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_laguerre_uptrend_above_50() {
        let mut l = LaguerreRsi::new("l", 0.5).unwrap();
        let prices: Vec<String> = (100..120).map(|i| i.to_string()).collect();
        let mut last = SignalValue::Unavailable;
        for p in &prices { last = l.update_bar(&bar(p)).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(50), "uptrend should give RSI > 50: {v}");
        }
    }

    #[test]
    fn test_laguerre_period() {
        let l = LaguerreRsi::new("l", 0.6).unwrap();
        assert_eq!(l.period(), 4);
    }

    #[test]
    fn test_laguerre_reset() {
        let mut l = LaguerreRsi::new("l", 0.5).unwrap();
        for p in &["100", "101", "102"] { l.update_bar(&bar(p)).unwrap(); }
        assert!(l.is_ready());
        l.reset();
        assert!(!l.is_ready());
        assert_eq!(l.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }
}
