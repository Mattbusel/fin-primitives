//! Price Reversal Strength indicator -- magnitude of closing reversals from extreme.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Reversal Strength -- measures how strongly price reversed from the intrabar
/// extreme back toward the close.
///
/// For a bar that reached a high extreme:
/// ```text
/// bull_reversal[t] = (close - low) / (high - low) * 100  (from low extreme)
/// ```
///
/// This rolling average indicates how consistently bars recover from their intraday
/// lows. High values signal strong buying at dips; low values signal weak recoveries.
///
/// Returns [`SignalValue::Unavailable`] until `period` valid (non-zero-range) bars
/// have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceReversalStrength;
/// use fin_primitives::signals::Signal;
/// let prs = PriceReversalStrength::new("prs", 14).unwrap();
/// assert_eq!(prs.period(), 14);
/// ```
pub struct PriceReversalStrength {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl PriceReversalStrength {
    /// Constructs a new `PriceReversalStrength`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for PriceReversalStrength {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        if range.is_zero() { return Ok(SignalValue::Unavailable); }
        let strength = (bar.close - bar.low) / range * Decimal::ONE_HUNDRED;
        self.window.push_back(strength);
        self.sum += strength;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() { self.sum -= old; }
        }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        #[allow(clippy::cast_possible_truncation)]
        Ok(SignalValue::Scalar(self.sum / Decimal::from(self.period as u32)))
    }

    fn reset(&mut self) {
        self.window.clear();
        self.sum = Decimal::ZERO;
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
            open: cp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_prs_period_0_error() { assert!(PriceReversalStrength::new("prs", 0).is_err()); }

    #[test]
    fn test_prs_zero_range_unavailable() {
        let mut prs = PriceReversalStrength::new("prs", 1).unwrap();
        let v = prs.update_bar(&bar("100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_prs_close_at_high_is_100() {
        // close == high -> (high-low)/(high-low)*100 = 100
        let mut prs = PriceReversalStrength::new("prs", 1).unwrap();
        let v = prs.update_bar(&bar("110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_prs_close_at_low_is_0() {
        let mut prs = PriceReversalStrength::new("prs", 1).unwrap();
        let v = prs.update_bar(&bar("110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_prs_rolling_average() {
        let mut prs = PriceReversalStrength::new("prs", 2).unwrap();
        prs.update_bar(&bar("110", "90", "110")).unwrap(); // 100%
        let v = prs.update_bar(&bar("110", "90", "90")).unwrap(); // 0% -> avg=50
        assert_eq!(v, SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_prs_reset() {
        let mut prs = PriceReversalStrength::new("prs", 2).unwrap();
        prs.update_bar(&bar("110", "90", "100")).unwrap();
        prs.update_bar(&bar("110", "90", "100")).unwrap();
        assert!(prs.is_ready());
        prs.reset();
        assert!(!prs.is_ready());
    }
}
