//! Ulcer Index indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::collections::VecDeque;

/// Ulcer Index — measures downside volatility (drawdown pain) over a window.
///
/// ```text
/// drawdown_i = 100 × (close_i − max(close, period)) / max(close, period)
/// UI         = sqrt( mean(drawdown_i², period) )
/// ```
///
/// Higher values indicate larger or more prolonged drawdowns.
/// Zero when price is at or above the period high on every bar.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::UlcerIndex;
/// use fin_primitives::signals::Signal;
///
/// let ui = UlcerIndex::new("ui", 14).unwrap();
/// assert_eq!(ui.period(), 14);
/// ```
pub struct UlcerIndex {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl UlcerIndex {
    /// Creates a new `UlcerIndex`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            closes: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for UlcerIndex {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period { self.closes.pop_front(); }
        if self.closes.len() < self.period { return Ok(SignalValue::Unavailable); }

        let period_high = self.closes.iter().cloned().max().unwrap();

        if period_high.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let mean_sq: f64 = {
            let sum_sq: f64 = self.closes.iter()
                .filter_map(|c| {
                    let h = period_high.to_f64()?;
                    let cv = c.to_f64()?;
                    let dd = 100.0 * (cv - h) / h;
                    Some(dd * dd)
                })
                .sum();
            sum_sq / self.period as f64
        };

        let ui = mean_sq.sqrt();
        Ok(SignalValue::Scalar(
            Decimal::try_from(ui).unwrap_or(Decimal::ZERO)
        ))
    }

    fn is_ready(&self) -> bool { self.closes.len() >= self.period }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.closes.clear();
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
    fn test_ui_invalid() {
        assert!(UlcerIndex::new("u", 0).is_err());
    }

    #[test]
    fn test_ui_unavailable_before_warmup() {
        let mut u = UlcerIndex::new("u", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(u.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_ui_flat_is_zero() {
        // Flat price: every close equals the period high → all drawdowns = 0 → UI = 0
        let mut u = UlcerIndex::new("u", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = u.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_ui_falling_is_positive() {
        // Falling prices: close < period_high → UI > 0
        let mut u = UlcerIndex::new("u", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for p in ["100", "99", "98"] {
            last = u.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(0), "expected positive UI, got {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_ui_reset() {
        let mut u = UlcerIndex::new("u", 3).unwrap();
        for _ in 0..5 { u.update_bar(&bar("100")).unwrap(); }
        assert!(u.is_ready());
        u.reset();
        assert!(!u.is_ready());
    }
}
