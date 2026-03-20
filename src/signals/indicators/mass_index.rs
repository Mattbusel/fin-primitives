//! Mass Index indicator.
//!
//! Detects trend reversals by measuring the widening and narrowing of the
//! high-low price range. A "reversal bulge" occurs when the 25-bar sum
//! rises above 27 and then falls below 26.5.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Mass Index: sum over `period` bars of `EMA9(high-low) / EMA9(EMA9(high-low))`.
///
/// Uses an EMA period of 9 internally. Returns
/// [`crate::signals::SignalValue::Unavailable`] until enough bars have been seen
/// to fully warm up both EMAs and the summation window.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::MassIndex;
/// use fin_primitives::signals::Signal;
/// let mi = MassIndex::new("mi", 25).unwrap();
/// assert_eq!(mi.period(), 25);
/// assert!(!mi.is_ready());
/// ```
pub struct MassIndex {
    name: String,
    period: usize,
    ema1: Option<Decimal>,
    ema2: Option<Decimal>,
    ema_period: usize,
    ema1_count: usize,
    ema2_count: usize,
    ratios: VecDeque<Decimal>,
    k: Decimal,
}

impl MassIndex {
    /// Constructs a new `MassIndex` with the given name and summation period.
    ///
    /// The internal EMA uses a fixed period of 9.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        let ema_period = 9;
        let k = Decimal::TWO / Decimal::from(ema_period as u32 + 1);
        Ok(Self {
            name: name.into(),
            period,
            ema1: None,
            ema2: None,
            ema_period,
            ema1_count: 0,
            ema2_count: 0,
            ratios: VecDeque::with_capacity(period),
            k,
        })
    }
}

impl Signal for MassIndex {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let hl = bar.high - bar.low;

        // EMA1: EMA of (high - low)
        self.ema1_count += 1;
        let ema1 = match self.ema1 {
            None => {
                self.ema1 = Some(hl);
                hl
            }
            Some(prev) => {
                let v = prev + self.k * (hl - prev);
                self.ema1 = Some(v);
                v
            }
        };

        // EMA2: EMA of EMA1 (only available once EMA1 is warmed up)
        if self.ema1_count >= self.ema_period {
            self.ema2_count += 1;
            let ema2 = match self.ema2 {
                None => {
                    self.ema2 = Some(ema1);
                    ema1
                }
                Some(prev) => {
                    let v = prev + self.k * (ema1 - prev);
                    self.ema2 = Some(v);
                    v
                }
            };

            if self.ema2_count >= self.ema_period && !ema2.is_zero() {
                let ratio = ema1 / ema2;
                self.ratios.push_back(ratio);
                if self.ratios.len() > self.period {
                    self.ratios.pop_front();
                }
            }
        }

        if self.ratios.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let mass: Decimal = self.ratios.iter().copied().sum();
        Ok(SignalValue::Scalar(mass))
    }

    fn is_ready(&self) -> bool {
        self.ratios.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.ema1 = None;
        self.ema2 = None;
        self.ema1_count = 0;
        self.ema2_count = 0;
        self.ratios.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals::Signal;
    use rust_decimal_macros::dec;

    fn bar(high: &str, low: &str) -> BarInput {
        BarInput::new(
            low.parse().unwrap(),
            high.parse().unwrap(),
            low.parse().unwrap(),
            low.parse().unwrap(),
            dec!(1000),
        )
    }

    #[test]
    fn test_mass_index_invalid_period() {
        assert!(MassIndex::new("mi", 0).is_err());
    }

    #[test]
    fn test_mass_index_unavailable_before_warmup() {
        let mut mi = MassIndex::new("mi", 25).unwrap();
        assert!(!mi.is_ready());
        mi.update(&bar("105", "95")).unwrap();
        assert!(!mi.is_ready());
    }

    #[test]
    fn test_mass_index_ready_after_enough_bars() {
        // Needs 9 (EMA1 warmup) + 9 (EMA2 warmup) + period - 1 bars
        let mut mi = MassIndex::new("mi", 5).unwrap();
        let total = 9 + 9 + 5 - 1;
        let mut last = SignalValue::Unavailable;
        for i in 0..total {
            let h = format!("{}", 105 + i);
            let l = format!("{}", 95 + i);
            last = mi.update(&bar(&h, &l)).unwrap();
        }
        assert!(mi.is_ready());
        assert!(matches!(last, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_mass_index_scalar_positive() {
        let mut mi = MassIndex::new("mi", 3).unwrap();
        let total = 9 + 9 + 3 - 1;
        let mut last = SignalValue::Unavailable;
        for _ in 0..total {
            last = mi.update(&bar("110", "90")).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(0), "mass index should be positive: {}", v);
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_mass_index_reset_clears_state() {
        let mut mi = MassIndex::new("mi", 3).unwrap();
        let total = 9 + 9 + 3 - 1;
        for _ in 0..total {
            mi.update(&bar("110", "90")).unwrap();
        }
        assert!(mi.is_ready());
        mi.reset();
        assert!(!mi.is_ready());
    }

    #[test]
    fn test_mass_index_period_and_name() {
        let mi = MassIndex::new("my_mi", 25).unwrap();
        assert_eq!(mi.period(), 25);
        assert_eq!(mi.name(), "my_mi");
    }
}
