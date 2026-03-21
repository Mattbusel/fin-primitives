//! Price Reversal Index indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Reversal Index.
///
/// Measures the tendency for price to reverse within a rolling window by counting
/// how often consecutive bars move in opposite directions, normalized to [0, 1].
///
/// For each consecutive pair of close-to-close returns in the window:
/// - Reversal: the two returns have opposite signs.
/// - Continuation: same sign (or at least one zero).
///
/// Formula: `pri = reversal_pairs / (period - 1)`
///
/// - 1.0: every consecutive pair reversed (maximum choppiness).
/// - 0.0: every pair continued (pure trend).
/// - ~0.5: random walk behavior.
///
/// Returns `SignalValue::Unavailable` until `period + 1` closes accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceReversalIndex;
/// use fin_primitives::signals::Signal;
/// let pri = PriceReversalIndex::new("pri_10", 10).unwrap();
/// assert_eq!(pri.period(), 10);
/// ```
pub struct PriceReversalIndex {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl PriceReversalIndex {
    /// Constructs a new `PriceReversalIndex`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            closes: VecDeque::with_capacity(period + 1),
        })
    }
}

impl Signal for PriceReversalIndex {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }

        let mut reversals: usize = 0;
        for i in 1..self.closes.len() - 1 {
            let r_prev = self.closes[i] - self.closes[i - 1];
            let r_curr = self.closes[i + 1] - self.closes[i];
            if (r_prev > Decimal::ZERO && r_curr < Decimal::ZERO)
                || (r_prev < Decimal::ZERO && r_curr > Decimal::ZERO)
            {
                reversals += 1;
            }
        }

        #[allow(clippy::cast_possible_truncation)]
        let pri = Decimal::from(reversals as u64)
            .checked_div(Decimal::from((self.period - 1) as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(pri))
    }

    fn is_ready(&self) -> bool {
        self.closes.len() >= self.period + 1
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
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

    fn bar(close: &str) -> OhlcvBar {
        let p = Price::new(close.parse().unwrap()).unwrap();
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
    fn test_period_less_than_2_fails() {
        assert!(PriceReversalIndex::new("pri", 0).is_err());
        assert!(PriceReversalIndex::new("pri", 1).is_err());
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut pri = PriceReversalIndex::new("pri", 3).unwrap();
        assert_eq!(pri.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_pure_trend_zero() {
        // Prices strictly increasing → no reversals → 0
        let mut pri = PriceReversalIndex::new("pri", 4).unwrap();
        for i in 0..5u32 {
            pri.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        let v = pri.update_bar(&bar("105")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_alternating_gives_one() {
        // Prices: 100, 102, 100, 102, 100 → 3 reversals, period-1=3 → pri=1
        let mut pri = PriceReversalIndex::new("pri", 4).unwrap();
        pri.update_bar(&bar("100")).unwrap();
        pri.update_bar(&bar("102")).unwrap();
        pri.update_bar(&bar("100")).unwrap();
        pri.update_bar(&bar("102")).unwrap();
        let v = pri.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_reset() {
        let mut pri = PriceReversalIndex::new("pri", 3).unwrap();
        for _ in 0..4 {
            pri.update_bar(&bar("100")).unwrap();
        }
        assert!(pri.is_ready());
        pri.reset();
        assert!(!pri.is_ready());
    }
}
