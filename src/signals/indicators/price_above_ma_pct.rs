//! Price Above MA Percentage — rolling fraction of bars where close exceeds the SMA.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Above MA Percentage — fraction of bars in a window where `close > SMA(period)`.
///
/// At each bar, computes the SMA of the window and counts how many closes exceed it,
/// returning a value in `[0, 1]`:
/// - **Near 1.0**: price consistently above its average — sustained uptrend.
/// - **= 0.5**: price crosses its average frequently — balanced / range-bound.
/// - **Near 0.0**: price consistently below its average — sustained downtrend.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period < 2`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceAboveMaPct;
/// use fin_primitives::signals::Signal;
/// let p = PriceAboveMaPct::new("pma_pct_20", 20).unwrap();
/// assert_eq!(p.period(), 20);
/// ```
pub struct PriceAboveMaPct {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
}

impl PriceAboveMaPct {
    /// Constructs a new `PriceAboveMaPct`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            window: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for PriceAboveMaPct {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.close);
        if self.window.len() > self.period {
            self.window.pop_front();
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = Decimal::from(self.period as u32);
        let sum: Decimal = self.window.iter().sum();
        let sma = sum.checked_div(n).ok_or(FinError::ArithmeticOverflow)?;

        let above = self.window.iter().filter(|&&c| c > sma).count();
        let frac = Decimal::from(above as u32)
            .checked_div(n)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(frac))
    }

    fn reset(&mut self) {
        self.window.clear();
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
    fn test_pamp_invalid_period() {
        assert!(PriceAboveMaPct::new("p", 0).is_err());
        assert!(PriceAboveMaPct::new("p", 1).is_err());
    }

    #[test]
    fn test_pamp_unavailable_before_period() {
        let mut s = PriceAboveMaPct::new("p", 4).unwrap();
        for _ in 0..3 {
            assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!s.is_ready());
    }

    #[test]
    fn test_pamp_flat_prices_give_zero() {
        // All equal prices → none above SMA → 0
        let mut s = PriceAboveMaPct::new("p", 4).unwrap();
        for _ in 0..4 { s.update_bar(&bar("100")).unwrap(); }
        assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_pamp_uptrend_gives_high_fraction() {
        // Rising prices: later half above SMA
        let mut s = PriceAboveMaPct::new("p", 4).unwrap();
        s.update_bar(&bar("100")).unwrap();
        s.update_bar(&bar("101")).unwrap();
        s.update_bar(&bar("102")).unwrap();
        if let SignalValue::Scalar(v) = s.update_bar(&bar("103")).unwrap() {
            assert!(v > dec!(0.4), "uptrend should give majority above SMA: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_pamp_in_range_zero_to_one() {
        let mut s = PriceAboveMaPct::new("p", 4).unwrap();
        let prices = ["95", "105", "95", "105", "95", "105"];
        for p in &prices {
            if let SignalValue::Scalar(v) = s.update_bar(&bar(p)).unwrap() {
                assert!(v >= dec!(0) && v <= dec!(1), "value out of [0,1]: {v}");
            }
        }
    }

    #[test]
    fn test_pamp_reset() {
        let mut s = PriceAboveMaPct::new("p", 3).unwrap();
        for p in &["100","101","102"] { s.update_bar(&bar(p)).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
