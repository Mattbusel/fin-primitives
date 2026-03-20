//! Negative Volume Index (NVI).
//!
//! Updates only on days when volume is lower than the previous day.
//! Tracks the "smart money" — institutional traders who act quietly on low-volume days.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Negative Volume Index: starts at 1000 and adjusts on days with lower volume.
///
/// - When `volume < prev_volume`: `NVI += NVI * (close - prev_close) / prev_close`
/// - When `volume >= prev_volume`: NVI unchanged
///
/// Returns [`crate::signals::SignalValue::Unavailable`] until the second bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Nvi;
/// use fin_primitives::signals::Signal;
/// let n = Nvi::new("nvi").unwrap();
/// assert_eq!(n.period(), 1);
/// assert!(!n.is_ready());
/// ```
pub struct Nvi {
    name: String,
    nvi: Decimal,
    prev_close: Option<Decimal>,
    prev_volume: Option<Decimal>,
    ready: bool,
}

impl Nvi {
    /// Constructs a new `Nvi` starting at 1000.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self {
            name: name.into(),
            nvi: Decimal::from(1000u32),
            prev_close: None,
            prev_volume: None,
            ready: false,
        })
    }
}

impl Signal for Nvi {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let (Some(pc), Some(pv)) = (self.prev_close, self.prev_volume) {
            if bar.volume < pv && !pc.is_zero() {
                self.nvi += self.nvi * (bar.close - pc) / pc;
            }
            self.ready = true;
        }
        self.prev_close = Some(bar.close);
        self.prev_volume = Some(bar.volume);

        if self.ready {
            Ok(SignalValue::Scalar(self.nvi))
        } else {
            Ok(SignalValue::Unavailable)
        }
    }

    fn is_ready(&self) -> bool {
        self.ready
    }

    fn period(&self) -> usize {
        1
    }

    fn reset(&mut self) {
        self.nvi = Decimal::from(1000u32);
        self.prev_close = None;
        self.prev_volume = None;
        self.ready = false;
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
    fn test_nvi_unavailable_on_first_bar() {
        let mut n = Nvi::new("nvi").unwrap();
        assert_eq!(n.update(&bar("100", "1000")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_nvi_ready_on_second_bar() {
        let mut n = Nvi::new("nvi").unwrap();
        n.update(&bar("100", "1000")).unwrap();
        let sv = n.update(&bar("110", "1000")).unwrap();
        assert!(n.is_ready());
        assert!(matches!(sv, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_nvi_updates_on_lower_volume() {
        let mut n = Nvi::new("nvi").unwrap();
        n.update(&bar("100", "2000")).unwrap();
        let sv = n.update(&bar("110", "1000")).unwrap(); // lower vol → nvi should update
        if let SignalValue::Scalar(v) = sv {
            assert!(v > dec!(1000), "nvi should increase on price up with lower vol: {}", v);
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_nvi_unchanged_on_equal_or_higher_volume() {
        let mut n = Nvi::new("nvi").unwrap();
        n.update(&bar("100", "1000")).unwrap();
        let sv = n.update(&bar("110", "2000")).unwrap(); // higher vol → nvi unchanged
        if let SignalValue::Scalar(v) = sv {
            assert_eq!(v, dec!(1000), "nvi should be unchanged: {}", v);
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_nvi_reset_clears_state() {
        let mut n = Nvi::new("nvi").unwrap();
        n.update(&bar("100", "2000")).unwrap();
        n.update(&bar("110", "1000")).unwrap();
        n.reset();
        assert!(!n.is_ready());
        assert_eq!(n.update(&bar("100", "1000")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_nvi_period_and_name() {
        let n = Nvi::new("my_nvi").unwrap();
        assert_eq!(n.period(), 1);
        assert_eq!(n.name(), "my_nvi");
    }
}
