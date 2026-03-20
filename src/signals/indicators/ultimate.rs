//! Ultimate Oscillator — Larry Williams' multi-period momentum indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Ultimate Oscillator — combines short, medium, and long buying-pressure averages.
///
/// ```text
/// BP  = close - min(low, prev_close)
/// TR  = max(high, prev_close) - min(low, prev_close)
/// Avg(n) = sum(BP, n) / sum(TR, n)
/// UO  = 100 * (4×Avg(fast) + 2×Avg(mid) + Avg(slow)) / 7
/// ```
///
/// Defaults: `fast=7`, `mid=14`, `slow=28`.
/// Returns [`SignalValue::Unavailable`] until `slow` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::UltimateOscillator;
/// use fin_primitives::signals::Signal;
///
/// let uo = UltimateOscillator::new("uo", 7, 14, 28).unwrap();
/// assert_eq!(uo.period(), 28);
/// ```
pub struct UltimateOscillator {
    name: String,
    fast: usize,
    mid: usize,
    slow: usize,
    prev_close: Option<Decimal>,
    bps: VecDeque<Decimal>,
    trs: VecDeque<Decimal>,
}

impl UltimateOscillator {
    /// Constructs a new `UltimateOscillator` with custom periods.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if any period is 0 or not strictly increasing.
    pub fn new(
        name: impl Into<String>,
        fast: usize,
        mid: usize,
        slow: usize,
    ) -> Result<Self, FinError> {
        if fast == 0 { return Err(FinError::InvalidPeriod(fast)); }
        if mid == 0  { return Err(FinError::InvalidPeriod(mid)); }
        if slow == 0 { return Err(FinError::InvalidPeriod(slow)); }
        if fast >= mid || mid >= slow {
            return Err(FinError::InvalidInput(format!(
                "periods must satisfy fast < mid < slow, got {fast}, {mid}, {slow}"
            )));
        }
        Ok(Self {
            name: name.into(),
            fast,
            mid,
            slow,
            prev_close: None,
            bps: VecDeque::with_capacity(slow),
            trs: VecDeque::with_capacity(slow),
        })
    }

    fn window_ratio(bps: &VecDeque<Decimal>, trs: &VecDeque<Decimal>, n: usize) -> Decimal {
        let bp_sum: Decimal = bps.iter().rev().take(n).sum();
        let tr_sum: Decimal = trs.iter().rev().take(n).sum();
        if tr_sum.is_zero() {
            Decimal::ZERO
        } else {
            bp_sum / tr_sum
        }
    }
}

impl Signal for UltimateOscillator {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let prev_close = match self.prev_close {
            Some(pc) => pc,
            None => {
                self.prev_close = Some(bar.close);
                return Ok(SignalValue::Unavailable);
            }
        };

        let true_low = bar.low.min(prev_close);
        let true_high = bar.high.max(prev_close);
        let bp = bar.close - true_low;
        let tr = true_high - true_low;

        self.bps.push_back(bp);
        self.trs.push_back(tr);
        if self.bps.len() > self.slow {
            self.bps.pop_front();
            self.trs.pop_front();
        }

        self.prev_close = Some(bar.close);

        if self.bps.len() < self.slow {
            return Ok(SignalValue::Unavailable);
        }

        let avg_fast = Self::window_ratio(&self.bps, &self.trs, self.fast);
        let avg_mid  = Self::window_ratio(&self.bps, &self.trs, self.mid);
        let avg_slow = Self::window_ratio(&self.bps, &self.trs, self.slow);

        let uo = Decimal::ONE_HUNDRED
            * (Decimal::from(4u32) * avg_fast + Decimal::TWO * avg_mid + avg_slow)
            / Decimal::from(7u32);

        Ok(SignalValue::Scalar(uo))
    }

    fn is_ready(&self) -> bool {
        self.bps.len() >= self.slow
    }

    fn period(&self) -> usize {
        self.slow
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.bps.clear();
        self.trs.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};

    fn bar(h: &str, l: &str, c: &str) -> OhlcvBar {
        let hi = Price::new(h.parse().unwrap()).unwrap();
        let lo = Price::new(l.parse().unwrap()).unwrap();
        let cl = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cl,
            high: hi,
            low: lo,
            close: cl,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_uo_invalid_period_zero() {
        assert!(UltimateOscillator::new("uo", 0, 14, 28).is_err());
    }

    #[test]
    fn test_uo_invalid_order() {
        assert!(UltimateOscillator::new("uo", 14, 7, 28).is_err());
        assert!(UltimateOscillator::new("uo", 7, 28, 14).is_err());
    }

    #[test]
    fn test_uo_unavailable_before_slow_bars() {
        let mut uo = UltimateOscillator::new("uo", 2, 3, 5).unwrap();
        for _ in 0..4 {
            assert_eq!(uo.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!uo.is_ready());
    }

    #[test]
    fn test_uo_ready_after_slow_bars() {
        let mut uo = UltimateOscillator::new("uo", 2, 3, 5).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..6 {
            last = uo.update_bar(&bar("110", "90", "100")).unwrap();
        }
        assert!(uo.is_ready());
        assert!(matches!(last, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_uo_flat_price_returns_fifty() {
        // Flat: high=low=close=100 always; BP = close - low = 0, TR = 0 → ratio = 0 → UO = 0
        // Actually: BP = 100 - 100 = 0, TR = 100 - 100 = 0 → window_ratio returns 0 → UO = 0
        let mut uo = UltimateOscillator::new("uo", 2, 3, 5).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..7 {
            last = uo.update_bar(&bar("100", "100", "100")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(rust_decimal_macros::dec!(0)));
    }

    #[test]
    fn test_uo_reset() {
        let mut uo = UltimateOscillator::new("uo", 2, 3, 5).unwrap();
        for _ in 0..6 {
            uo.update_bar(&bar("110", "90", "100")).unwrap();
        }
        assert!(uo.is_ready());
        uo.reset();
        assert!(!uo.is_ready());
    }
}
