//! Close Momentum Rank indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Close Momentum Rank.
///
/// Ranks the current bar's close-to-close return within the distribution of
/// returns over the lookback window. The output is the percentile rank of the
/// current return among the `period` most recent returns.
///
/// Formula: `rank = count(returns ≤ current_return) / period × 100`
///
/// - Values near 100 indicate the current return is one of the largest in the window.
/// - Values near 0 indicate the current return is one of the smallest (most negative).
/// - Values near 50 indicate the return is typical.
///
/// Returns `SignalValue::Unavailable` until `period + 1` bars (first bar no return).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseMomentumRank;
/// use fin_primitives::signals::Signal;
/// let cmr = CloseMomentumRank::new("cmr_20", 20).unwrap();
/// assert_eq!(cmr.period(), 20);
/// ```
pub struct CloseMomentumRank {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    returns: VecDeque<Decimal>,
}

impl CloseMomentumRank {
    /// Constructs a new `CloseMomentumRank` with the given name and period.
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
            returns: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for CloseMomentumRank {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let Some(pc) = self.prev_close else {
            self.prev_close = Some(bar.close);
            return Ok(SignalValue::Unavailable);
        };

        let ret = if pc.is_zero() {
            Decimal::ZERO
        } else {
            (bar.close - pc)
                .checked_div(pc)
                .ok_or(FinError::ArithmeticOverflow)?
        };

        self.prev_close = Some(bar.close);
        self.returns.push_back(ret);
        if self.returns.len() > self.period {
            self.returns.pop_front();
        }

        if self.returns.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let count_le = self.returns.iter().filter(|&&r| r <= ret).count();
        #[allow(clippy::cast_possible_truncation)]
        let rank = Decimal::from(count_le as u32)
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_mul(Decimal::from(100u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(rank))
    }

    fn is_ready(&self) -> bool {
        self.returns.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.returns.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(close: &str) -> OhlcvBar {
        let p = Price::new(close.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p,
            high: p,
            low: p,
            close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_period_zero_fails() {
        assert!(matches!(CloseMomentumRank::new("cmr", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut cmr = CloseMomentumRank::new("cmr", 3).unwrap();
        let v = cmr.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_ready_after_period_plus_one() {
        let mut cmr = CloseMomentumRank::new("cmr", 3).unwrap();
        for _ in 0..4 {
            cmr.update_bar(&bar("100")).unwrap();
        }
        assert!(cmr.is_ready());
    }

    #[test]
    fn test_constant_price_median_rank() {
        // All returns = 0: rank = 100% (all ≤ 0)
        let mut cmr = CloseMomentumRank::new("cmr", 3).unwrap();
        for _ in 0..5 {
            cmr.update_bar(&bar("100")).unwrap();
        }
        let v = cmr.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_large_return_high_rank() {
        let mut cmr = CloseMomentumRank::new("cmr", 3).unwrap();
        for p in ["100", "101", "100", "101"] {
            cmr.update_bar(&bar(p)).unwrap();
        }
        // Large spike
        let v = cmr.update_bar(&bar("200")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s > dec!(50), "expected high rank for large return, got {}", s);
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_reset() {
        let mut cmr = CloseMomentumRank::new("cmr", 3).unwrap();
        for _ in 0..5 {
            cmr.update_bar(&bar("100")).unwrap();
        }
        assert!(cmr.is_ready());
        cmr.reset();
        assert!(!cmr.is_ready());
        assert!(cmr.prev_close.is_none());
    }
}
