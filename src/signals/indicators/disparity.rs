//! Disparity Index indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Disparity Index — percentage deviation of the current close from its SMA.
///
/// ```text
/// disparity = (close - SMA(close, period)) / SMA(close, period) × 100
/// ```
///
/// Positive values mean price is above its moving average; negative means below.
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::DisparityIndex;
/// use fin_primitives::signals::Signal;
///
/// let d = DisparityIndex::new("disp", 10).unwrap();
/// assert_eq!(d.period(), 10);
/// ```
pub struct DisparityIndex {
    name: String,
    period: usize,
    history: VecDeque<Decimal>,
}

impl DisparityIndex {
    /// Creates a new `DisparityIndex`.
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
            history: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for DisparityIndex {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.history.push_back(bar.close);
        if self.history.len() > self.period {
            self.history.pop_front();
        }
        if self.history.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sma = self.history.iter().sum::<Decimal>() / Decimal::from(self.period as u32);
        if sma.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let disparity = (bar.close - sma)
            .checked_div(sma)
            .ok_or(FinError::ArithmeticOverflow)?
            * Decimal::from(100u32);

        Ok(SignalValue::Scalar(disparity))
    }

    fn is_ready(&self) -> bool {
        self.history.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.history.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(c: &str) -> OhlcvBar {
        let p = Price::new(c.parse().unwrap()).unwrap();
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
    fn test_disparity_invalid_period() {
        assert!(DisparityIndex::new("d", 0).is_err());
    }

    #[test]
    fn test_disparity_unavailable_before_period() {
        let mut d = DisparityIndex::new("d", 3).unwrap();
        assert_eq!(d.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(d.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_disparity_flat_is_zero() {
        let mut d = DisparityIndex::new("d", 3).unwrap();
        for _ in 0..3 { d.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = d.update_bar(&bar("100")).unwrap() {
            assert_eq!(v, dec!(0));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_disparity_price_above_sma_positive() {
        let mut d = DisparityIndex::new("d", 3).unwrap();
        d.update_bar(&bar("90")).unwrap();
        d.update_bar(&bar("100")).unwrap();
        d.update_bar(&bar("110")).unwrap(); // SMA = 100, close = 110
        if let SignalValue::Scalar(v) = d.update_bar(&bar("120")).unwrap() {
            // SMA of [100,110,120]=110, close=120 => (120-110)/110*100
            assert!(v > dec!(0), "above SMA should be positive: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_disparity_price_below_sma_negative() {
        let mut d = DisparityIndex::new("d", 3).unwrap();
        d.update_bar(&bar("110")).unwrap();
        d.update_bar(&bar("100")).unwrap();
        d.update_bar(&bar("90")).unwrap(); // SMA ~= 100
        if let SignalValue::Scalar(v) = d.update_bar(&bar("80")).unwrap() {
            // SMA of [100,90,80]=90, close=80 => negative
            assert!(v < dec!(0), "below SMA should be negative: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_disparity_reset() {
        let mut d = DisparityIndex::new("d", 3).unwrap();
        for _ in 0..3 { d.update_bar(&bar("100")).unwrap(); }
        assert!(d.is_ready());
        d.reset();
        assert!(!d.is_ready());
        assert_eq!(d.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }
}
