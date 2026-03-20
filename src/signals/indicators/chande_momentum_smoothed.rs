//! Smoothed Chande Momentum Oscillator (CMO) — CMO with EMA smoothing.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Smoothed Chande Momentum Oscillator.
///
/// Computes the standard CMO over `period` bars, then applies an EMA with
/// `smooth_period` to reduce whipsaw. Suitable for trend-following systems
/// that need reduced noise compared to the raw CMO.
///
/// CMO formula:
/// ```text
/// CMO = (sum_up - sum_down) / (sum_up + sum_down) * 100
/// ```
/// where `sum_up` = sum of positive price changes and `sum_down` = sum of
/// absolute negative price changes over the last `period` bars.
///
/// Returns [`SignalValue::Unavailable`] until `period + smooth_period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ChandeMomentumSmoothed;
/// use fin_primitives::signals::Signal;
/// let cmos = ChandeMomentumSmoothed::new("cmos", 14, 3).unwrap();
/// assert_eq!(cmos.period(), 14);
/// ```
pub struct ChandeMomentumSmoothed {
    name: String,
    period: usize,
    smooth_period: usize,
    changes: VecDeque<Decimal>,
    prev_close: Option<Decimal>,
    /// EMA of raw CMO values (seeded with SMA of first `smooth_period` values).
    ema: Option<Decimal>,
    smooth_k: Decimal,
    /// Accumulator for seeding the EMA.
    seed_buf: Vec<Decimal>,
}

impl ChandeMomentumSmoothed {
    /// Constructs a new `ChandeMomentumSmoothed`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0` or `smooth_period == 0`.
    pub fn new(
        name: impl Into<String>,
        period: usize,
        smooth_period: usize,
    ) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        if smooth_period == 0 {
            return Err(FinError::InvalidPeriod(smooth_period));
        }
        #[allow(clippy::cast_possible_truncation)]
        let smooth_k = Decimal::from(2u32)
            .checked_div(Decimal::from(smooth_period as u32 + 1))
            .unwrap_or(Decimal::ONE);
        Ok(Self {
            name: name.into(),
            period,
            smooth_period,
            changes: VecDeque::with_capacity(period),
            prev_close: None,
            ema: None,
            smooth_k,
            seed_buf: Vec::with_capacity(smooth_period),
        })
    }

    fn compute_cmo(&self) -> Option<Decimal> {
        if self.changes.len() < self.period {
            return None;
        }
        let mut sum_up = Decimal::ZERO;
        let mut sum_down = Decimal::ZERO;
        for &c in &self.changes {
            if c > Decimal::ZERO {
                sum_up += c;
            } else {
                sum_down += -c;
            }
        }
        let total = sum_up + sum_down;
        if total.is_zero() {
            return Some(Decimal::ZERO);
        }
        let cmo = (sum_up - sum_down)
            .checked_div(total)?
            .checked_mul(Decimal::ONE_HUNDRED)?;
        Some(cmo)
    }
}

impl Signal for ChandeMomentumSmoothed {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.ema.is_some()
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let Some(prev) = self.prev_close else {
            self.prev_close = Some(bar.close);
            return Ok(SignalValue::Unavailable);
        };
        self.prev_close = Some(bar.close);

        let change = bar.close - prev;
        self.changes.push_back(change);
        if self.changes.len() > self.period {
            self.changes.pop_front();
        }

        let Some(raw_cmo) = self.compute_cmo() else {
            return Ok(SignalValue::Unavailable);
        };

        // Seed EMA with SMA of first `smooth_period` CMO values.
        if self.ema.is_none() {
            self.seed_buf.push(raw_cmo);
            if self.seed_buf.len() < self.smooth_period {
                return Ok(SignalValue::Unavailable);
            }
            let seed_sum: Decimal = self.seed_buf.iter().copied().sum();
            #[allow(clippy::cast_possible_truncation)]
            let seed_avg = seed_sum
                .checked_div(Decimal::from(self.smooth_period as u32))
                .ok_or(FinError::ArithmeticOverflow)?;
            self.ema = Some(seed_avg);
            return Ok(SignalValue::Scalar(seed_avg));
        }

