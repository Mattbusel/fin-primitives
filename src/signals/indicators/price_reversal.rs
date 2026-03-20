//! Price Reversal indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Reversal — detects when close crosses the N-period median, normalized by ATR.
///
/// ```text
/// median_t   = median(close, period)
/// atr_t      = mean(|close_i − close_{i−1}|, period)   [simplified ATR using close changes]
/// output     = (close_t − median_t) / atr_t
/// ```
///
/// Positive values indicate close is above median (momentum); negative below (reversal setup).
/// Returns 0 when ATR is zero (flat market).
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceReversal;
/// use fin_primitives::signals::Signal;
///
/// let pr = PriceReversal::new("pr", 14).unwrap();
/// assert_eq!(pr.period(), 14);
/// ```
pub struct PriceReversal {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
    changes: VecDeque<Decimal>,
    prev_close: Option<Decimal>,
}

impl PriceReversal {
    /// Creates a new `PriceReversal`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            closes: VecDeque::with_capacity(period),
            changes: VecDeque::with_capacity(period),
            prev_close: None,
        })
    }
}

impl Signal for PriceReversal {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let change = (bar.close - pc).abs();
            self.changes.push_back(change);
            if self.changes.len() > self.period { self.changes.pop_front(); }
        }
        self.prev_close = Some(bar.close);

        self.closes.push_back(bar.close);
        if self.closes.len() > self.period { self.closes.pop_front(); }

        if self.closes.len() < self.period || self.changes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        // Compute median
        let mut sorted: Vec<Decimal> = self.closes.iter().cloned().collect();
        sorted.sort();
        let median = if self.period % 2 == 1 {
            sorted[self.period / 2]
        } else {
            (sorted[self.period / 2 - 1] + sorted[self.period / 2]) / Decimal::from(2u32)
        };

        let atr = self.changes.iter().sum::<Decimal>() / Decimal::from(self.period as u32);

        if atr.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let current = bar.close;
        Ok(SignalValue::Scalar((current - median) / atr))
    }

    fn is_ready(&self) -> bool {
        self.closes.len() >= self.period && self.changes.len() >= self.period
    }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.closes.clear();
        self.changes.clear();
        self.prev_close = None;
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

    #[test]
    fn test_pr_invalid() {
        assert!(PriceReversal::new("p", 0).is_err());
        assert!(PriceReversal::new("p", 1).is_err());
    }

    #[test]
    fn test_pr_unavailable_before_warmup() {
        let mut p = PriceReversal::new("p", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(p.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_pr_flat_is_zero() {
        let mut p = PriceReversal::new("p", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..8 { last = p.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_pr_above_median_positive() {
        // bars=100,100,100,110,120; at bar5: closes=[100,110,120], median=110
        // changes=[0,10,10,10], atr=(10+10+10)/3≈10
        // output = (120-110)/10 = 1 > 0
        let mut p = PriceReversal::new("p", 3).unwrap();
        for _ in 0..3 { p.update_bar(&bar("100")).unwrap(); }
        p.update_bar(&bar("110")).unwrap();
        if let SignalValue::Scalar(v) = p.update_bar(&bar("120")).unwrap() {
            assert!(v > dec!(0), "expected positive, got {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_pr_reset() {
        let mut p = PriceReversal::new("p", 3).unwrap();
        for _ in 0..8 { p.update_bar(&bar("100")).unwrap(); }
        assert!(p.is_ready());
        p.reset();
        assert!(!p.is_ready());
    }
}
