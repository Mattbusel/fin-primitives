//! Average Up Return indicator.
//!
//! Rolling mean of close-to-close returns on bars where the close exceeded
//! the prior close. Measures the typical gain magnitude on winning bars.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Average Up Return — rolling mean of returns on up bars only.
///
/// A bar is an "up bar" when `close[i] > close[i-1]`. Its return is:
/// ```text
/// ret[i] = (close[i] - close[i-1]) / close[i-1] × 100
/// ```
///
/// Only up-bar returns are averaged. If there are no up bars in the window,
/// returns [`SignalValue::Unavailable`].
///
/// Use alongside `AvgDownReturn` and `GainToLossRatio` to assess the
/// asymmetry of wins vs losses.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen
/// (i.e., `period` return observations, including both up and flat/down bars),
/// OR when no up bars exist in the current window.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::AvgUpReturn;
/// use fin_primitives::signals::Signal;
/// let aur = AvgUpReturn::new("aur_20", 20).unwrap();
/// assert_eq!(aur.period(), 20);
/// ```
pub struct AvgUpReturn {
    name: String,
    period: usize,
    /// Sliding window of (return_pct, is_up) pairs
    window: VecDeque<(f64, bool)>,
    prev_close: Option<f64>,
}

impl AvgUpReturn {
    /// Constructs a new `AvgUpReturn`.
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
            window: VecDeque::with_capacity(period),
            prev_close: None,
        })
    }
}

impl Signal for AvgUpReturn {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        use rust_decimal::prelude::ToPrimitive;

        let c = bar.close.to_f64().unwrap_or(0.0);
        if let Some(pc) = self.prev_close {
            if pc > 0.0 {
                let ret = (c - pc) / pc * 100.0;
                let is_up = c > pc;
                self.window.push_back((ret, is_up));
                if self.window.len() > self.period {
                    self.window.pop_front();
                }
            }
        }
        self.prev_close = Some(c);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let up_rets: Vec<f64> = self.window.iter()
            .filter(|(_, is_up)| *is_up)
            .map(|(r, _)| *r)
            .collect();

        if up_rets.is_empty() {
            return Ok(SignalValue::Unavailable);
        }

        let avg = up_rets.iter().sum::<f64>() / up_rets.len() as f64;
        Decimal::try_from(avg)
            .map(SignalValue::Scalar)
            .map_err(|_| FinError::ArithmeticOverflow)
    }

    fn reset(&mut self) {
        self.window.clear();
        self.prev_close = None;
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
    fn test_aur_invalid_period() {
        assert!(AvgUpReturn::new("aur", 0).is_err());
    }

    #[test]
    fn test_aur_unavailable_during_warmup() {
        let mut aur = AvgUpReturn::new("aur", 3).unwrap();
        aur.update_bar(&bar("100")).unwrap();
        aur.update_bar(&bar("102")).unwrap();
        aur.update_bar(&bar("104")).unwrap();
        assert!(!aur.is_ready());
    }

    #[test]
    fn test_aur_no_up_bars_unavailable() {
        // All down bars → no up returns → Unavailable
        let mut aur = AvgUpReturn::new("aur", 3).unwrap();
        aur.update_bar(&bar("110")).unwrap();
        aur.update_bar(&bar("108")).unwrap();
        aur.update_bar(&bar("106")).unwrap();
        assert_eq!(aur.update_bar(&bar("104")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_aur_all_up_bars_positive() {
        // All up bars → avg of all returns
        let mut aur = AvgUpReturn::new("aur", 3).unwrap();
        aur.update_bar(&bar("100")).unwrap();
        aur.update_bar(&bar("102")).unwrap();
        aur.update_bar(&bar("104")).unwrap();
        if let SignalValue::Scalar(v) = aur.update_bar(&bar("106")).unwrap() {
            assert!(v > dec!(0), "all up bars → positive avg up return: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_aur_mixed_bars() {
        // Window: up, down, up → average of 2 up returns
        let mut aur = AvgUpReturn::new("aur", 3).unwrap();
        aur.update_bar(&bar("100")).unwrap();
        aur.update_bar(&bar("110")).unwrap(); // up: +10%
        aur.update_bar(&bar("105")).unwrap(); // down
        if let SignalValue::Scalar(v) = aur.update_bar(&bar("115.5")).unwrap() {
            // up: (115.5-105)/105 * 100 ≈ 10%; window=[10,-4.55,10] → avg up = (10+10)/2 = 10
            assert!(v > dec!(0), "mixed bars → positive avg: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_aur_reset() {
        let mut aur = AvgUpReturn::new("aur", 2).unwrap();
        aur.update_bar(&bar("100")).unwrap();
        aur.update_bar(&bar("105")).unwrap();
        aur.update_bar(&bar("110")).unwrap();
        assert!(aur.is_ready());
        aur.reset();
        assert!(!aur.is_ready());
    }
}
