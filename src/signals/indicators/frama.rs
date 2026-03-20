//! Fractal Adaptive Moving Average (FRAMA) — John Ehlers.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Fractal Adaptive Moving Average (FRAMA) — an EMA whose smoothing coefficient
/// adapts based on the fractal dimension of recent price action.
///
/// The fractal dimension is computed over a `period`-bar window split into two halves:
///
/// ```text
/// N1 = (highest_high(first half) − lowest_low(first half)) / (period / 2)
/// N2 = (highest_high(second half) − lowest_low(second half)) / (period / 2)
/// N3 = (highest_high(full window) − lowest_low(full window)) / period
/// D  = (ln(N1 + N2) − ln(N3)) / ln(2)           — fractal dimension in [1, 2]
/// alpha = exp(−4.6 × (D − 1))                   — clipped to [0.01, 1.0]
/// FRAMA  = alpha × close + (1 − alpha) × FRAMA_prev
/// ```
///
/// When `N1 + N2 == 0` or `N3 == 0`, `alpha` defaults to `1.0` (fast EMA).
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// `period` must be even and ≥ 4.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Frama;
/// use fin_primitives::signals::Signal;
///
/// let f = Frama::new("frama20", 20).unwrap();
/// assert_eq!(f.period(), 20);
/// ```
pub struct Frama {
    name: String,
    period: usize,
    half: usize,
    highs: VecDeque<f64>,
    lows: VecDeque<f64>,
    frama_prev: Option<f64>,
}

impl Frama {
    /// Constructs a new `Frama`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 4` or `period` is odd.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 4 || period % 2 != 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            half: period / 2,
            highs: VecDeque::with_capacity(period),
            lows: VecDeque::with_capacity(period),
            frama_prev: None,
        })
    }
}

impl Signal for Frama {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.frama_prev.is_some() }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        use rust_decimal::prelude::ToPrimitive;
        let h = bar.high.to_f64().unwrap_or(0.0);
        let l = bar.low.to_f64().unwrap_or(0.0);
        let c = bar.close.to_f64().unwrap_or(0.0);

        self.highs.push_back(h);
        self.lows.push_back(l);
        if self.highs.len() > self.period {
            self.highs.pop_front();
            self.lows.pop_front();
        }

        if self.highs.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        // First half
        let h1 = self.highs.iter().take(self.half).cloned().fold(f64::MIN, f64::max);
        let l1 = self.lows.iter().take(self.half).cloned().fold(f64::MAX, f64::min);
        // Second half
        let h2 = self.highs.iter().skip(self.half).cloned().fold(f64::MIN, f64::max);
        let l2 = self.lows.iter().skip(self.half).cloned().fold(f64::MAX, f64::min);
        // Full window
        let h3 = self.highs.iter().cloned().fold(f64::MIN, f64::max);
        let l3 = self.lows.iter().cloned().fold(f64::MAX, f64::min);

        let n1 = (h1 - l1) / self.half as f64;
        let n2 = (h2 - l2) / self.half as f64;
        let n3 = (h3 - l3) / self.period as f64;

        let alpha = if n1 + n2 > 0.0 && n3 > 0.0 {
            let d = ((n1 + n2).ln() - n3.ln()) / 2_f64.ln();
            (-4.6 * (d - 1.0)).exp().clamp(0.01, 1.0)
        } else {
            1.0
        };

        let prev = self.frama_prev.unwrap_or(c);
        let frama = alpha * c + (1.0 - alpha) * prev;
        self.frama_prev = Some(frama);

        Decimal::try_from(frama)
            .map(SignalValue::Scalar)
            .map_err(|_| FinError::ArithmeticOverflow)
    }

    fn reset(&mut self) {
        self.highs.clear();
        self.lows.clear();
        self.frama_prev = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};

    fn bar(h: &str, l: &str, c: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_frama_invalid_period_odd() {
        assert!(Frama::new("f", 5).is_err());
    }

    #[test]
    fn test_frama_invalid_period_too_small() {
        assert!(Frama::new("f", 2).is_err());
    }

    #[test]
    fn test_frama_unavailable_before_period() {
        let mut f = Frama::new("f", 4).unwrap();
        for _ in 0..3 {
            assert_eq!(f.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!f.is_ready());
    }

    #[test]
    fn test_frama_produces_scalar_after_period() {
        let mut f = Frama::new("f", 4).unwrap();
        for _ in 0..4 {
            f.update_bar(&bar("110", "90", "100")).unwrap();
        }
        assert!(f.is_ready());
        assert!(matches!(f.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Scalar(_)));
    }

    #[test]
    fn test_frama_flat_prices_tracks_close() {
        let mut f = Frama::new("f", 4).unwrap();
        // All identical bars → fractal dimension undefined → alpha=1.0 → FRAMA = close
        for _ in 0..10 {
            if let SignalValue::Scalar(v) = f.update_bar(&bar("100", "100", "100")).unwrap() {
                let diff = (v - rust_decimal_macros::dec!(100)).abs();
                assert!(diff < rust_decimal_macros::dec!(0.0001), "expected close to 100, got {v}");
            }
        }
    }

    #[test]
    fn test_frama_reset() {
        let mut f = Frama::new("f", 4).unwrap();
        for _ in 0..5 { f.update_bar(&bar("110", "90", "100")).unwrap(); }
        assert!(f.is_ready());
        f.reset();
        assert!(!f.is_ready());
        assert_eq!(f.update_bar(&bar("100", "100", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_frama_period() {
        let f = Frama::new("f", 10).unwrap();
        assert_eq!(f.period(), 10);
    }
}
