//! Price Gap indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Gap — rolling mean of gaps between successive closes.
///
/// ```text
/// gap_t  = close_t − close_{t−1}
/// output = mean(gap, period)
/// ```
///
/// Positive output indicates persistent upward drift; negative downward drift.
/// Useful for detecting systematic price drift or momentum.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceGap;
/// use fin_primitives::signals::Signal;
///
/// let pg = PriceGap::new("pg", 10).unwrap();
/// assert_eq!(pg.period(), 10);
/// ```
pub struct PriceGap {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    gaps: VecDeque<Decimal>,
}

impl PriceGap {
    /// Creates a new `PriceGap`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            gaps: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for PriceGap {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let gap = bar.close - pc;
            self.gaps.push_back(gap);
            if self.gaps.len() > self.period { self.gaps.pop_front(); }
        }
        self.prev_close = Some(bar.close);

        if self.gaps.len() < self.period { return Ok(SignalValue::Unavailable); }

        let avg = self.gaps.iter().sum::<Decimal>() / Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(avg))
    }

    fn is_ready(&self) -> bool { self.gaps.len() >= self.period }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.prev_close = None;
        self.gaps.clear();
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
    fn test_pg_invalid() {
        assert!(PriceGap::new("p", 0).is_err());
    }

    #[test]
    fn test_pg_unavailable_before_warmup() {
        let mut p = PriceGap::new("p", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(p.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_pg_flat_is_zero() {
        let mut p = PriceGap::new("p", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..6 { last = p.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_pg_constant_rise() {
        // Each bar rises by 2: mean gap = 2
        let mut p = PriceGap::new("p", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0u32..8 {
            let price = format!("{}", 100 + 2 * i);
            last = p.update_bar(&bar(&price)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(2));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_pg_alternating_zero_mean() {
        // +5, -5, +5, -5 → mean gap = 0
        let mut p = PriceGap::new("p", 4).unwrap();
        let mut last = SignalValue::Unavailable;
        for c in ["100", "105", "100", "105", "100"] {
            last = p.update_bar(&bar(c)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_pg_reset() {
        let mut p = PriceGap::new("p", 3).unwrap();
        for _ in 0..6 { p.update_bar(&bar("100")).unwrap(); }
        assert!(p.is_ready());
        p.reset();
        assert!(!p.is_ready());
    }
}
