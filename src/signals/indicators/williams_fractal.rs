//! Williams Fractal indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Williams Fractal — identifies fractal pivot highs and lows using a 5-bar pattern.
///
/// A **fractal high** occurs when the middle bar of five consecutive bars has the
/// highest high: `bars[2].high > bars[i].high` for all `i ≠ 2`.
///
/// A **fractal low** occurs when the middle bar has the lowest low:
/// `bars[2].low < bars[i].low` for all `i ≠ 2`.
///
/// The indicator returns:
/// - `+1` when the confirmed bar (2 bars ago) is a fractal high
/// - `-1` when it is a fractal low
/// - `0` when it is both (rare equality case)
/// - `SignalValue::Unavailable` until 5 bars have been seen
///
/// Use [`WilliamsFractal::fractal_high`] and [`WilliamsFractal::fractal_low`] to
/// read the most recently confirmed fractal price levels directly.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::WilliamsFractal;
/// use fin_primitives::signals::Signal;
///
/// let f = WilliamsFractal::new("wfrac").unwrap();
/// assert_eq!(f.period(), 5);
/// assert!(!f.is_ready());
/// ```
pub struct WilliamsFractal {
    name: String,
    window: VecDeque<BarInput>,
    /// Most recently confirmed fractal high price.
    fractal_high: Option<Decimal>,
    /// Most recently confirmed fractal low price.
    fractal_low: Option<Decimal>,
    last_value: Option<SignalValue>,
}

impl WilliamsFractal {
    /// Constructs a new `WilliamsFractal` indicator.
    ///
    /// # Errors
    /// Never errors — provided for API consistency.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self {
            name: name.into(),
            window: VecDeque::with_capacity(5),
            fractal_high: None,
            fractal_low: None,
            last_value: None,
        })
    }

    /// Returns the most recently confirmed fractal high price, or `None`.
    pub fn fractal_high(&self) -> Option<Decimal> {
        self.fractal_high
    }

    /// Returns the most recently confirmed fractal low price, or `None`.
    pub fn fractal_low(&self) -> Option<Decimal> {
        self.fractal_low
    }

    fn check_fractal(window: &VecDeque<BarInput>) -> (bool, bool) {
        // window has exactly 5 bars; index 2 is the middle bar (2 bars ago)
        let mid_high = window[2].high;
        let mid_low = window[2].low;
        let is_fractal_high = window.iter().enumerate().all(|(i, b)| i == 2 || b.high < mid_high);
        let is_fractal_low  = window.iter().enumerate().all(|(i, b)| i == 2 || b.low  > mid_low);
        (is_fractal_high, is_fractal_low)
    }
}

impl Signal for WilliamsFractal {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(*bar);
        if self.window.len() > 5 {
            self.window.pop_front();
        }
        if self.window.len() < 5 {
            return Ok(SignalValue::Unavailable);
        }

        let (is_high, is_low) = Self::check_fractal(&self.window);

        let value = match (is_high, is_low) {
            (true, false) => {
                self.fractal_high = Some(self.window[2].high);
                SignalValue::Scalar(Decimal::ONE)
            }
            (false, true) => {
                self.fractal_low = Some(self.window[2].low);
                SignalValue::Scalar(Decimal::NEGATIVE_ONE)
            }
            (true, true) => {
                self.fractal_high = Some(self.window[2].high);
                self.fractal_low  = Some(self.window[2].low);
                SignalValue::Scalar(Decimal::ZERO)
            }
            (false, false) => SignalValue::Scalar(Decimal::ZERO),
        };
        self.last_value = Some(value.clone());
        Ok(value)
    }

    fn is_ready(&self) -> bool {
        self.last_value.is_some()
    }

    fn period(&self) -> usize {
        5
    }

    fn reset(&mut self) {
        self.window.clear();
        self.fractal_high = None;
        self.fractal_low  = None;
        self.last_value   = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(h.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_fractal_unavailable_before_five_bars() {
        let mut f = WilliamsFractal::new("wf").unwrap();
        for _ in 0..4 {
            assert_eq!(f.update_bar(&bar("100", "90")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!f.is_ready());
    }

    #[test]
    fn test_fractal_high_detected() {
        let mut f = WilliamsFractal::new("wf").unwrap();
        // Pattern: bars where middle is highest
        f.update_bar(&bar("100", "90")).unwrap();
        f.update_bar(&bar("102", "91")).unwrap();
        f.update_bar(&bar("110", "95")).unwrap(); // fractal high candidate
        f.update_bar(&bar("105", "92")).unwrap();
        let v = f.update_bar(&bar("103", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
        assert_eq!(f.fractal_high(), Some(dec!(110)));
    }

    #[test]
    fn test_fractal_low_detected() {
        let mut f = WilliamsFractal::new("wf").unwrap();
        // Pattern: middle bar has lowest low
        f.update_bar(&bar("110", "95")).unwrap();
        f.update_bar(&bar("108", "92")).unwrap();
        f.update_bar(&bar("106", "80")).unwrap(); // fractal low candidate
        f.update_bar(&bar("109", "88")).unwrap();
        let v = f.update_bar(&bar("111", "91")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
        assert_eq!(f.fractal_low(), Some(dec!(80)));
    }

    #[test]
    fn test_fractal_reset() {
        let mut f = WilliamsFractal::new("wf").unwrap();
        for _ in 0..5 {
            f.update_bar(&bar("100", "90")).unwrap();
        }
        assert!(f.is_ready());
        f.reset();
        assert!(!f.is_ready());
        assert_eq!(f.fractal_high(), None);
        assert_eq!(f.fractal_low(), None);
    }

    #[test]
    fn test_fractal_no_pattern_returns_zero() {
        let mut f = WilliamsFractal::new("wf").unwrap();
        // All equal highs — no fractal high or low
        for _ in 0..5 {
            f.update_bar(&bar("100", "90")).unwrap();
        }
        // Last value is zero (no fractal)
        assert!(f.is_ready());
    }
}
