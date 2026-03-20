//! Bar Momentum Score indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Bar Momentum Score — the current bar's directional body normalized by the
/// rolling average true range over `period` bars.
///
/// ```text
/// body  = close - open  (positive = bullish, negative = bearish)
/// atr   = SMA(TR, period)
/// score = body / atr
/// ```
///
/// A score near ±1 means the body is about one ATR in size.
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen or ATR is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BarMomentumScore;
/// use fin_primitives::signals::Signal;
///
/// let bms = BarMomentumScore::new("bms", 14).unwrap();
/// assert_eq!(bms.period(), 14);
/// ```
pub struct BarMomentumScore {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    trs: VecDeque<Decimal>,
    sum: Decimal,
}

impl BarMomentumScore {
    /// Constructs a new `BarMomentumScore`.
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
            prev_close: None,
            trs: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for BarMomentumScore {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.trs.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = match self.prev_close {
            None => bar.range(),
            Some(pc) => {
                let hl = bar.range();
                let hpc = (bar.high - pc).abs();
                let lpc = (bar.low - pc).abs();
                hl.max(hpc).max(lpc)
            }
        };
        self.prev_close = Some(bar.close);

        self.trs.push_back(tr);
        self.sum += tr;
        if self.trs.len() > self.period {
            self.sum -= self.trs.pop_front().unwrap();
        }

        if self.trs.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let nd = Decimal::from(self.period as u32);
        let atr = self.sum / nd;
        if atr.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let body = bar.close - bar.open;
        Ok(SignalValue::Scalar(body / atr))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.trs.clear();
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
    fn test_bms_invalid_period() {
        assert!(BarMomentumScore::new("bms", 0).is_err());
    }

    #[test]
    fn test_bms_unavailable_before_warm_up() {
        let mut bms = BarMomentumScore::new("bms", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(bms.update_bar(&bar("100", "110", "90", "105")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_bms_bullish_score_positive() {
        let mut bms = BarMomentumScore::new("bms", 3).unwrap();
        // Feed bars with TR=10 each, close > open
        for _ in 0..3 {
            bms.update_bar(&bar("100", "110", "100", "105")).unwrap();
        }
        // ATR = 10, body = 5 → score = 0.5
        let result = bms.update_bar(&bar("100", "110", "100", "105")).unwrap();
        if let SignalValue::Scalar(v) = result {
            assert!(v > dec!(0), "bullish bar should give positive score");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_bms_bearish_score_negative() {
        let mut bms = BarMomentumScore::new("bms", 3).unwrap();
        for _ in 0..3 {
            bms.update_bar(&bar("105", "110", "100", "100")).unwrap();
        }
        let result = bms.update_bar(&bar("105", "110", "100", "100")).unwrap();
        if let SignalValue::Scalar(v) = result {
            assert!(v < dec!(0), "bearish bar should give negative score");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_bms_reset() {
        let mut bms = BarMomentumScore::new("bms", 3).unwrap();
        for _ in 0..3 { bms.update_bar(&bar("100", "110", "90", "105")).unwrap(); }
        assert!(bms.is_ready());
        bms.reset();
        assert!(!bms.is_ready());
    }
}
