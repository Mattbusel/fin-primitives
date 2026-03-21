//! Intraday Momentum indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Intraday Momentum.
///
/// Measures the net intrabar directional move (close − open) accumulated over
/// a rolling window and normalized by the total intrabar range. Captures
/// whether the majority of intrabar moves are in the same direction.
///
/// Per-bar formula:
/// - `body = close - open`
/// - `range = high - low` (0 when flat)
///
/// Rolling:
/// - `net_body = Σ body`
/// - `total_range = Σ range`
/// - `imom = net_body / total_range` ∈ [−1, +1] (0 when total_range == 0)
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::IntradayMomentum;
/// use fin_primitives::signals::Signal;
/// let im = IntradayMomentum::new("imom_14", 14).unwrap();
/// assert_eq!(im.period(), 14);
/// ```
pub struct IntradayMomentum {
    name: String,
    period: usize,
    /// (body, range) per bar
    bars: VecDeque<(Decimal, Decimal)>,
}

impl IntradayMomentum {
    /// Constructs a new `IntradayMomentum`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { name: name.into(), period, bars: VecDeque::with_capacity(period) })
    }
}

impl Signal for IntradayMomentum {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let body = bar.close - bar.open;
        let range = bar.high - bar.low;
        self.bars.push_back((body, range));
        if self.bars.len() > self.period {
            self.bars.pop_front();
        }
        if self.bars.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let net_body: Decimal = self.bars.iter().map(|(b, _)| b).copied().sum();
        let total_range: Decimal = self.bars.iter().map(|(_, r)| r).copied().sum();

        if total_range.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let imom = net_body.checked_div(total_range).ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(imom))
    }

    fn is_ready(&self) -> bool {
        self.bars.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.bars.clear();
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
        let hi = Price::new(h.parse().unwrap()).unwrap();
        let lo = Price::new(l.parse().unwrap()).unwrap();
        let cl = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hi, low: lo, close: cl,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_period_zero_fails() {
        assert!(matches!(IntradayMomentum::new("im", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut im = IntradayMomentum::new("im", 3).unwrap();
        assert_eq!(im.update_bar(&bar("10", "12", "9", "11")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_all_bullish_full_body_gives_one() {
        // open=8, high=12, low=8, close=12 → body=4, range=4 → ratio=1
        let mut im = IntradayMomentum::new("im", 3).unwrap();
        for _ in 0..3 {
            im.update_bar(&bar("8", "12", "8", "12")).unwrap();
        }
        let v = im.update_bar(&bar("8", "12", "8", "12")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_all_bearish_full_body_gives_minus_one() {
        // open=12, high=12, low=8, close=8 → body=-4, range=4 → ratio=-1
        let mut im = IntradayMomentum::new("im", 3).unwrap();
        for _ in 0..3 {
            im.update_bar(&bar("12", "12", "8", "8")).unwrap();
        }
        let v = im.update_bar(&bar("12", "12", "8", "8")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_reset() {
        let mut im = IntradayMomentum::new("im", 2).unwrap();
        im.update_bar(&bar("8", "12", "8", "11")).unwrap();
        im.update_bar(&bar("8", "12", "8", "11")).unwrap();
        assert!(im.is_ready());
        im.reset();
        assert!(!im.is_ready());
    }
}
