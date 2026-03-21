//! Close Above Pivot indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Close Above Pivot — rolling fraction of bars where `close > pivot_price`, where
/// the pivot is the rolling average of `(high + low + close) / 3` (typical price)
/// over the prior `period` bars.
///
/// ```text
/// pivot[t]   = mean(typical_price[t-period .. t-1])
/// output[t]  = count(close[i] > pivot[t], i in [t-period+1, t]) / period × 100
/// ```
///
/// - **100**: all closes in the window are above the rolling pivot (strong bullish zone).
/// - **0**: all closes are below the rolling pivot (strong bearish zone).
/// - **50**: price is oscillating around the pivot.
///
/// Returns [`SignalValue::Unavailable`] until `2 × period` bars have been seen
/// (need `period` typical prices to form the pivot, then `period` close vs pivot checks).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseAbovePivot;
/// use fin_primitives::signals::Signal;
/// let cap = CloseAbovePivot::new("cap_10", 10).unwrap();
/// assert_eq!(cap.period(), 10);
/// ```
pub struct CloseAbovePivot {
    name: String,
    period: usize,
    typical_prices: VecDeque<Decimal>,
    above_flags: VecDeque<bool>,
}

impl CloseAbovePivot {
    /// Constructs a new `CloseAbovePivot`.
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
            typical_prices: VecDeque::with_capacity(period),
            above_flags: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for CloseAbovePivot {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.above_flags.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tp = bar.typical_price();

        // Compute pivot from prior typical prices
        if self.typical_prices.len() >= self.period {
            let pivot_sum: Decimal = self.typical_prices.iter().copied().sum();
            let pivot = pivot_sum
                .checked_div(Decimal::from(self.period as u32))
                .ok_or(FinError::ArithmeticOverflow)?;
            let above = bar.close > pivot;
            self.above_flags.push_back(above);
            if self.above_flags.len() > self.period {
                self.above_flags.pop_front();
            }
        }

        self.typical_prices.push_back(tp);
        if self.typical_prices.len() > self.period {
            self.typical_prices.pop_front();
        }

        if self.above_flags.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let count = self.above_flags.iter().filter(|&&f| f).count();
        let pct = Decimal::from(count as u32)
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?
            * Decimal::ONE_HUNDRED;

        Ok(SignalValue::Scalar(pct))
    }

    fn reset(&mut self) {
        self.typical_prices.clear();
        self.above_flags.clear();
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
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    fn close_bar(c: &str) -> OhlcvBar {
        bar(c, c, c)
    }

    #[test]
    fn test_cap_invalid_period() {
        assert!(CloseAbovePivot::new("cap", 0).is_err());
    }

    #[test]
    fn test_cap_unavailable_during_warmup() {
        let mut cap = CloseAbovePivot::new("cap", 3).unwrap();
        for _ in 0..5 {
            assert_eq!(cap.update_bar(&close_bar("100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!cap.is_ready());
    }

    #[test]
    fn test_cap_all_above_100() {
        // Pivot ≈ 100 (flat), then close at 200 always above → 100%
        let mut cap = CloseAbovePivot::new("cap", 3).unwrap();
        // Build pivot with 3 bars at 100
        for _ in 0..6 {
            cap.update_bar(&close_bar("100")).unwrap();
        }
        // Now push high-close bars
        cap.update_bar(&bar("200", "190", "195")).unwrap();
        cap.update_bar(&bar("200", "190", "195")).unwrap();
        if let SignalValue::Scalar(v) = cap.update_bar(&bar("200", "190", "195")).unwrap() {
            assert!(v > dec!(50), "high closes → majority above pivot: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_cap_reset() {
        let mut cap = CloseAbovePivot::new("cap", 3).unwrap();
        for _ in 0..8 { cap.update_bar(&close_bar("100")).unwrap(); }
        assert!(cap.is_ready());
        cap.reset();
        assert!(!cap.is_ready());
    }
}
