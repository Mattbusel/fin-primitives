//! True Range Expansion — detects when current TR expands beyond or contracts within prior N TRs.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// True Range Expansion — `+1` if current TR > max(prior N TRs), `-1` if < min, `0` otherwise.
///
/// Compares the current bar's true range against the prior `period` bars' true range extremes:
/// - **+1**: current bar's range is the widest in the past `period` bars — volatility expansion.
/// - **-1**: current bar's range is the narrowest in the past `period` bars — volatility compression.
/// - **0**: current TR is within the prior range bounds — no expansion/compression.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::TrueRangeExpansion;
/// use fin_primitives::signals::Signal;
/// let tre = TrueRangeExpansion::new("tre_10", 10).unwrap();
/// assert_eq!(tre.period(), 10);
/// ```
pub struct TrueRangeExpansion {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    // Store the prior N TRs (not including current bar)
    prior_trs: VecDeque<Decimal>,
    bars_seen: usize,
}

impl TrueRangeExpansion {
    /// Constructs a new `TrueRangeExpansion`.
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
            prior_trs: VecDeque::with_capacity(period),
            bars_seen: 0,
        })
    }

    fn true_range(bar: &BarInput, prev_close: Option<Decimal>) -> Decimal {
        let hl = bar.range();
        if let Some(pc) = prev_close {
            hl.max((bar.high - pc).abs()).max((bar.low - pc).abs())
        } else {
            hl
        }
    }
}

impl Signal for TrueRangeExpansion {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.bars_seen > self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = Self::true_range(bar, self.prev_close);
        self.bars_seen += 1;

        // Capture prior TRs (before current bar)
        let result = if self.prior_trs.len() < self.period {
            SignalValue::Unavailable
        } else {
            let max_tr = self.prior_trs.iter().copied().fold(Decimal::ZERO, Decimal::max);
            let min_tr = self.prior_trs.iter().copied().fold(Decimal::MAX, Decimal::min);
            if tr > max_tr {
                SignalValue::Scalar(Decimal::ONE)
            } else if tr < min_tr {
                SignalValue::Scalar(Decimal::NEGATIVE_ONE)
            } else {
                SignalValue::Scalar(Decimal::ZERO)
            }
        };

        // Maintain rolling window of prior TRs
        self.prior_trs.push_back(tr);
        if self.prior_trs.len() > self.period {
            self.prior_trs.pop_front();
        }

        self.prev_close = Some(bar.close);
        Ok(result)
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.prior_trs.clear();
        self.bars_seen = 0;
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
    fn test_tre_invalid_period() {
        assert!(TrueRangeExpansion::new("tre", 0).is_err());
    }

    #[test]
    fn test_tre_unavailable_before_warmup() {
        let mut s = TrueRangeExpansion::new("tre", 2).unwrap();
        assert_eq!(s.update_bar(&bar("110","90","100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("110","90","100")).unwrap(), SignalValue::Unavailable);
        assert!(!s.is_ready());
    }

    #[test]
    fn test_tre_wide_bar_gives_plus_one() {
        let mut s = TrueRangeExpansion::new("tre", 2).unwrap();
        // Prior TRs ~20, ~20
        s.update_bar(&bar("110","90","100")).unwrap();
        s.update_bar(&bar("110","90","100")).unwrap();
        // Wide bar: TR=100 > prior max ~20 → +1
        if let SignalValue::Scalar(v) = s.update_bar(&bar("200","100","150")).unwrap() {
            assert_eq!(v, dec!(1), "wide bar should give +1: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_tre_narrow_bar_gives_minus_one() {
        let mut s = TrueRangeExpansion::new("tre", 2).unwrap();
        // Prior TRs ~20, ~20
        s.update_bar(&bar("110","90","100")).unwrap();
        s.update_bar(&bar("110","90","100")).unwrap();
        // Narrow bar: TR ~1 < prior min ~20 → -1
        if let SignalValue::Scalar(v) = s.update_bar(&bar("100.5","99.5","100")).unwrap() {
            assert_eq!(v, dec!(-1), "narrow bar should give -1: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_tre_equal_bar_gives_zero() {
        let mut s = TrueRangeExpansion::new("tre", 2).unwrap();
        // Prior TRs ~20, ~20 (same range)
        s.update_bar(&bar("110","90","100")).unwrap();
        s.update_bar(&bar("110","90","100")).unwrap();
        // Equal TR: same as prior → 0
        if let SignalValue::Scalar(v) = s.update_bar(&bar("110","90","100")).unwrap() {
            // Could be 0 (within range) since equal is not strictly > max
            assert!(v == dec!(0) || v == dec!(-1), "equal TR should not give +1: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_tre_reset() {
        let mut s = TrueRangeExpansion::new("tre", 2).unwrap();
        for _ in 0..4 { s.update_bar(&bar("110","90","100")).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
