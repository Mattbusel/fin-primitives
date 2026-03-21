//! Rolling Calmar Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling Calmar Ratio — cumulative return over the window divided by the maximum
/// drawdown within the same window.
///
/// ```text
/// cum_return    = (close[t] - close[t-period]) / close[t-period]  × 100
/// max_drawdown  = max peak-to-trough drawdown of close prices in window  (positive %)
/// calmar        = cum_return / max_drawdown
/// ```
///
/// - **High positive value**: strong return relative to the worst drawdown experienced.
/// - **Near zero or negative**: returns are poor relative to drawdown risk.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars are collected, or
/// when the max drawdown in the window is zero (monotonically rising prices).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period < 2`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CalmarRatio;
/// use fin_primitives::signals::Signal;
/// let cr = CalmarRatio::new("calmar_20", 20).unwrap();
/// assert_eq!(cr.period(), 20);
/// ```
pub struct CalmarRatio {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
}

impl CalmarRatio {
    /// Constructs a new `CalmarRatio`.
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
            window: VecDeque::with_capacity(period + 1),
        })
    }
}

impl Signal for CalmarRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() > self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.close);
        if self.window.len() > self.period + 1 {
            self.window.pop_front();
        }
        if self.window.len() <= self.period {
            return Ok(SignalValue::Unavailable);
        }

        let prices: Vec<Decimal> = self.window.iter().copied().collect();
        let first = prices[0];
        let last = *prices.last().unwrap();

        if first.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let cum_return = (last - first)
            .checked_div(first)
            .ok_or(FinError::ArithmeticOverflow)?
            * Decimal::ONE_HUNDRED;

        // Compute max drawdown (always positive %)
        let mut max_dd = Decimal::ZERO;
        let mut peak = prices[0];
        for &p in &prices[1..] {
            if p > peak { peak = p; }
            if peak.is_zero() { continue; }
            let dd = (peak - p)
                .checked_div(peak)
                .ok_or(FinError::ArithmeticOverflow)?
                * Decimal::ONE_HUNDRED;
            if dd > max_dd { max_dd = dd; }
        }

        if max_dd.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let calmar = cum_return
            .checked_div(max_dd)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(calmar))
    }

    fn reset(&mut self) {
        self.window.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
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
    fn test_calmar_invalid_period() {
        assert!(CalmarRatio::new("c", 0).is_err());
        assert!(CalmarRatio::new("c", 1).is_err());
    }

    #[test]
    fn test_calmar_unavailable_during_warmup() {
        let mut c = CalmarRatio::new("c", 4).unwrap();
        for p in &["100", "102", "99", "103"] {
            assert_eq!(c.update_bar(&bar(p)).unwrap(), SignalValue::Unavailable);
        }
        assert!(!c.is_ready());
    }

    #[test]
    fn test_calmar_monotone_up_unavailable() {
        // Monotone rise → no drawdown → Unavailable
        let mut c = CalmarRatio::new("c", 3).unwrap();
        for p in &["100", "102", "104", "106"] {
            c.update_bar(&bar(p)).unwrap();
        }
        assert_eq!(c.update_bar(&bar("108")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_calmar_positive_with_drawdown() {
        // Net positive return with a dip in between
        let mut c = CalmarRatio::new("c", 4).unwrap();
        c.update_bar(&bar("100")).unwrap();
        c.update_bar(&bar("95")).unwrap();  // drawdown of 5%
        c.update_bar(&bar("98")).unwrap();
        c.update_bar(&bar("102")).unwrap();
        if let SignalValue::Scalar(v) = c.update_bar(&bar("110")).unwrap() {
            assert!(v > dec!(0), "net positive return with drawdown: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_calmar_reset() {
        let mut c = CalmarRatio::new("c", 3).unwrap();
        for p in &["100", "95", "98", "103"] { c.update_bar(&bar(p)).unwrap(); }
        assert!(c.is_ready());
        c.reset();
        assert!(!c.is_ready());
    }
}
