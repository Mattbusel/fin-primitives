//! Body Height Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Body Height Ratio — measures the current bar's body size relative to the
/// rolling Average True Range (ATR), indicating whether the current candle is
/// unusually large or small given recent volatility.
///
/// ```text
/// body_height_ratio = |close - open| / ATR(period)
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BodyHeightRatio;
/// use fin_primitives::signals::Signal;
///
/// let bhr = BodyHeightRatio::new("bhr", 14).unwrap();
/// assert_eq!(bhr.period(), 14);
/// ```
pub struct BodyHeightRatio {
    name: String,
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
    closes: VecDeque<Decimal>,
}

impl BodyHeightRatio {
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            highs: VecDeque::with_capacity(period + 1),
            lows: VecDeque::with_capacity(period + 1),
            closes: VecDeque::with_capacity(period + 2),
        })
    }

    fn atr(highs: &VecDeque<Decimal>, lows: &VecDeque<Decimal>, closes: &VecDeque<Decimal>, period: usize) -> Decimal {
        if highs.len() < period || closes.len() < 2 { return Decimal::ZERO; }
        let trs: Vec<Decimal> = highs.iter().rev().take(period)
            .zip(lows.iter().rev().take(period))
            .zip(closes.iter().rev().skip(1).take(period))
            .map(|((h, l), pc)| {
                let hl = h - l;
                let hc = (h - pc).abs();
                let lc = (l - pc).abs();
                hl.max(hc).max(lc)
            })
            .collect();
        if trs.is_empty() { return Decimal::ZERO; }
        #[allow(clippy::cast_possible_truncation)]
        let n = trs.len() as u32;
        trs.iter().sum::<Decimal>() / Decimal::from(n)
    }
}

impl Signal for BodyHeightRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.highs.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.highs.push_back(bar.high);
        if self.highs.len() > self.period + 1 { self.highs.pop_front(); }
        self.lows.push_back(bar.low);
        if self.lows.len() > self.period + 1 { self.lows.pop_front(); }
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 2 { self.closes.pop_front(); }

        if self.highs.len() < self.period { return Ok(SignalValue::Unavailable); }

        let atr = Self::atr(&self.highs, &self.lows, &self.closes, self.period);
        if atr.is_zero() { return Ok(SignalValue::Unavailable); }
        let body = (bar.close - bar.open).abs();
        Ok(SignalValue::Scalar(body / atr))
    }

    fn reset(&mut self) {
        self.highs.clear();
        self.lows.clear();
        self.closes.clear();
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
    fn test_bhr_invalid() { assert!(BodyHeightRatio::new("b", 0).is_err()); }

    #[test]
    fn test_bhr_unavailable() {
        let mut bhr = BodyHeightRatio::new("b", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(bhr.update_bar(&bar("99", "110", "90", "100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_bhr_ready_after_warm_up() {
        let mut bhr = BodyHeightRatio::new("b", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..4 {
            last = bhr.update_bar(&bar("99", "110", "90", "105")).unwrap();
        }
        assert!(matches!(last, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_bhr_doji_gives_zero() {
        let mut bhr = BodyHeightRatio::new("b", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            last = bhr.update_bar(&bar("100", "110", "90", "100")).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0));
        }
    }

    #[test]
    fn test_bhr_reset() {
        let mut bhr = BodyHeightRatio::new("b", 3).unwrap();
        for _ in 0..4 { bhr.update_bar(&bar("99", "110", "90", "105")).unwrap(); }
        assert!(bhr.is_ready());
        bhr.reset();
        assert!(!bhr.is_ready());
    }
}
