//! Percentage Volume Oscillator (PVO).
//!
//! Measures momentum in volume. Analogous to PPO but applied to volume instead of price.
//! `PVO = (EMA(fast, volume) - EMA(slow, volume)) / EMA(slow, volume) * 100`

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Percentage Volume Oscillator.
///
/// `PVO = (EMA(fast_period, volume) - EMA(slow_period, volume)) / EMA(slow_period, volume) * 100`.
///
/// Returns [`crate::signals::SignalValue::Unavailable`] until both EMAs are seeded.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Pvo;
/// use fin_primitives::signals::Signal;
/// let p = Pvo::new("pvo", 12, 26).unwrap();
/// assert_eq!(p.period(), 26);
/// assert!(!p.is_ready());
/// ```
pub struct Pvo {
    name: String,
    fast_period: usize,
    slow_period: usize,
    fast_ema: Option<Decimal>,
    slow_ema: Option<Decimal>,
    fast_k: Decimal,
    slow_k: Decimal,
    bar_count: usize,
}

impl Pvo {
    /// Constructs a new `Pvo`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if either period is zero or `fast_period >= slow_period`.
    pub fn new(name: impl Into<String>, fast_period: usize, slow_period: usize) -> Result<Self, FinError> {
        if fast_period == 0 || slow_period == 0 {
            return Err(FinError::InvalidPeriod(0));
        }
        if fast_period >= slow_period {
            return Err(FinError::InvalidInput(
                format!("fast_period ({}) must be < slow_period ({})", fast_period, slow_period)
            ));
        }
        let fast_k = Decimal::TWO / Decimal::from(fast_period as u32 + 1);
        let slow_k = Decimal::TWO / Decimal::from(slow_period as u32 + 1);
        Ok(Self {
            name: name.into(),
            fast_period,
            slow_period,
            fast_ema: None,
            slow_ema: None,
            fast_k,
            slow_k,
            bar_count: 0,
        })
    }
}

impl Pvo {
    /// Returns the fast EMA period.
    pub fn fast_period(&self) -> usize { self.fast_period }
}

impl Signal for Pvo {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let vol = bar.volume;
        self.bar_count += 1;

        let fast = match self.fast_ema {
            None => { self.fast_ema = Some(vol); vol }
            Some(prev) => { let v = prev + self.fast_k * (vol - prev); self.fast_ema = Some(v); v }
        };
        let slow = match self.slow_ema {
            None => { self.slow_ema = Some(vol); vol }
            Some(prev) => { let v = prev + self.slow_k * (vol - prev); self.slow_ema = Some(v); v }
        };

        if self.bar_count < self.slow_period {
            return Ok(SignalValue::Unavailable);
        }
        if slow.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let pvo = (fast - slow) / slow * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(pvo))
    }

    fn is_ready(&self) -> bool {
        self.bar_count >= self.slow_period
    }

    fn period(&self) -> usize {
        self.slow_period
    }

    fn reset(&mut self) {
        self.fast_ema = None;
        self.slow_ema = None;
        self.bar_count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals::Signal;
    use rust_decimal_macros::dec;

    fn bar_vol(vol: &str) -> BarInput {
        BarInput::new(
            dec!(100),
            dec!(105),
            dec!(95),
            dec!(100),
            vol.parse().unwrap(),
        )
    }

    #[test]
    fn test_pvo_invalid_period() {
        assert!(Pvo::new("p", 0, 26).is_err());
        assert!(Pvo::new("p", 26, 12).is_err()); // fast >= slow
    }

    #[test]
    fn test_pvo_unavailable_before_warmup() {
        let mut p = Pvo::new("p", 3, 6).unwrap();
        for _ in 0..5 {
            assert_eq!(p.update(&bar_vol("1000")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_pvo_ready_after_slow_period() {
        let mut p = Pvo::new("p", 3, 5).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 {
            last = p.update(&bar_vol("1000")).unwrap();
        }
        assert!(p.is_ready());
        assert!(matches!(last, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_pvo_zero_when_flat_volume() {
        let mut p = Pvo::new("p", 3, 5).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..20 {
            last = p.update(&bar_vol("1000")).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v.abs() < dec!(1), "pvo should be near 0 for flat volume: {}", v);
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_pvo_reset_clears_state() {
        let mut p = Pvo::new("p", 3, 5).unwrap();
        for _ in 0..5 {
            p.update(&bar_vol("1000")).unwrap();
        }
        assert!(p.is_ready());
        p.reset();
        assert!(!p.is_ready());
    }

    #[test]
    fn test_pvo_period_and_name() {
        let p = Pvo::new("my_pvo", 12, 26).unwrap();
        assert_eq!(p.period(), 26);
        assert_eq!(p.name(), "my_pvo");
    }
}
