//! ATR Ratio indicator -- current ATR as a multiple of its rolling average.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// ATR Ratio -- current True Range (1-bar ATR) divided by the SMA of True Range
/// over `period` bars.
///
/// ```text
/// tr[t]        = max(high-low, |high-prev_close|, |low-prev_close|)
/// atr_sma[t]   = SMA(tr, period)
/// atr_ratio[t] = tr[t] / atr_sma[t]
/// ```
///
/// A ratio > 1 means the current bar's range is above average (elevated volatility).
/// A ratio < 1 means below-average volatility (compression).
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen
/// (needs prev_close and a full ATR window).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::AtrRatio;
/// use fin_primitives::signals::Signal;
/// let ar = AtrRatio::new("ar", 14).unwrap();
/// assert_eq!(ar.period(), 14);
/// ```
pub struct AtrRatio {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    tr_window: VecDeque<Decimal>,
    tr_sum: Decimal,
}

impl AtrRatio {
    /// Constructs a new `AtrRatio`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            tr_window: VecDeque::with_capacity(period),
            tr_sum: Decimal::ZERO,
        })
    }
}

impl Signal for AtrRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.tr_window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = bar.true_range(self.prev_close);
        self.prev_close = Some(bar.close);

        self.tr_window.push_back(tr);
        self.tr_sum += tr;
        if self.tr_window.len() > self.period {
            if let Some(old) = self.tr_window.pop_front() { self.tr_sum -= old; }
        }
        if self.tr_window.len() < self.period { return Ok(SignalValue::Unavailable); }

        #[allow(clippy::cast_possible_truncation)]
        let atr_sma = self.tr_sum / Decimal::from(self.period as u32);
        if atr_sma.is_zero() { return Ok(SignalValue::Unavailable); }
        Ok(SignalValue::Scalar(tr / atr_sma))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.tr_window.clear();
        self.tr_sum = Decimal::ZERO;
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
    fn test_ar_period_0_error() { assert!(AtrRatio::new("ar", 0).is_err()); }

    #[test]
    fn test_ar_unavailable_before_period() {
        let mut ar = AtrRatio::new("ar", 3).unwrap();
        assert_eq!(ar.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_ar_constant_range_is_one() {
        // All bars have same range -> current TR == avg TR -> ratio = 1
        let mut ar = AtrRatio::new("ar", 3).unwrap();
        for _ in 0..5 {
            ar.update_bar(&bar("110", "90", "100")).unwrap();
        }
        if let SignalValue::Scalar(v) = ar.update_bar(&bar("110", "90", "100")).unwrap() {
            let diff = (v - dec!(1)).abs();
            assert!(diff < dec!(0.001), "expected ~1, got {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ar_high_volatility_above_one() {
        let mut ar = AtrRatio::new("ar", 3).unwrap();
        // seed with small ranges
        for _ in 0..3 { ar.update_bar(&bar("101", "99", "100")).unwrap(); }
        // then spike
        let v = ar.update_bar(&bar("120", "80", "100")).unwrap();
        if let SignalValue::Scalar(ratio) = v {
            assert!(ratio > dec!(1), "expected ratio > 1 for spike bar, got {ratio}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ar_reset() {
        let mut ar = AtrRatio::new("ar", 3).unwrap();
        for _ in 0..5 { ar.update_bar(&bar("110", "90", "100")).unwrap(); }
        assert!(ar.is_ready());
        ar.reset();
        assert!(!ar.is_ready());
    }
}
