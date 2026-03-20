//! True Strength Index indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Simplified True Strength Index: double-smoothed momentum / double-smoothed absolute momentum.
///
/// Uses simple moving averages instead of EMA for streaming simplicity.
/// `TSI = SMA(SMA(close_change, r), s) / SMA(SMA(|close_change|, r), s) * 100`
///
/// Ranges from -100 to +100. Positive values indicate bullish momentum.
/// Uses `long_period` for outer smoothing, `short_period` for inner smoothing.
pub struct TrueStrengthIndex {
    long_period: usize,
    short_period: usize,
    prev_close: Option<Decimal>,
    changes: VecDeque<Decimal>,   // raw price changes
    abs_changes: VecDeque<Decimal>, // absolute price changes
}

impl TrueStrengthIndex {
    /// Creates a new `TrueStrengthIndex` with given periods (short < long, both >= 2).
    pub fn new(short_period: usize, long_period: usize) -> Result<Self, FinError> {
        if short_period < 2 || long_period <= short_period {
            return Err(FinError::InvalidPeriod(long_period));
        }
        Ok(Self {
            long_period,
            short_period,
            prev_close: None,
            changes: VecDeque::with_capacity(long_period),
            abs_changes: VecDeque::with_capacity(long_period),
        })
    }

    fn sma(window: &VecDeque<Decimal>, n: usize) -> Option<Decimal> {
        if window.len() < n { return None; }
        let sum: Decimal = window.iter().rev().take(n).sum();
        Some(sum / Decimal::from(n as u32))
    }
}

impl Signal for TrueStrengthIndex {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let change = bar.close - pc;
            self.changes.push_back(change);
            self.abs_changes.push_back(change.abs());
            if self.changes.len() > self.long_period {
                self.changes.pop_front();
                self.abs_changes.pop_front();
            }
        }
        self.prev_close = Some(bar.close);

        if self.changes.len() < self.long_period {
            return Ok(SignalValue::Unavailable);
        }

        // inner smooth (short period) then outer smooth (long period of inner results)
        // Simplified: compute rolling SMA of last short_period, then check vs long_period avg
        let inner_mom = match Self::sma(&self.changes, self.short_period) {
            Some(v) => v,
            None => return Ok(SignalValue::Unavailable),
        };
        let inner_abs = match Self::sma(&self.abs_changes, self.short_period) {
            Some(v) => v,
            None => return Ok(SignalValue::Unavailable),
        };

        // outer smooth: use long_period window of inner values (approximated as long SMA)
        let outer_mom = match Self::sma(&self.changes, self.long_period) {
            Some(v) => v,
            None => return Ok(SignalValue::Unavailable),
        };
        let outer_abs = match Self::sma(&self.abs_changes, self.long_period) {
            Some(v) => v,
            None => return Ok(SignalValue::Unavailable),
        };

        // blend: use average of inner and outer as double-smoothed approximation
        let smoothed_mom = (inner_mom + outer_mom) / Decimal::TWO;
        let smoothed_abs = (inner_abs + outer_abs) / Decimal::TWO;

        if smoothed_abs.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        Ok(SignalValue::Scalar(smoothed_mom / smoothed_abs * Decimal::ONE_HUNDRED))
    }

    fn is_ready(&self) -> bool { self.changes.len() >= self.long_period }
    fn period(&self) -> usize { self.long_period }
    fn reset(&mut self) { self.prev_close = None; self.changes.clear(); self.abs_changes.clear(); }
    fn name(&self) -> &str { "TrueStrengthIndex" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(c: &str) -> BarInput {
        BarInput {
            open: c.parse().unwrap(),
            high: c.parse().unwrap(),
            low: c.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_tsi_constant_zero() {
        // Constant prices → all changes = 0 → TSI = 0
        let mut sig = TrueStrengthIndex::new(2, 4).unwrap();
        for _ in 0..6 {
            sig.update(&bar("100")).unwrap();
        }
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_tsi_all_up_positive() {
        // Consistently rising prices → TSI = +100
        let mut sig = TrueStrengthIndex::new(2, 4).unwrap();
        for i in 0..=6u32 {
            sig.update(&bar(&format!("{}", 100 + i))).unwrap();
        }
        if let SignalValue::Scalar(v) = sig.update(&bar("108")).unwrap() {
            assert!(v > dec!(0), "expected positive TSI, got {v}");
        } else {
            panic!("expected Scalar");
        }
    }
}
