//! Chaikin Oscillator indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Chaikin Oscillator — EMA(fast, A/D) – EMA(slow, A/D).
///
/// Accumulation/Distribution line:
/// ```text
/// CLV = ((close - low) - (high - close)) / (high - low)
/// AD[i] = AD[i-1] + volume × CLV
/// Chaikin = EMA(fast, AD) - EMA(slow, AD)
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `slow_period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ChaikinOsc;
/// use fin_primitives::signals::Signal;
///
/// let c = ChaikinOsc::new("chaikin", 3, 10).unwrap();
/// assert_eq!(c.period(), 10);
/// assert!(!c.is_ready());
/// ```
pub struct ChaikinOsc {
    name: String,
    fast: usize,
    slow: usize,
    fast_k: Decimal,
    slow_k: Decimal,
    ad: Decimal,
    fast_ema: Option<Decimal>,
    slow_ema: Option<Decimal>,
    bar_count: usize,
}

impl ChaikinOsc {
    /// Constructs a new `ChaikinOsc`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if either period is 0.
    /// Returns [`FinError::InvalidInput`] if `fast >= slow`.
    pub fn new(name: impl Into<String>, fast: usize, slow: usize) -> Result<Self, FinError> {
        if fast == 0 { return Err(FinError::InvalidPeriod(fast)); }
        if slow == 0 { return Err(FinError::InvalidPeriod(slow)); }
        if fast >= slow {
            return Err(FinError::InvalidInput(format!("fast ({fast}) must be < slow ({slow})")));
        }
        #[allow(clippy::cast_possible_truncation)]
        let fast_k = Decimal::TWO / Decimal::from((fast + 1) as u32);
        #[allow(clippy::cast_possible_truncation)]
        let slow_k = Decimal::TWO / Decimal::from((slow + 1) as u32);
        Ok(Self {
            name: name.into(),
            fast,
            slow,
            fast_k,
            slow_k,
            ad: Decimal::ZERO,
            fast_ema: None,
            slow_ema: None,
            bar_count: 0,
        })
    }
}

impl ChaikinOsc {
    /// Returns the fast EMA period.
    pub fn fast_period(&self) -> usize { self.fast }
    /// Returns the slow EMA period.
    pub fn slow_period(&self) -> usize { self.slow }
}

impl Signal for ChaikinOsc {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.bar_count += 1;
        let hl = bar.range();
        let clv = if hl.is_zero() {
            Decimal::ZERO
        } else {
            ((bar.close - bar.low) - (bar.high - bar.close)) / hl
        };
        self.ad += bar.volume * clv;

        let fast = match self.fast_ema {
            None => { self.fast_ema = Some(self.ad); self.ad }
            Some(prev) => { let v = prev + self.fast_k * (self.ad - prev); self.fast_ema = Some(v); v }
        };
        let slow = match self.slow_ema {
            None => { self.slow_ema = Some(self.ad); self.ad }
            Some(prev) => { let v = prev + self.slow_k * (self.ad - prev); self.slow_ema = Some(v); v }
        };

        if self.bar_count < self.slow {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(fast - slow))
    }

    fn is_ready(&self) -> bool { self.bar_count >= self.slow }

    fn period(&self) -> usize { self.slow }

    fn reset(&mut self) {
        self.ad = Decimal::ZERO;
        self.fast_ema = None;
        self.slow_ema = None;
        self.bar_count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals::Signal;

    fn bar(h: &str, l: &str, c: &str, vol: &str) -> BarInput {
        BarInput::new(
            l.parse().unwrap(),
            h.parse().unwrap(),
            l.parse().unwrap(),
            c.parse().unwrap(),
            vol.parse().unwrap(),
        )
    }

    #[test]
    fn test_chaikin_invalid() {
        assert!(ChaikinOsc::new("c", 0, 10).is_err());
        assert!(ChaikinOsc::new("c", 10, 3).is_err()); // fast >= slow
    }

    #[test]
    fn test_chaikin_unavailable_before_slow() {
        let mut c = ChaikinOsc::new("c", 3, 5).unwrap();
        for _ in 0..4 {
            assert_eq!(c.update(&bar("110", "90", "105", "1000")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_chaikin_ready_after_slow() {
        let mut c = ChaikinOsc::new("c", 3, 5).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 {
            last = c.update(&bar("110", "90", "105", "1000")).unwrap();
        }
        assert!(c.is_ready());
        assert!(matches!(last, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_chaikin_zero_hl_range() {
        // When high == low, CLV = 0 → AD unchanged
        let mut c = ChaikinOsc::new("c", 2, 4).unwrap();
        for _ in 0..4 {
            c.update(&bar("100", "100", "100", "1000")).unwrap();
        }
        assert!(c.is_ready());
    }

    #[test]
    fn test_chaikin_reset() {
        let mut c = ChaikinOsc::new("c", 3, 5).unwrap();
        for _ in 0..5 { c.update(&bar("110", "90", "105", "1000")).unwrap(); }
        assert!(c.is_ready());
        c.reset();
        assert!(!c.is_ready());
    }

    #[test]
    fn test_chaikin_period_and_name() {
        let c = ChaikinOsc::new("my_c", 3, 10).unwrap();
        assert_eq!(c.period(), 10);
        assert_eq!(c.name(), "my_c");
    }
}
