//! Trend Strength Index indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Trend Strength Index (TSI) — measures the ratio of the net directional move
/// to the total path length over a rolling window.
///
/// ```text
/// net_move   = |close[t] - close[t-period]|
/// path       = sum(|close[i] - close[i-1]|, i in [t-period+1, t])
/// tsi        = net_move / path × 100
/// ```
///
/// - **100**: price moved in a perfectly straight line (pure trend, zero zigzag).
/// - **0**: total path was taken but net displacement is zero (no net trend).
/// - **High value (> 60)**: strong directional move with little noise.
/// - **Low value (< 30)**: choppy, directionless price action.
///
/// Note: This is a different calculation from the `Tsi` (True Strength Index /
/// momentum-based), which uses EMA-smoothed momentum.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen,
/// or when the total path is zero (all prices flat).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::TrendStrengthIndex;
/// use fin_primitives::signals::Signal;
/// let tsi = TrendStrengthIndex::new("tsi_10", 10).unwrap();
/// assert_eq!(tsi.period(), 10);
/// ```
pub struct TrendStrengthIndex {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl TrendStrengthIndex {
    /// Constructs a new `TrendStrengthIndex`.
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
            closes: VecDeque::with_capacity(period + 1),
        })
    }
}

impl Signal for TrendStrengthIndex {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.closes.len() > self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() <= self.period {
            return Ok(SignalValue::Unavailable);
        }

        let prices: Vec<Decimal> = self.closes.iter().copied().collect();
        let first = prices[0];
        let last = *prices.last().unwrap();

        let net_move = (last - first).abs();
        let path: Decimal = prices.windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .sum();

        if path.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let tsi = net_move
            .checked_div(path)
            .ok_or(FinError::ArithmeticOverflow)?
            * Decimal::ONE_HUNDRED;

        Ok(SignalValue::Scalar(tsi))
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
    fn test_tsi_invalid_period() {
        assert!(TrendStrengthIndex::new("tsi", 0).is_err());
    }

    #[test]
    fn test_tsi_unavailable_during_warmup() {
        let mut tsi = TrendStrengthIndex::new("tsi", 3).unwrap();
        for p in &["100", "101", "102"] {
            assert_eq!(tsi.update_bar(&bar(p)).unwrap(), SignalValue::Unavailable);
        }
        assert!(!tsi.is_ready());
    }

    #[test]
    fn test_tsi_straight_trend_is_100() {
        // Perfectly linear uptrend: net = path → TSI = 100
        let mut tsi = TrendStrengthIndex::new("tsi", 4).unwrap();
        for p in &["100", "101", "102", "103", "104"] {
            tsi.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(v) = tsi.update_bar(&bar("105")).unwrap() {
            assert_eq!(v, dec!(100), "straight trend → TSI = 100");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_tsi_choppy_low() {
        // Exact zigzag that returns to start within the window: net=0, path=large → TSI=0
        // period=4 → window of 5 closes; push 5 bars where last equals first
        let mut tsi = TrendStrengthIndex::new("tsi", 4).unwrap();
        // 100 → 110 → 90 → 110 → 100: window [100,110,90,110,100], net=0, path=40 → TSI=0
        for p in &["100", "110", "90", "110", "100"] {
            tsi.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(v) = tsi.update_bar(&bar("100")).unwrap() {
            // window shifts to [110,90,110,100,100]: net=|100-110|=10, path=20+20+10+0=50 → TSI=20
            assert!(v <= dec!(25), "choppy/no-net-trend prices → low TSI: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_tsi_flat_unavailable() {
        // Flat prices → path = 0 → Unavailable
        let mut tsi = TrendStrengthIndex::new("tsi", 3).unwrap();
        for _ in 0..5 {
            tsi.update_bar(&bar("100")).unwrap();
        }
        assert_eq!(tsi.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_tsi_reset() {
        let mut tsi = TrendStrengthIndex::new("tsi", 3).unwrap();
        for p in &["100", "101", "102", "103"] { tsi.update_bar(&bar(p)).unwrap(); }
        assert!(tsi.is_ready());
        tsi.reset();
        assert!(!tsi.is_ready());
    }
}
