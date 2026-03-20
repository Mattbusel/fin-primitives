//! Pivot Strength — ATR-normalised distance from the classic pivot point.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Pivot Strength — distance from the classic pivot point measured in ATR units.
///
/// The pivot point is: `PP = (prev_high + prev_low + prev_close) / 3`.
/// This indicator computes how many ATRs the current close is above or below that pivot:
///
/// ```text
/// pivot_strength = (close - PP) / ATR(period)
/// ```
///
/// - **Positive**: close is above the pivot (bullish zone).
/// - **Negative**: close is below the pivot (bearish zone).
/// - The magnitude shows how extreme the displacement is relative to recent volatility.
///
/// Requires at least 2 bars to establish a "previous" bar (for the pivot calculation),
/// plus `period` bars to build the ATR. Returns [`SignalValue::Unavailable`] until ready
/// or when ATR is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PivotStrength;
/// use fin_primitives::signals::Signal;
/// let ps = PivotStrength::new("ps_14", 14).unwrap();
/// assert_eq!(ps.period(), 14);
/// ```
pub struct PivotStrength {
    name: String,
    period: usize,
    prev_bar: Option<(Decimal, Decimal, Decimal)>,
    tr_values: VecDeque<Decimal>,
    prev_close: Option<Decimal>,
}

impl PivotStrength {
    /// Constructs a new `PivotStrength`.
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
            prev_bar: None,
            tr_values: VecDeque::with_capacity(period),
            prev_close: None,
        })
    }
}

impl Signal for PivotStrength {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.tr_values.len() >= self.period && self.prev_bar.is_some()
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        // Compute true range.
        let tr = match self.prev_close {
            None => {
                self.prev_close = Some(bar.close);
                self.prev_bar = Some((bar.high, bar.low, bar.close));
                return Ok(SignalValue::Unavailable);
            }
            Some(pc) => {
                let hl = bar.range();
                let hc = (bar.high - pc).abs();
                let lc = (bar.low - pc).abs();
                hl.max(hc).max(lc)
            }
        };
        self.prev_close = Some(bar.close);

        self.tr_values.push_back(tr);
        if self.tr_values.len() > self.period {
            self.tr_values.pop_front();
        }

        let prev = self.prev_bar;
        self.prev_bar = Some((bar.high, bar.low, bar.close));

        if self.tr_values.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let (ph, pl, pc) = match prev {
            Some(p) => p,
            None => return Ok(SignalValue::Unavailable),
        };

        let pivot = (ph + pl + pc)
            .checked_div(Decimal::from(3u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        let sum: Decimal = self.tr_values.iter().copied().sum();
        #[allow(clippy::cast_possible_truncation)]
        let atr = sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        if atr.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let strength = (bar.close - pivot)
            .checked_div(atr)
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(strength))
    }

    fn reset(&mut self) {
        self.prev_bar = None;
        self.tr_values.clear();
        self.prev_close = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str, c: &str) -> OhlcvBar {
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(c.parse().unwrap()).unwrap(),
            high: Price::new(h.parse().unwrap()).unwrap(),
            low: Price::new(l.parse().unwrap()).unwrap(),
            close: Price::new(c.parse().unwrap()).unwrap(),
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_ps_invalid_period() {
        assert!(PivotStrength::new("ps", 0).is_err());
    }

    #[test]
    fn test_ps_unavailable_early() {
        let mut ps = PivotStrength::new("ps", 3).unwrap();
        let v = ps.update_bar(&bar("105", "95", "100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
        assert!(!ps.is_ready());
    }

    #[test]
    fn test_ps_produces_value_after_warm_up() {
        let mut ps = PivotStrength::new("ps", 2).unwrap();
        let b = bar("105", "95", "100");
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 {
            last = ps.update_bar(&b).unwrap();
        }
        assert!(last.is_scalar(), "expected Scalar after warm-up");
        assert!(ps.is_ready());
    }

    #[test]
    fn test_ps_close_at_pivot_gives_zero() {
        let mut ps = PivotStrength::new("ps", 2).unwrap();
        // Use h=110, l=90, c=100 → pivot = (110+90+100)/3 = 100.
        // If next bar's close is also 100, strength should be 0.
        let ref_bar = bar("110", "90", "100");
        ps.update_bar(&ref_bar).unwrap();
        ps.update_bar(&ref_bar).unwrap();
        ps.update_bar(&ref_bar).unwrap();
        let v = ps.update_bar(&ref_bar).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s.abs() < dec!(0.001), "expected ~0, got {s}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ps_reset() {
        let mut ps = PivotStrength::new("ps", 2).unwrap();
        let b = bar("105", "95", "100");
        for _ in 0..5 {
            ps.update_bar(&b).unwrap();
        }
        ps.reset();
        assert!(!ps.is_ready());
    }

    #[test]
    fn test_ps_period_and_name() {
        let ps = PivotStrength::new("my_ps", 14).unwrap();
        assert_eq!(ps.period(), 14);
        assert_eq!(ps.name(), "my_ps");
    }
}
