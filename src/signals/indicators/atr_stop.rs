//! ATR Stop indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// ATR Stop — trailing stop level based on ATR.
///
/// ```text
/// ATR_t     = mean(TR, period)
/// stop_long = close_t − multiplier × ATR_t    (stop for long positions)
/// stop_short= close_t + multiplier × ATR_t    (stop for short positions)
/// output    = (close_t − stop_long) / ATR_t   (position in ATR units above stop)
/// ```
///
/// Use `stop_long()` and `stop_short()` for the raw stop levels.
/// Output positive means price is above the long stop; near zero = near the stop.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::AtrStop;
/// use fin_primitives::signals::Signal;
///
/// let s = AtrStop::new("atr_stop", 14, "2.0".parse().unwrap()).unwrap();
/// assert_eq!(s.period(), 14);
/// ```
pub struct AtrStop {
    name: String,
    period: usize,
    multiplier: Decimal,
    trs: VecDeque<Decimal>,
    prev_close: Option<Decimal>,
    stop_long: Option<Decimal>,
    stop_short: Option<Decimal>,
}

impl AtrStop {
    /// Creates a new `AtrStop`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    /// Returns [`FinError::InvalidInput`] if `multiplier` is not positive.
    pub fn new(
        name: impl Into<String>,
        period: usize,
        multiplier: Decimal,
    ) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        if multiplier <= Decimal::ZERO {
            return Err(FinError::InvalidInput("multiplier must be positive".into()));
        }
        Ok(Self {
            name: name.into(),
            period,
            multiplier,
            trs: VecDeque::with_capacity(period),
            prev_close: None,
            stop_long: None,
            stop_short: None,
        })
    }

    /// Returns the current long stop level (close - N×ATR).
    pub fn stop_long(&self) -> Option<Decimal> { self.stop_long }
    /// Returns the current short stop level (close + N×ATR).
    pub fn stop_short(&self) -> Option<Decimal> { self.stop_short }
}

impl Signal for AtrStop {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = bar.true_range(self.prev_close);
        self.prev_close = Some(bar.close);

        self.trs.push_back(tr);
        if self.trs.len() > self.period { self.trs.pop_front(); }
        if self.trs.len() < self.period { return Ok(SignalValue::Unavailable); }

        let atr = self.trs.iter().sum::<Decimal>() / Decimal::from(self.period as u32);
        let band = self.multiplier * atr;

        self.stop_long = Some(bar.close - band);
        self.stop_short = Some(bar.close + band);

        if atr.is_zero() {
            return Ok(SignalValue::Scalar(self.multiplier));
        }
        Ok(SignalValue::Scalar(band / atr))
    }

    fn is_ready(&self) -> bool { self.stop_long.is_some() }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.trs.clear();
        self.prev_close = None;
        self.stop_long = None;
        self.stop_short = None;
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

    #[test]
    fn test_atr_stop_invalid() {
        assert!(AtrStop::new("s", 0, dec!(2)).is_err());
        assert!(AtrStop::new("s", 14, dec!(0)).is_err());
        assert!(AtrStop::new("s", 14, dec!(-1)).is_err());
    }

    #[test]
    fn test_atr_stop_unavailable_before_warmup() {
        let mut s = AtrStop::new("s", 3, dec!(2)).unwrap();
        for _ in 0..2 {
            assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_atr_stop_stops_set() {
        let mut s = AtrStop::new("s", 3, dec!(2)).unwrap();
        for _ in 0..3 { s.update_bar(&bar_hlc("105", "95", "100")).unwrap(); }
        assert!(s.stop_long().is_some());
        assert!(s.stop_short().is_some());
        // stop_long < close < stop_short
        assert!(s.stop_long().unwrap() < dec!(100));
        assert!(s.stop_short().unwrap() > dec!(100));
    }

    #[test]
    fn test_atr_stop_flat_returns_multiplier() {
        // Flat → ATR=0 → returns multiplier
        let mut s = AtrStop::new("s", 3, dec!(2)).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = s.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(2));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_atr_stop_reset() {
        let mut s = AtrStop::new("s", 3, dec!(2)).unwrap();
        for _ in 0..5 { s.update_bar(&bar("100")).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
        assert!(s.stop_long().is_none());
        assert!(s.stop_short().is_none());
    }
}
