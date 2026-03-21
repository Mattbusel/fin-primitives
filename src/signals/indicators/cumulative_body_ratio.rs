//! Cumulative Body Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Cumulative Body Ratio.
///
/// Computes the ratio of the cumulative signed body (net directional move)
/// to the cumulative absolute body (total body movement) over a rolling window.
///
/// Per-bar formula:
/// - `signed_body = close - open`
/// - `abs_body = |close - open|`
///
/// Rolling:
/// - `net = Σ signed_body`
/// - `total = Σ abs_body`
/// - `cbr = net / total` ∈ [−1, +1] (0 when total == 0)
///
/// - +1: all bodies are bullish (all closes above opens).
/// - −1: all bodies are bearish.
/// - 0: perfect cancellation between bull and bear bodies.
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CumulativeBodyRatio;
/// use fin_primitives::signals::Signal;
/// let cbr = CumulativeBodyRatio::new("cbr_14", 14).unwrap();
/// assert_eq!(cbr.period(), 14);
/// ```
pub struct CumulativeBodyRatio {
    name: String,
    period: usize,
    /// (signed_body, abs_body) per bar
    bars: VecDeque<(Decimal, Decimal)>,
}

impl CumulativeBodyRatio {
    /// Constructs a new `CumulativeBodyRatio`.
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

impl Signal for CumulativeBodyRatio {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let signed = bar.close - bar.open;
        let abs = signed.abs();
        self.bars.push_back((signed, abs));
        if self.bars.len() > self.period {
            self.bars.pop_front();
        }
        if self.bars.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let net: Decimal = self.bars.iter().map(|(s, _)| s).copied().sum();
        let total: Decimal = self.bars.iter().map(|(_, a)| a).copied().sum();

        if total.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let cbr = net.checked_div(total).ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(cbr))
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

    fn bar(o: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cl = Price::new(c.parse().unwrap()).unwrap();
        let hi = op.max(cl);
        let lo = op.min(cl);
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
        assert!(matches!(CumulativeBodyRatio::new("cbr", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut cbr = CumulativeBodyRatio::new("cbr", 3).unwrap();
        assert_eq!(cbr.update_bar(&bar("10", "11")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_all_bull_gives_one() {
        let mut cbr = CumulativeBodyRatio::new("cbr", 3).unwrap();
        for _ in 0..3 {
            cbr.update_bar(&bar("10", "12")).unwrap();
        }
        let v = cbr.update_bar(&bar("10", "12")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_all_bear_gives_minus_one() {
        let mut cbr = CumulativeBodyRatio::new("cbr", 3).unwrap();
        for _ in 0..3 {
            cbr.update_bar(&bar("12", "10")).unwrap();
        }
        let v = cbr.update_bar(&bar("12", "10")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_cancel_gives_zero() {
        // Alternating +2 and -2 bodies → net=0, total=4*n → cbr=0
        let mut cbr = CumulativeBodyRatio::new("cbr", 4).unwrap();
        cbr.update_bar(&bar("10", "12")).unwrap(); // +2
        cbr.update_bar(&bar("12", "10")).unwrap(); // -2
        cbr.update_bar(&bar("10", "12")).unwrap(); // +2
        let v = cbr.update_bar(&bar("12", "10")).unwrap(); // -2
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_reset() {
        let mut cbr = CumulativeBodyRatio::new("cbr", 2).unwrap();
        cbr.update_bar(&bar("10", "12")).unwrap();
        cbr.update_bar(&bar("10", "12")).unwrap();
        assert!(cbr.is_ready());
        cbr.reset();
        assert!(!cbr.is_ready());
    }
}
