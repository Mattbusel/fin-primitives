//! Open-Close Gap indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Open-Close Gap — rolling mean of the gap between the previous bar's close and
/// the current bar's open, expressed as a percentage of the previous close.
///
/// ```text
/// gap[i]  = (open[i] - close[i-1]) / close[i-1] × 100
/// output  = mean(gap[t-period+1 .. t])
/// ```
///
/// - **Positive**: on average, bars open above the prior close (overnight upward drift).
/// - **Negative**: on average, bars open below the prior close (overnight downward drift).
/// - **Near zero**: no persistent gap bias.
///
/// Useful for detecting systematic overnight drift, news-driven gap behavior, or
/// session-open bias in intraday data.
///
/// Returns [`SignalValue::Unavailable`] until `period` gaps are collected (needs
/// `period + 1` bars).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::OpenCloseGap;
/// use fin_primitives::signals::Signal;
/// let ocg = OpenCloseGap::new("ocg_10", 10).unwrap();
/// assert_eq!(ocg.period(), 10);
/// ```
pub struct OpenCloseGap {
    name: String,
    period: usize,
    gaps: VecDeque<Decimal>,
    prev_close: Option<Decimal>,
}

impl OpenCloseGap {
    /// Constructs a new `OpenCloseGap`.
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
            gaps: VecDeque::with_capacity(period),
            prev_close: None,
        })
    }
}

impl Signal for OpenCloseGap {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.gaps.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            if !pc.is_zero() {
                let gap = (bar.open - pc)
                    .checked_div(pc)
                    .ok_or(FinError::ArithmeticOverflow)?
                    * Decimal::ONE_HUNDRED;
                self.gaps.push_back(gap);
                if self.gaps.len() > self.period {
                    self.gaps.pop_front();
                }
            }
        }
        self.prev_close = Some(bar.close);

        if self.gaps.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sum: Decimal = self.gaps.iter().copied().sum();
        let mean = sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(mean))
    }

    fn reset(&mut self) {
        self.gaps.clear();
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

    fn bar_oc(o: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        let hi = op.value().max(cp.value());
        let lo = op.value().min(cp.value());
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op,
            high: Price::new(hi).unwrap(),
            low: Price::new(lo).unwrap(),
            close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_ocg_invalid_period() {
        assert!(OpenCloseGap::new("ocg", 0).is_err());
    }

    #[test]
    fn test_ocg_unavailable_during_warmup() {
        let mut ocg = OpenCloseGap::new("ocg", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(ocg.update_bar(&bar_oc("100", "101")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!ocg.is_ready());
    }

    #[test]
    fn test_ocg_zero_gap_flat() {
        // Open always equals prev close → gaps all zero → mean = 0
        let mut ocg = OpenCloseGap::new("ocg", 3).unwrap();
        ocg.update_bar(&bar_oc("100", "100")).unwrap();
        ocg.update_bar(&bar_oc("100", "100")).unwrap();
        ocg.update_bar(&bar_oc("100", "100")).unwrap();
        let v = ocg.update_bar(&bar_oc("100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ocg_positive_gap_bias() {
        // Each bar opens 1% above prior close
        let mut ocg = OpenCloseGap::new("ocg", 3).unwrap();
        // bar 1: close=100
        ocg.update_bar(&bar_oc("100", "100")).unwrap();
        // bar 2: open=101 (1% above 100), close=101
        ocg.update_bar(&bar_oc("101", "101")).unwrap();
        // bar 3: open=102.01 (~1% above 101), close=102.01
        ocg.update_bar(&bar_oc("102.01", "102.01")).unwrap();
        // bar 4: open=103.0301 (~1% above 102.01), close=103.0301
        if let SignalValue::Scalar(v) = ocg.update_bar(&bar_oc("103.0301", "103.0301")).unwrap() {
            assert!(v > dec!(0), "persistent up-gaps → positive mean gap: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ocg_reset() {
        let mut ocg = OpenCloseGap::new("ocg", 2).unwrap();
        ocg.update_bar(&bar_oc("100", "100")).unwrap();
        ocg.update_bar(&bar_oc("100", "100")).unwrap();
        ocg.update_bar(&bar_oc("100", "100")).unwrap();
        assert!(ocg.is_ready());
        ocg.reset();
        assert!(!ocg.is_ready());
    }
}