        let prev_ema = self.ema.unwrap();
        let new_ema = raw_cmo * self.smooth_k + prev_ema * (Decimal::ONE - self.smooth_k);
        self.ema = Some(new_ema);
        Ok(SignalValue::Scalar(new_ema))
    }

    fn reset(&mut self) {
        self.changes.clear();
        self.prev_close = None;
        self.ema = None;
        self.seed_buf.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(close: &str) -> OhlcvBar {
        let p = Price::new(close.parse().unwrap()).unwrap();
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
    fn test_cmos_invalid_period() {
        assert!(ChandeMomentumSmoothed::new("c", 0, 3).is_err());
        assert!(ChandeMomentumSmoothed::new("c", 3, 0).is_err());
    }

    #[test]
    fn test_cmos_unavailable_before_warm_up() {
        // period=3, smooth_period=2:
        // bar 0 → sets prev_close → Unavailable
        // bar 1..3 → fills change window (needs period changes) → Unavailable
        // bar 4 → first CMO, seeds smooth buf (1 of 2) → Unavailable
        // bar 5 → second CMO, seeds smooth buf (2 of 2) → first Scalar
        let mut cmos = ChandeMomentumSmoothed::new("c", 3, 2).unwrap();
        for i in 0..4u32 {
            let v = cmos.update_bar(&bar(&(100 + i).to_string())).unwrap();
            assert_eq!(v, SignalValue::Unavailable, "bar {i} should be Unavailable");
        }
        assert!(!cmos.is_ready());
    }

    #[test]
    fn test_cmos_produces_value_after_warm_up() {
        let mut cmos = ChandeMomentumSmoothed::new("c", 3, 2).unwrap();
        // Need 1 (prev) + 3 (cmo period) + 2 (smooth seed) = 6 bars total before first output.
        // Actually need prev_close (1) + period changes (3) + smooth_period seeds (2).
        let mut last = SignalValue::Unavailable;
        for i in 0..8u32 {
            last = cmos.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        assert!(last.is_scalar(), "expected a scalar after warm-up");
        assert!(cmos.is_ready());
    }

    #[test]
    fn test_cmos_all_gains_positive_output() {
        let mut cmos = ChandeMomentumSmoothed::new("c", 3, 1).unwrap();
        // Rising prices → CMO should be positive (100), smoothed stays positive.
        let mut last = SignalValue::Unavailable;
        for i in 0..6u32 {
            last = cmos.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(0), "expected positive CMOS for all gains, got {v}");
        } else {
            panic!("expected Scalar after warm-up");
        }
    }

    #[test]
    fn test_cmos_output_in_bounds() {
        let mut cmos = ChandeMomentumSmoothed::new("c", 5, 3).unwrap();
        let prices = [
            "100", "102", "101", "103", "102", "105", "104", "106", "105", "108",
        ];
        for p in &prices {
            if let SignalValue::Scalar(v) = cmos.update_bar(&bar(p)).unwrap() {
                assert!(v >= dec!(-100), "CMOS below -100: {v}");
                assert!(v <= dec!(100), "CMOS above 100: {v}");
            }
        }
    }

    #[test]
    fn test_cmos_reset() {
        let mut cmos = ChandeMomentumSmoothed::new("c", 3, 2).unwrap();
        for i in 0..8u32 {
            cmos.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        assert!(cmos.is_ready());
        cmos.reset();
        assert!(!cmos.is_ready());
        let v = cmos.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_cmos_name_and_period() {
        let cmos = ChandeMomentumSmoothed::new("my_cmos", 14, 3).unwrap();
        assert_eq!(cmos.name(), "my_cmos");
        assert_eq!(cmos.period(), 14);
    }
}
