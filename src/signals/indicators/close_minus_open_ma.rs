//! Close Minus Open MA indicator -- SMA of bar body (close - open) over N bars.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Close Minus Open MA -- rolling SMA of the signed bar body `(close - open)`.
///
/// Positive values indicate net bullish pressure over the window;
/// negative values indicate net bearish pressure.
///
/// ```text
/// body[t]    = close[t] - open[t]
/// cmo_ma[t]  = SMA(body, period)
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseMinusOpenMa;
/// use fin_primitives::signals::Signal;
/// let c = CloseMinusOpenMa::new("cmo_ma", 10).unwrap();
/// assert_eq!(c.period(), 10);
/// ```
pub struct CloseMinusOpenMa {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl CloseMinusOpenMa {
    /// Constructs a new `CloseMinusOpenMa`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for CloseMinusOpenMa {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let body = bar.net_move();
        self.window.push_back(body);
        self.sum += body;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() { self.sum -= old; }
        }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        #[allow(clippy::cast_possible_truncation)]
        let avg = self.sum / Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(avg))
    }

    fn reset(&mut self) {
        self.window.clear();
        self.sum = Decimal::ZERO;
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
        let cp = Price::new(c.parse().unwrap()).unwrap();
        let high = if cp.value() > op.value() { cp } else { op };
        let low  = if cp.value() < op.value() { cp } else { op };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high, low, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_cmo_ma_period_0_error() { assert!(CloseMinusOpenMa::new("c", 0).is_err()); }

    #[test]
    fn test_cmo_ma_unavailable_before_period() {
        let mut c = CloseMinusOpenMa::new("c", 3).unwrap();
        assert_eq!(c.update_bar(&bar("100", "105")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_cmo_ma_all_bullish() {
        // body = 5 each bar
        let mut c = CloseMinusOpenMa::new("c", 3).unwrap();
        c.update_bar(&bar("100", "105")).unwrap();
        c.update_bar(&bar("100", "105")).unwrap();
        let v = c.update_bar(&bar("100", "105")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(5)));
    }

    #[test]
    fn test_cmo_ma_mixed_near_zero() {
        let mut c = CloseMinusOpenMa::new("c", 2).unwrap();
        c.update_bar(&bar("100", "110")).unwrap(); // body=+10
        let v = c.update_bar(&bar("110", "100")).unwrap(); // body=-10, avg=0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cmo_ma_reset() {
        let mut c = CloseMinusOpenMa::new("c", 2).unwrap();
        c.update_bar(&bar("100", "105")).unwrap();
        c.update_bar(&bar("100", "105")).unwrap();
        assert!(c.is_ready());
        c.reset();
        assert!(!c.is_ready());
    }
}
