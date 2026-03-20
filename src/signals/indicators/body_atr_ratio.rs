//! Body ATR Ratio — bar body size normalized by rolling average true range.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Body ATR Ratio — `|close - open| / ATR(period)`.
///
/// Normalizes the current bar's body size by the N-period average true range,
/// measuring whether this bar's directional move is large or small relative
/// to recent typical volatility:
/// - **> 1.0**: body is larger than the average true range — strong, decisive bar.
/// - **~1.0**: body is in line with typical volatility.
/// - **< 1.0**: small body relative to recent volatility — indecision or compression.
///
/// Uses a simple rolling average of true ranges for ATR.
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen
/// or if ATR is zero.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BodyAtrRatio;
/// use fin_primitives::signals::Signal;
/// let bar_atr = BodyAtrRatio::new("bar_atr_14", 14).unwrap();
/// assert_eq!(bar_atr.period(), 14);
/// ```
pub struct BodyAtrRatio {
    name: String,
    period: usize,
    tr_window: VecDeque<Decimal>,
    tr_sum: Decimal,
    prev_close: Option<Decimal>,
}

impl BodyAtrRatio {
    /// Constructs a new `BodyAtrRatio`.
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
            tr_window: VecDeque::with_capacity(period),
            tr_sum: Decimal::ZERO,
            prev_close: None,
        })
    }
}

impl Signal for BodyAtrRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.tr_window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = bar.true_range(self.prev_close);
        self.prev_close = Some(bar.close);

        self.tr_sum += tr;
        self.tr_window.push_back(tr);
        if self.tr_window.len() > self.period {
            let removed = self.tr_window.pop_front().unwrap();
            self.tr_sum -= removed;
        }

        if self.tr_window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let atr = self.tr_sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        if atr.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let body = bar.body_size();
        let ratio = body
            .checked_div(atr)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(ratio))
    }

    fn reset(&mut self) {
        self.tr_window.clear();
        self.tr_sum = Decimal::ZERO;
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

    fn bar(o: &str, h: &str, l: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_bar_atr_invalid_period() {
        assert!(BodyAtrRatio::new("bar_atr", 0).is_err());
    }

    #[test]
    fn test_bar_atr_unavailable_during_warmup() {
        let mut s = BodyAtrRatio::new("bar_atr", 3).unwrap();
        assert_eq!(s.update_bar(&bar("100","110","90","105")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("105","115","95","108")).unwrap(), SignalValue::Unavailable);
        assert!(!s.is_ready());
    }

    #[test]
    fn test_bar_atr_non_negative() {
        let mut s = BodyAtrRatio::new("bar_atr", 3).unwrap();
        let bars = [("100","110","90","105"),("105","115","95","108"),("108","118","98","102"),
                    ("102","112","92","110")];
        for (o, h, l, c) in &bars {
            if let SignalValue::Scalar(v) = s.update_bar(&bar(o, h, l, c)).unwrap() {
                assert!(v >= dec!(0), "body/ATR ratio must be non-negative: {v}");
            }
        }
    }

    #[test]
    fn test_bar_atr_large_body_gives_high_ratio() {
        // Bars with range ~10, then a bar with body=20 → ratio > 1
        let mut s = BodyAtrRatio::new("bar_atr", 2).unwrap();
        s.update_bar(&bar("100","105","95","102")).unwrap();   // TR=10, body=2
        s.update_bar(&bar("102","107","97","104")).unwrap();   // TR=10, body=2
        // Now large body bar
        if let SignalValue::Scalar(v) = s.update_bar(&bar("100","120","98","120")).unwrap() {
            // body=20, ATR≈10 → ratio≈2
            assert!(v > dec!(1), "large body relative to ATR: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_bar_atr_doji_gives_low_ratio() {
        // Normal bars, then a doji (body≈0) → ratio near 0
        let mut s = BodyAtrRatio::new("bar_atr", 2).unwrap();
        s.update_bar(&bar("100","110","90","105")).unwrap();   // TR=20, body=5
        s.update_bar(&bar("105","115","95","110")).unwrap();   // TR=20, body=5
        if let SignalValue::Scalar(v) = s.update_bar(&bar("105","115","95","105")).unwrap() {
            // doji: body=0 → ratio=0
            assert!(v < dec!(0.1), "doji gives near-zero body/ATR: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_bar_atr_reset() {
        let mut s = BodyAtrRatio::new("bar_atr", 2).unwrap();
        for (o, h, l, c) in &[("100","110","90","105"),("105","115","95","110"),
                                ("110","120","100","115")] {
            s.update_bar(&bar(o, h, l, c)).unwrap();
        }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
