//! Trend Strength indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Trend Strength — measures how directionally "efficient" price movement is over a window.
///
/// ```text
/// trend_strength = |close[now] - close[now-period]| / Σ|close[i] - close[i-1]|  × 100
/// ```
///
/// A value of 100 means prices moved in a perfectly straight line (maximum trend efficiency).
/// A value near 0 means prices oscillated without net progress (choppy market).
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen
/// or total path length is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::TrendStrength;
/// use fin_primitives::signals::Signal;
///
/// let ts = TrendStrength::new("ts", 10).unwrap();
/// assert_eq!(ts.period(), 10);
/// ```
pub struct TrendStrength {
    name: String,
    period: usize,
    history: VecDeque<Decimal>,
}

impl TrendStrength {
    /// Creates a new `TrendStrength`.
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
            history: VecDeque::with_capacity(period + 1),
        })
    }
}

impl Signal for TrendStrength {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.history.push_back(bar.close);
        if self.history.len() > self.period + 1 {
            self.history.pop_front();
        }
        if self.history.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }

        let closes: Vec<Decimal> = self.history.iter().copied().collect();
        let net = (closes[self.period] - closes[0]).abs();
        let path: Decimal = closes.windows(2).map(|w| (w[1] - w[0]).abs()).sum();

        if path.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let strength = net
            .checked_div(path)
            .ok_or(FinError::ArithmeticOverflow)?
            * Decimal::from(100u32);

        Ok(SignalValue::Scalar(strength))
    }

    fn is_ready(&self) -> bool {
        self.history.len() >= self.period + 1
    }

    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.history.clear();
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
    fn test_ts_invalid_period() {
        assert!(TrendStrength::new("t", 0).is_err());
    }

    #[test]
    fn test_ts_unavailable_early() {
        let mut ts = TrendStrength::new("t", 3).unwrap();
        assert_eq!(ts.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(ts.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
        assert_eq!(ts.update_bar(&bar("102")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_ts_perfect_trend_is_100() {
        // Perfectly linear: 100, 101, 102, 103 — net == path
        let mut ts = TrendStrength::new("t", 3).unwrap();
        ts.update_bar(&bar("100")).unwrap();
        ts.update_bar(&bar("101")).unwrap();
        ts.update_bar(&bar("102")).unwrap();
        if let SignalValue::Scalar(v) = ts.update_bar(&bar("103")).unwrap() {
            assert_eq!(v, dec!(100));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_ts_choppy_less_than_100() {
        // Up then down then up: net < path
        let mut ts = TrendStrength::new("t", 3).unwrap();
        ts.update_bar(&bar("100")).unwrap();
        ts.update_bar(&bar("110")).unwrap();
        ts.update_bar(&bar("90")).unwrap();
        if let SignalValue::Scalar(v) = ts.update_bar(&bar("105")).unwrap() {
            assert!(v < dec!(100), "choppy should be < 100: {v}");
            assert!(v > dec!(0), "some net movement: {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_ts_flat_unavailable() {
        let mut ts = TrendStrength::new("t", 2).unwrap();
        ts.update_bar(&bar("100")).unwrap();
        ts.update_bar(&bar("100")).unwrap();
        assert_eq!(ts.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_ts_reset() {
        let mut ts = TrendStrength::new("t", 2).unwrap();
        for p in &["100", "101", "102"] { ts.update_bar(&bar(p)).unwrap(); }
        assert!(ts.is_ready());
        ts.reset();
        assert!(!ts.is_ready());
    }
}
