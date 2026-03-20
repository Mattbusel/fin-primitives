//! True Range Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// True Range Ratio — current True Range divided by the SMA of True Range over `period` bars.
///
/// ```text
/// TR  = max(high - low, |high - prev_close|, |low - prev_close|)
/// TRR = TR / SMA(TR, period)
/// ```
///
/// Values > 1 indicate an above-average range (potential breakout or spike).
/// Values < 1 indicate a below-average range (compression/consolidation).
///
/// Returns [`SignalValue::Unavailable`] until `period` bars of TR have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::TrueRangeRatio;
/// use fin_primitives::signals::Signal;
///
/// let trr = TrueRangeRatio::new("trr", 14).unwrap();
/// assert_eq!(trr.period(), 14);
/// ```
pub struct TrueRangeRatio {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    trs: VecDeque<Decimal>,
    sum: Decimal,
}

impl TrueRangeRatio {
    /// Constructs a new `TrueRangeRatio`.
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

impl Signal for TrueRangeRatio {
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
        let sma = self.sum / nd;
        if sma.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(tr / sma))
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

    fn bar(h: &str, l: &str, c: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_trr_invalid_period() {
        assert!(TrueRangeRatio::new("trr", 0).is_err());
    }

    #[test]
    fn test_trr_unavailable_before_warm_up() {
        let mut trr = TrueRangeRatio::new("trr", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(trr.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_trr_constant_range_gives_one() {
        // All bars have equal TR → ratio = 1
        let mut trr = TrueRangeRatio::new("trr", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            last = trr.update_bar(&bar("110", "90", "100")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_trr_large_bar_above_one() {
        let mut trr = TrueRangeRatio::new("trr", 3).unwrap();
        // Seed with 3 small bars (TR=10 each)
        for _ in 0..3 {
            trr.update_bar(&bar("105", "95", "100")).unwrap();
        }
        // Now feed a big bar (TR=100), rolling SMA becomes dominated by small bars initially
        let result = trr.update_bar(&bar("200", "100", "150")).unwrap();
        if let SignalValue::Scalar(v) = result {
            assert!(v > dec!(1), "large bar should give ratio > 1, got {}", v);
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_trr_reset() {
        let mut trr = TrueRangeRatio::new("trr", 3).unwrap();
        for _ in 0..3 { trr.update_bar(&bar("110", "90", "100")).unwrap(); }
        assert!(trr.is_ready());
        trr.reset();
        assert!(!trr.is_ready());
    }
}
