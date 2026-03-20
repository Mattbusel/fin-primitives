//! Relative Volatility indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Relative Volatility — ratio of short-term ATR to long-term ATR.
///
/// ```text
/// ATR_short = average true range over `short` bars
/// ATR_long  = average true range over `long`  bars
/// RV        = ATR_short / ATR_long
/// ```
///
/// Values > 1 indicate expanding volatility; < 1 indicate contraction.
/// Requires `long + 1` bars (for true range calculation).
///
/// Returns [`SignalValue::Unavailable`] until `long + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RelativeVolatility;
/// use fin_primitives::signals::Signal;
///
/// let rv = RelativeVolatility::new("rv", 5, 20).unwrap();
/// assert_eq!(rv.period(), 20);
/// ```
pub struct RelativeVolatility {
    name: String,
    short: usize,
    long: usize,
    prev_close: Option<Decimal>,
    trs: VecDeque<Decimal>,
}

impl RelativeVolatility {
    /// Creates a new `RelativeVolatility`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if either period is zero or `short >= long`.
    pub fn new(name: impl Into<String>, short: usize, long: usize) -> Result<Self, FinError> {
        if short == 0 { return Err(FinError::InvalidPeriod(short)); }
        if long == 0  { return Err(FinError::InvalidPeriod(long)); }
        if short >= long { return Err(FinError::InvalidPeriod(short)); }
        Ok(Self {
            name: name.into(),
            short,
            long,
            prev_close: None,
            trs: VecDeque::with_capacity(long),
        })
    }
}

impl Signal for RelativeVolatility {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = bar.true_range(self.prev_close);
        self.prev_close = Some(bar.close);

        self.trs.push_back(tr);
        if self.trs.len() > self.long { self.trs.pop_front(); }
        if self.trs.len() < self.long { return Ok(SignalValue::Unavailable); }

        let atr_long = self.trs.iter().sum::<Decimal>()
            / Decimal::from(self.long as u32);
        let atr_short = self.trs.iter().rev().take(self.short).sum::<Decimal>()
            / Decimal::from(self.short as u32);

        if atr_long.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ONE));
        }
        Ok(SignalValue::Scalar(atr_short / atr_long))
    }

    fn is_ready(&self) -> bool { self.trs.len() >= self.long }
    fn period(&self) -> usize { self.long }

    fn reset(&mut self) {
        self.prev_close = None;
        self.trs.clear();
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
        OhlcvBar { symbol: Symbol::new("X").unwrap(), open: p, high: p, low: p, close: p,
            volume: Quantity::zero(), ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1), tick_count: 1 }
    }

    #[test]
    fn test_rv_invalid() {
        assert!(RelativeVolatility::new("r", 0, 20).is_err());
        assert!(RelativeVolatility::new("r", 20, 5).is_err());
    }

    #[test]
    fn test_rv_unavailable_before_warmup() {
        let mut rv = RelativeVolatility::new("r", 3, 5).unwrap();
        for _ in 0..4 { assert_eq!(rv.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable); }
    }

    #[test]
    fn test_rv_flat_is_one() {
        let mut rv = RelativeVolatility::new("r", 3, 5).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..10 { last = rv.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(1));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_rv_reset() {
        let mut rv = RelativeVolatility::new("r", 3, 5).unwrap();
        for _ in 0..10 { rv.update_bar(&bar("100")).unwrap(); }
        assert!(rv.is_ready());
        rv.reset();
        assert!(!rv.is_ready());
    }
}
