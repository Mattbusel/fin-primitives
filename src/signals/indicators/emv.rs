//! Ease of Movement (EMV) indicator.
//!
//! Measures the ease with which price moves. A positive value means price moved up
//! easily on low volume; a negative value means price moved down easily on low volume.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Ease of Movement: `SMA(period, (midpoint_move / box_ratio))`.
///
/// - `midpoint_move = (high + low) / 2 - (prev_high + prev_low) / 2`
/// - `box_ratio = volume / (high - low)` (normalized; we scale volume by 1e6)
/// - `EMV(1) = midpoint_move / box_ratio`
/// - `EMV(n) = SMA(n, EMV(1))`
///
/// Returns [`crate::signals::SignalValue::Unavailable`] until `period + 1` bars are seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Emv;
/// use fin_primitives::signals::Signal;
/// let e = Emv::new("emv14", 14).unwrap();
/// assert_eq!(e.period(), 14);
/// assert!(!e.is_ready());
/// ```
pub struct Emv {
    name: String,
    period: usize,
    prev_mid: Option<Decimal>,
    raw_values: VecDeque<Decimal>,
}

impl Emv {
    /// Constructs a new `Emv` indicator.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            prev_mid: None,
            raw_values: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for Emv {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let mid = (bar.high + bar.low) / Decimal::TWO;
        let hl = bar.high - bar.low;

        if let Some(prev_mid) = self.prev_mid {
            if !hl.is_zero() && !bar.volume.is_zero() {
                let midpoint_move = mid - prev_mid;
                // box_ratio = (volume / 1e6) / (high - low); we scale for typical values
                let box_ratio = bar.volume / Decimal::from(1_000_000u64) / hl;
                if !box_ratio.is_zero() {
                    let raw_emv = midpoint_move / box_ratio;
                    self.raw_values.push_back(raw_emv);
                    if self.raw_values.len() > self.period {
                        self.raw_values.pop_front();
                    }
                }
            }
        }
        self.prev_mid = Some(mid);

        if self.raw_values.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let sum: Decimal = self.raw_values.iter().copied().sum();
        #[allow(clippy::cast_possible_truncation)]
        let avg = sum / Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(avg))
    }

    fn is_ready(&self) -> bool {
        self.raw_values.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.prev_mid = None;
        self.raw_values.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals::Signal;

    fn bar(high: &str, low: &str, vol: &str) -> BarInput {
        BarInput::new(
            low.parse().unwrap(),
            high.parse().unwrap(),
            low.parse().unwrap(),
            low.parse().unwrap(),
            vol.parse().unwrap(),
        )
    }

    #[test]
    fn test_emv_invalid_period() {
        assert!(Emv::new("e", 0).is_err());
    }

    #[test]
    fn test_emv_unavailable_before_warmup() {
        let mut e = Emv::new("e", 3).unwrap();
        e.update(&bar("105", "95", "500000")).unwrap();
        assert!(!e.is_ready());
    }

    #[test]
    fn test_emv_ready_after_period_bars() {
        let mut e = Emv::new("e", 2).unwrap();
        e.update(&bar("105", "95", "500000")).unwrap();
        e.update(&bar("108", "98", "400000")).unwrap();
        let sv = e.update(&bar("110", "100", "600000")).unwrap();
        assert!(e.is_ready());
        assert!(matches!(sv, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_emv_reset_clears_state() {
        let mut e = Emv::new("e", 2).unwrap();
        for _ in 0..3 {
            e.update(&bar("105", "95", "500000")).unwrap();
        }
        e.reset();
        assert!(!e.is_ready());
    }

    #[test]
    fn test_emv_period_and_name() {
        let e = Emv::new("my_emv", 14).unwrap();
        assert_eq!(e.period(), 14);
        assert_eq!(e.name(), "my_emv");
    }

    #[test]
    fn test_emv_zero_volume_skipped() {
        let mut e = Emv::new("e", 1).unwrap();
        e.update(&bar("105", "95", "500000")).unwrap();
        // Zero volume should not advance the raw_values queue
        let sv = e.update(&bar("108", "98", "0")).unwrap();
        assert_eq!(sv, SignalValue::Unavailable);
    }
}
