//! Wilder Smoothed Range indicator.

use rust_decimal::Decimal;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Wilder-smoothed (RMA) true range — the smoothing used in ATR.
///
/// `smoothed[t] = (smoothed[t-1] * (period-1) + true_range[t]) / period`
///
/// Equivalent to `ATR` but exposed as a standalone indicator.
/// Initial value seeded after `period` bars of simple averaging.
pub struct WilderSmoothedRange {
    period: usize,
    smoothed: Option<Decimal>,
    warm_up_count: usize,
    warm_up_sum: Decimal,
    prev_close: Option<Decimal>,
}

impl WilderSmoothedRange {
    /// Creates a new `WilderSmoothedRange` with the given smoothing period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            smoothed: None,
            warm_up_count: 0,
            warm_up_sum: Decimal::ZERO,
            prev_close: None,
        })
    }

    fn true_range(bar: &BarInput, prev_close: Option<Decimal>) -> Decimal {
        let hl = bar.high - bar.low;
        match prev_close {
            Some(pc) => {
                let hc = (bar.high - pc).abs();
                let lc = (bar.low - pc).abs();
                hl.max(hc).max(lc)
            }
            None => hl,
        }
    }
}

impl Signal for WilderSmoothedRange {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = Self::true_range(bar, self.prev_close);
        self.prev_close = Some(bar.close);

        match self.smoothed {
            None => {
                self.warm_up_sum += tr;
                self.warm_up_count += 1;
                if self.warm_up_count >= self.period {
                    self.smoothed = Some(self.warm_up_sum / Decimal::from(self.period as u32));
                    Ok(SignalValue::Scalar(self.smoothed.unwrap()))
                } else {
                    Ok(SignalValue::Unavailable)
                }
            }
            Some(prev) => {
                let p = Decimal::from(self.period as u32);
                let new_val = (prev * (p - Decimal::ONE) + tr) / p;
                self.smoothed = Some(new_val);
                Ok(SignalValue::Scalar(new_val))
            }
        }
    }

    fn is_ready(&self) -> bool { self.smoothed.is_some() }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) {
        self.smoothed = None;
        self.warm_up_count = 0;
        self.warm_up_sum = Decimal::ZERO;
        self.prev_close = None;
    }
    fn name(&self) -> &str { "WilderSmoothedRange" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str, c: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: h.parse().unwrap(),
            low: l.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_wsr_constant_range() {
        // Constant range of 20 → smoothed value converges to 20
        let mut sig = WilderSmoothedRange::new(3).unwrap();
        sig.update(&bar("110", "90", "100")).unwrap();
        sig.update(&bar("110", "90", "100")).unwrap();
        let v = sig.update(&bar("110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(20)));
        // More bars → stays at 20
        let v2 = sig.update(&bar("110", "90", "100")).unwrap();
        assert_eq!(v2, SignalValue::Scalar(dec!(20)));
    }

    #[test]
    fn test_wsr_not_ready() {
        let mut sig = WilderSmoothedRange::new(3).unwrap();
        assert_eq!(sig.update(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(sig.update(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
    }
}
