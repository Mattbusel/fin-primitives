//! Max Drawdown Window indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Max Drawdown Window — maximum peak-to-trough drawdown percentage within a window.
///
/// ```text
/// For each bar in the window:
///   running_max = max(close, 0..i)
///   drawdown_i  = (running_max − close_i) / running_max × 100
/// output = max(drawdown, window)
/// ```
///
/// Higher values indicate a more significant drawdown occurred recently.
/// Returns 0 for monotonically rising or flat price series.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::MaxDrawdownWindow;
/// use fin_primitives::signals::Signal;
///
/// let mdw = MaxDrawdownWindow::new("mdw", 20).unwrap();
/// assert_eq!(mdw.period(), 20);
/// ```
pub struct MaxDrawdownWindow {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl MaxDrawdownWindow {
    /// Creates a new `MaxDrawdownWindow`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            closes: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for MaxDrawdownWindow {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period { self.closes.pop_front(); }
        if self.closes.len() < self.period { return Ok(SignalValue::Unavailable); }

        let mut running_max = self.closes[0];
        let mut max_dd = Decimal::ZERO;

        for &c in &self.closes {
            if c > running_max { running_max = c; }
            if !running_max.is_zero() {
                let dd = (running_max - c) / running_max * Decimal::from(100u32);
                if dd > max_dd { max_dd = dd; }
            }
        }

        Ok(SignalValue::Scalar(max_dd))
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
    fn test_mdw_invalid() {
        assert!(MaxDrawdownWindow::new("m", 0).is_err());
        assert!(MaxDrawdownWindow::new("m", 1).is_err());
    }

    #[test]
    fn test_mdw_unavailable_before_warmup() {
        let mut m = MaxDrawdownWindow::new("m", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(m.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_mdw_flat_is_zero() {
        let mut m = MaxDrawdownWindow::new("m", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = m.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_mdw_rising_is_zero() {
        let mut m = MaxDrawdownWindow::new("m", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0u32..8 {
            let p = format!("{}", 100 + i);
            last = m.update_bar(&bar(&p)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_mdw_drawdown_positive() {
        // 100→110→90: peak=110, trough=90 → dd = (110-90)/110 ≈ 18.18%
        let mut m = MaxDrawdownWindow::new("m", 3).unwrap();
        m.update_bar(&bar("100")).unwrap();
        m.update_bar(&bar("110")).unwrap();
        if let SignalValue::Scalar(v) = m.update_bar(&bar("90")).unwrap() {
            let expected = (dec!(110) - dec!(90)) / dec!(110) * dec!(100);
            let diff = (v - expected).abs();
            assert!(diff < dec!(0.001), "expected {expected}, got {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_mdw_reset() {
        let mut m = MaxDrawdownWindow::new("m", 3).unwrap();
        for _ in 0..5 { m.update_bar(&bar("100")).unwrap(); }
        assert!(m.is_ready());
        m.reset();
        assert!(!m.is_ready());
    }
}
