//! Polarized Fractal Efficiency (PFE) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Polarized Fractal Efficiency — measures trend strength and direction.
///
/// `PFE = sign(close - close[n]) × (distance / path_length) × 100`
///
/// where:
/// - `distance = sqrt((n² + Δclose²))` — straight-line efficiency
/// - `path_length = Σ sqrt(1 + (close[i] - close[i-1])²)` — actual path
///
/// Values near ±100 indicate strong trend; near 0 indicates choppy market.
/// Then a 5-bar EMA smoothing is applied to reduce noise.
///
/// Returns [`SignalValue::Unavailable`] until `period + smoothing` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Pfe;
/// use fin_primitives::signals::Signal;
///
/// let p = Pfe::new("pfe10", 10, 5).unwrap();
/// assert_eq!(p.period(), 10);
/// assert!(!p.is_ready());
/// ```
pub struct Pfe {
    name: String,
    period: usize,
    smoothing: usize,
    smooth_k: Decimal,
    closes: VecDeque<Decimal>,
    smooth_ema: Option<Decimal>,
}

impl Pfe {
    /// Constructs a new `Pfe`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0` or `smoothing == 0`.
    pub fn new(name: impl Into<String>, period: usize, smoothing: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        if smoothing == 0 { return Err(FinError::InvalidPeriod(smoothing)); }
        #[allow(clippy::cast_possible_truncation)]
        let smooth_k = Decimal::TWO / Decimal::from((smoothing + 1) as u32);
        Ok(Self {
            name: name.into(),
            period,
            smoothing,
            smooth_k,
            closes: VecDeque::with_capacity(period + 1),
            smooth_ema: None,
        })
    }
}

impl Pfe {
    /// Returns the EMA smoothing period.
    pub fn smoothing(&self) -> usize { self.smoothing }
}

impl Signal for Pfe {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        use rust_decimal::prelude::ToPrimitive;
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }

        let closes_vec: Vec<f64> = self.closes.iter()
            .filter_map(|c| c.to_f64())
            .collect();
        let n = closes_vec.len() as f64 - 1.0;
        let delta = closes_vec.last().unwrap() - closes_vec.first().unwrap();
        let direction = if delta > 0.0 { 1.0f64 } else if delta < 0.0 { -1.0f64 } else { 0.0 };
        let distance = (n * n + delta * delta).sqrt();
        let path: f64 = closes_vec.windows(2)
            .map(|w| { let d = w[1] - w[0]; (1.0 + d * d).sqrt() })
            .sum();
        let raw_pfe = if path == 0.0 {
            0.0
        } else {
            direction * distance / path * 100.0
        };
        let raw_dec = Decimal::try_from(raw_pfe).unwrap_or(Decimal::ZERO);

        let new_ema = match self.smooth_ema {
            None => raw_dec,
            Some(prev) => prev + self.smooth_k * (raw_dec - prev),
        };
        self.smooth_ema = Some(new_ema);
        Ok(SignalValue::Scalar(new_ema))
    }

    fn is_ready(&self) -> bool { self.smooth_ema.is_some() }

    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.closes.clear();
        self.smooth_ema = None;
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
    fn test_pfe_invalid_period() {
        assert!(Pfe::new("p", 0, 5).is_err());
        assert!(Pfe::new("p", 10, 0).is_err());
    }

    #[test]
    fn test_pfe_unavailable_before_period() {
        let mut p = Pfe::new("p", 3, 2).unwrap();
        for _ in 0..3 {
            assert_eq!(p.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_pfe_ready_after_warmup() {
        let mut p = Pfe::new("p", 3, 2).unwrap();
        for _ in 0..4 {
            p.update_bar(&bar("100")).unwrap();
        }
        assert!(p.is_ready());
    }

    #[test]
    fn test_pfe_flat_market_near_zero() {
        let mut p = Pfe::new("p", 5, 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..20 {
            last = p.update_bar(&bar("100")).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v.abs() < dec!(1), "PFE should be near 0 for flat market: {v}");
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_pfe_reset() {
        let mut p = Pfe::new("p", 3, 2).unwrap();
        for _ in 0..10 { p.update_bar(&bar("100")).unwrap(); }
        assert!(p.is_ready());
        p.reset();
        assert!(!p.is_ready());
    }

    #[test]
    fn test_pfe_period_and_name() {
        let p = Pfe::new("my_pfe", 10, 5).unwrap();
        assert_eq!(p.period(), 10);
        assert_eq!(p.name(), "my_pfe");
    }
}
