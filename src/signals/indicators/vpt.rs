//! Volume Price Trend (VPT) indicator.
//!
//! A cumulative volume-based indicator that relates volume to price changes.
//! `VPT += volume × (close - prev_close) / prev_close`

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Volume Price Trend: cumulative sum of `volume × ROC(1)`.
///
/// Starts from zero; requires 2 bars before producing the first value.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Vpt;
/// use fin_primitives::signals::Signal;
/// let v = Vpt::new("vpt").unwrap();
/// assert_eq!(v.period(), 1);
/// assert!(!v.is_ready());
/// ```
pub struct Vpt {
    name: String,
    prev_close: Option<Decimal>,
    vpt: Decimal,
}

impl Vpt {
    /// Constructs a new `Vpt` indicator.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self {
            name: name.into(),
            prev_close: None,
            vpt: Decimal::ZERO,
        })
    }
}

impl Signal for Vpt {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            if !pc.is_zero() {
                self.vpt += bar.volume * (bar.close - pc) / pc;
            }
            self.prev_close = Some(bar.close);
            Ok(SignalValue::Scalar(self.vpt))
        } else {
            self.prev_close = Some(bar.close);
            Ok(SignalValue::Unavailable)
        }
    }

    fn is_ready(&self) -> bool {
        self.prev_close.is_some() && self.vpt != Decimal::ZERO
            || self.prev_close.is_some() && self.vpt == Decimal::ZERO
                && matches!(self.prev_close, Some(_))
    }

    fn period(&self) -> usize {
        1
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.vpt = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals::Signal;
    use rust_decimal_macros::dec;

    fn bar(close: &str, vol: &str) -> BarInput {
        BarInput::new(
            close.parse().unwrap(),
            close.parse().unwrap(),
            close.parse().unwrap(),
            close.parse().unwrap(),
            vol.parse().unwrap(),
        )
    }

    #[test]
    fn test_vpt_unavailable_on_first_bar() {
        let mut v = Vpt::new("vpt").unwrap();
        assert_eq!(v.update(&bar("100", "1000")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_vpt_positive_when_price_rises() {
        let mut v = Vpt::new("vpt").unwrap();
        v.update(&bar("100", "1000")).unwrap();
        let sv = v.update(&bar("110", "1000")).unwrap();
        if let SignalValue::Scalar(val) = sv {
            assert!(val > dec!(0), "vpt should be positive: {}", val);
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_vpt_negative_when_price_falls() {
        let mut v = Vpt::new("vpt").unwrap();
        v.update(&bar("110", "1000")).unwrap();
        let sv = v.update(&bar("100", "1000")).unwrap();
        if let SignalValue::Scalar(val) = sv {
            assert!(val < dec!(0), "vpt should be negative: {}", val);
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_vpt_cumulative() {
        let mut v = Vpt::new("vpt").unwrap();
        v.update(&bar("100", "1000")).unwrap();
        v.update(&bar("110", "1000")).unwrap(); // vpt += 1000 * 10/100 = 100
        let sv = v.update(&bar("105", "500")).unwrap(); // vpt += 500 * -5/110 ≈ -22.7
        if let SignalValue::Scalar(val) = sv {
            // cumulative: ~77.3
            assert!(val > dec!(0), "vpt should still be positive: {}", val);
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_vpt_reset_clears_state() {
        let mut v = Vpt::new("vpt").unwrap();
        v.update(&bar("100", "1000")).unwrap();
        v.update(&bar("110", "1000")).unwrap();
        v.reset();
        assert_eq!(v.update(&bar("100", "1000")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_vpt_period_and_name() {
        let v = Vpt::new("my_vpt").unwrap();
        assert_eq!(v.period(), 1);
        assert_eq!(v.name(), "my_vpt");
    }

    #[test]
    fn test_vpt_zero_prev_close_skipped() {
        let mut v = Vpt::new("vpt").unwrap();
        // Manually setting up a scenario with zero prev close via reset is not needed.
        // This test ensures zero-volume doesn't crash
        v.update(&bar("0", "1000")).unwrap(); // edge case: close = 0, so prev_close = 0 next
        let sv = v.update(&bar("100", "1000")).unwrap();
        // prev_close = 0, so update is skipped, returns Scalar(0)
        assert!(matches!(sv, SignalValue::Scalar(_)));
    }
}
