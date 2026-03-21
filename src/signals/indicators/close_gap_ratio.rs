//! Close Gap Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Close Gap Ratio.
///
/// Rolling mean of the absolute close-to-close gap expressed as a percentage
/// of the prior close. Measures how large each session's price move is
/// relative to the prior close, on average.
///
/// Per-bar formula: `gap_pct = |close_t - close_{t-1}| / close_{t-1} * 100`
///
/// Rolling: `mean(gap_pct, period)`
///
/// - High value: large average moves between sessions.
/// - Low value: price stable between sessions.
///
/// Returns `SignalValue::Unavailable` until `period + 1` closes accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseGapRatio;
/// use fin_primitives::signals::Signal;
/// let cgr = CloseGapRatio::new("cgr_14", 14).unwrap();
/// assert_eq!(cgr.period(), 14);
/// ```
pub struct CloseGapRatio {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
    gaps: VecDeque<Decimal>,
}

impl CloseGapRatio {
    /// Constructs a new `CloseGapRatio`.
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
            closes: VecDeque::with_capacity(2),
            gaps: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for CloseGapRatio {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > 2 {
            self.closes.pop_front();
        }
        if self.closes.len() < 2 {
            return Ok(SignalValue::Unavailable);
        }

        let prev = self.closes[0];
        let curr = self.closes[1];

        let gap_pct = if prev.is_zero() {
            Decimal::ZERO
        } else {
            let abs_gap = (curr - prev).abs();
            abs_gap
                .checked_div(prev)
                .ok_or(FinError::ArithmeticOverflow)?
                .checked_mul(Decimal::from(100u32))
                .ok_or(FinError::ArithmeticOverflow)?
        };

        self.gaps.push_back(gap_pct);
        if self.gaps.len() > self.period {
            self.gaps.pop_front();
        }
        if self.gaps.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sum: Decimal = self.gaps.iter().copied().sum();
        #[allow(clippy::cast_possible_truncation)]
        let avg = sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(avg))
    }

    fn is_ready(&self) -> bool {
        self.gaps.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.closes.clear();
        self.gaps.clear();
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
    fn test_period_zero_fails() {
        assert!(matches!(CloseGapRatio::new("cgr", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_ready() {
        let mut cgr = CloseGapRatio::new("cgr", 3).unwrap();
        assert_eq!(cgr.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_flat_price_zero_gap() {
        let mut cgr = CloseGapRatio::new("cgr", 3).unwrap();
        for _ in 0..4 {
            cgr.update_bar(&bar("100")).unwrap();
        }
        let v = cgr.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_one_pct_gap() {
        // 100 → 101 each step: gap = 1% per step, mean = 1%
        let mut cgr = CloseGapRatio::new("cgr", 3).unwrap();
        cgr.update_bar(&bar("100")).unwrap();
        cgr.update_bar(&bar("101")).unwrap();
        cgr.update_bar(&bar("102.01")).unwrap();
        let v = cgr.update_bar(&bar("103.0301")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s > dec!(0));
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_reset() {
        let mut cgr = CloseGapRatio::new("cgr", 2).unwrap();
        for i in 0..3u32 {
            cgr.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        assert!(cgr.is_ready());
        cgr.reset();
        assert!(!cgr.is_ready());
    }
}
