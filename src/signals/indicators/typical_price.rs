//! Typical Price Moving Average indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Typical Price MA — simple moving average of (high + low + close) / 3.
///
/// ```text
/// typical_i = (high_i + low_i + close_i) / 3
/// output    = mean(typical, period)
/// ```
///
/// The typical price is the basis of CCI, MFI, and Vortex calculations.
/// This indicator provides its moving average as a standalone signal.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::TypicalPriceMa;
/// use fin_primitives::signals::Signal;
///
/// let tp = TypicalPriceMa::new("tp", 14).unwrap();
/// assert_eq!(tp.period(), 14);
/// ```
pub struct TypicalPriceMa {
    name: String,
    period: usize,
    typicals: VecDeque<Decimal>,
}

impl TypicalPriceMa {
    /// Creates a new `TypicalPriceMa`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            typicals: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for TypicalPriceMa {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tp = (bar.high + bar.low + bar.close) / Decimal::from(3u32);
        self.typicals.push_back(tp);
        if self.typicals.len() > self.period { self.typicals.pop_front(); }
        if self.typicals.len() < self.period { return Ok(SignalValue::Unavailable); }

        let sma = self.typicals.iter().sum::<Decimal>() / Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(sma))
    }

    fn is_ready(&self) -> bool { self.typicals.len() >= self.period }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.typicals.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar_hlc(h: &str, l: &str, c: &str) -> OhlcvBar {
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

    fn bar(c: &str) -> OhlcvBar { bar_hlc(c, c, c) }

    #[test]
    fn test_tp_invalid() {
        assert!(TypicalPriceMa::new("t", 0).is_err());
    }

    #[test]
    fn test_tp_unavailable_before_warmup() {
        let mut t = TypicalPriceMa::new("t", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(t.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_tp_flat_equals_price() {
        // When H=L=C=100, typical=100, MA=100
        let mut t = TypicalPriceMa::new("t", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = t.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(100));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_tp_typical_price_computed() {
        // h=110, l=90, c=100 → typical = (110+90+100)/3 = 100
        // period=1, so MA = typical
        let mut t = TypicalPriceMa::new("t", 1).unwrap();
        if let SignalValue::Scalar(v) = t.update_bar(&bar_hlc("110", "90", "100")).unwrap() {
            assert_eq!(v, dec!(100));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_tp_rolling_average() {
        // Two bars: bar1 tp=(110+90+100)/3=100, bar2 tp=(120+80+100)/3=100 → MA=100
        let mut t = TypicalPriceMa::new("t", 2).unwrap();
        t.update_bar(&bar_hlc("110", "90", "100")).unwrap();
        if let SignalValue::Scalar(v) = t.update_bar(&bar_hlc("120", "80", "100")).unwrap() {
            assert_eq!(v, dec!(100));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_tp_reset() {
        let mut t = TypicalPriceMa::new("t", 3).unwrap();
        for _ in 0..5 { t.update_bar(&bar("100")).unwrap(); }
        assert!(t.is_ready());
        t.reset();
        assert!(!t.is_ready());
    }
}
