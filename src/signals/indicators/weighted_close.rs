//! Weighted Close indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Weighted Close MA — moving average of (high + low + 2×close) / 4.
///
/// ```text
/// wc_t   = (high_t + low_t + 2 × close_t) / 4
/// output = mean(wc, period)
/// ```
///
/// Gives double weight to the close versus high and low, making it more
/// sensitive to close prices than a typical price average.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::WeightedClose;
/// use fin_primitives::signals::Signal;
///
/// let wc = WeightedClose::new("wc", 14).unwrap();
/// assert_eq!(wc.period(), 14);
/// ```
pub struct WeightedClose {
    name: String,
    period: usize,
    wcs: VecDeque<Decimal>,
}

impl WeightedClose {
    /// Creates a new `WeightedClose`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            wcs: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for WeightedClose {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let wc = (bar.high + bar.low + Decimal::from(2u32) * bar.close) / Decimal::from(4u32);
        self.wcs.push_back(wc);
        if self.wcs.len() > self.period { self.wcs.pop_front(); }
        if self.wcs.len() < self.period { return Ok(SignalValue::Unavailable); }

        let avg = self.wcs.iter().sum::<Decimal>() / Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(avg))
    }

    fn is_ready(&self) -> bool { self.wcs.len() >= self.period }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.wcs.clear();
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
    fn test_wc_invalid() {
        assert!(WeightedClose::new("w", 0).is_err());
    }

    #[test]
    fn test_wc_unavailable_before_warmup() {
        let mut w = WeightedClose::new("w", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(w.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_wc_flat_equals_price() {
        // H=L=C=100 → wc = (100+100+200)/4 = 100; MA = 100
        let mut w = WeightedClose::new("w", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = w.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(100));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_wc_bullish_close() {
        // High close → wc > typical price (high+low+close)/3
        // h=110, l=90, c=110 → wc=(110+90+220)/4=105; typical=(110+90+110)/3=103.33
        let mut w = WeightedClose::new("w", 1).unwrap();
        if let SignalValue::Scalar(v) = w.update_bar(&bar_hlc("110", "90", "110")).unwrap() {
            assert_eq!(v, dec!(105));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_wc_bearish_close() {
        // Low close → wc < typical price
        // h=110, l=90, c=90 → wc=(110+90+180)/4=95
        let mut w = WeightedClose::new("w", 1).unwrap();
        if let SignalValue::Scalar(v) = w.update_bar(&bar_hlc("110", "90", "90")).unwrap() {
            assert_eq!(v, dec!(95));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_wc_reset() {
        let mut w = WeightedClose::new("w", 3).unwrap();
        for _ in 0..5 { w.update_bar(&bar("100")).unwrap(); }
        assert!(w.is_ready());
        w.reset();
        assert!(!w.is_ready());
    }
}
