//! Price-to-SMA Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price-to-SMA Ratio -- how far price has extended above or below its N-period SMA.
///
/// ```text
/// sma[t]   = SMA(close, period)
/// ratio[t] = close[t] / sma[t]
/// ```
///
/// A ratio above 1.0 means price is above its moving average; below 1.0 means below.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have accumulated or if SMA is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceToSmaRatio;
/// use fin_primitives::signals::Signal;
/// let p = PriceToSmaRatio::new("ptsr", 20).unwrap();
/// assert_eq!(p.period(), 20);
/// ```
pub struct PriceToSmaRatio {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl PriceToSmaRatio {
    /// Constructs a new `PriceToSmaRatio`.
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

impl Signal for PriceToSmaRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.close);
        self.sum += bar.close;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() { self.sum -= old; }
        }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        #[allow(clippy::cast_possible_truncation)]
        let sma = self.sum / Decimal::from(self.period as u32);
        if sma.is_zero() { return Ok(SignalValue::Unavailable); }
        Ok(SignalValue::Scalar(bar.close / sma))
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
    fn test_ptsr_period_0_error() { assert!(PriceToSmaRatio::new("p", 0).is_err()); }

    #[test]
    fn test_ptsr_unavailable_before_period() {
        let mut p = PriceToSmaRatio::new("p", 3).unwrap();
        assert_eq!(p.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_ptsr_at_sma_is_one() {
        let mut p = PriceToSmaRatio::new("p", 3).unwrap();
        // constant price => close == SMA => ratio = 1
        p.update_bar(&bar("100")).unwrap();
        p.update_bar(&bar("100")).unwrap();
        let v = p.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_ptsr_above_sma() {
        // SMA(10,10,20) = 40/3, close=20 => ratio = 20/(40/3) = 1.5
        let mut p = PriceToSmaRatio::new("p", 3).unwrap();
        p.update_bar(&bar("10")).unwrap();
        p.update_bar(&bar("10")).unwrap();
        let v = p.update_bar(&bar("20")).unwrap();
        if let SignalValue::Scalar(ratio) = v {
            assert!(ratio > dec!(1), "expected ratio > 1, got {ratio}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ptsr_reset() {
        let mut p = PriceToSmaRatio::new("p", 2).unwrap();
        p.update_bar(&bar("100")).unwrap();
        p.update_bar(&bar("100")).unwrap();
        assert!(p.is_ready());
        p.reset();
        assert!(!p.is_ready());
    }
}
