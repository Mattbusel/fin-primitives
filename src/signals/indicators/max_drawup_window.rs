//! Max Drawup Window — maximum trough-to-peak gain over a rolling N-bar window.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Max Drawup Window — maximum `(close - trough) / trough` over the last `period` bars.
///
/// Tracks the largest cumulative gain from any local trough to any subsequent close
/// within the rolling window. Complement to max drawdown:
/// - **High value**: large upside swing within the window — strong recovery or rally.
/// - **Near 0**: prices broadly flat or declining — no meaningful rally.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period < 2`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::MaxDrawupWindow;
/// use fin_primitives::signals::Signal;
/// let mdu = MaxDrawupWindow::new("mdu_20", 20).unwrap();
/// assert_eq!(mdu.period(), 20);
/// ```
pub struct MaxDrawupWindow {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl MaxDrawupWindow {
    /// Constructs a new `MaxDrawupWindow`.
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
            closes: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for MaxDrawupWindow {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.closes.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        // Find the maximum trough-to-peak drawup: for each close, compute max gain
        // from any prior close that was lower.
        let closes: Vec<Decimal> = self.closes.iter().copied().collect();
        let mut max_drawup = Decimal::ZERO;
        let mut running_min = closes[0];

        for &c in &closes[1..] {
            if running_min < c && !running_min.is_zero() {
                let drawup = (c - running_min)
                    .checked_div(running_min)
                    .ok_or(FinError::ArithmeticOverflow)?;
                if drawup > max_drawup {
                    max_drawup = drawup;
                }
            }
            if c < running_min {
                running_min = c;
            }
        }

        Ok(SignalValue::Scalar(max_drawup))
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
    fn test_mdu_invalid_period() {
        assert!(MaxDrawupWindow::new("mdu", 0).is_err());
        assert!(MaxDrawupWindow::new("mdu", 1).is_err());
    }

    #[test]
    fn test_mdu_unavailable_before_period() {
        let mut s = MaxDrawupWindow::new("mdu", 3).unwrap();
        assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
        assert!(!s.is_ready());
    }

    #[test]
    fn test_mdu_uptrend_gives_full_gain() {
        // 100 → 110: max drawup = 10/100 = 0.1
        let mut s = MaxDrawupWindow::new("mdu", 2).unwrap();
        s.update_bar(&bar("100")).unwrap();
        if let SignalValue::Scalar(v) = s.update_bar(&bar("110")).unwrap() {
            assert!((v - dec!(0.1)).abs() < dec!(0.001), "drawup should be 0.1: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_mdu_downtrend_gives_zero() {
        // 110 → 100: drawup = 0 (no trough-to-peak gain)
        let mut s = MaxDrawupWindow::new("mdu", 2).unwrap();
        s.update_bar(&bar("110")).unwrap();
        if let SignalValue::Scalar(v) = s.update_bar(&bar("100")).unwrap() {
            assert_eq!(v, dec!(0), "pure downtrend drawup should be 0: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_mdu_non_negative() {
        let mut s = MaxDrawupWindow::new("mdu", 3).unwrap();
        let prices = ["105","95","100","90","110","80","120"];
        for p in &prices {
            if let SignalValue::Scalar(v) = s.update_bar(&bar(p)).unwrap() {
                assert!(v >= dec!(0), "drawup should be non-negative: {v}");
            }
        }
    }

    #[test]
    fn test_mdu_reset() {
        let mut s = MaxDrawupWindow::new("mdu", 2).unwrap();
        s.update_bar(&bar("100")).unwrap();
        s.update_bar(&bar("110")).unwrap();
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
