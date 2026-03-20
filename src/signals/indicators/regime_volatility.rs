//! Regime Volatility indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Regime Volatility — ratio of short-term ATR to long-term ATR.
///
/// A ratio > 1 indicates expanding volatility (potentially a regime change or
/// breakout). A ratio < 1 indicates contracting/quieter conditions.
///
/// ```text
/// ratio = ATR(fast_period) / ATR(slow_period)
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `slow_period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RegimeVolatility;
/// use fin_primitives::signals::Signal;
///
/// let rv = RegimeVolatility::new("rv", 5, 20).unwrap();
/// assert_eq!(rv.period(), 20);
/// ```
pub struct RegimeVolatility {
    name: String,
    fast_period: usize,
    slow_period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
    closes: VecDeque<Decimal>,
}

impl RegimeVolatility {
    /// Constructs a new `RegimeVolatility`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `fast_period == 0` or
    /// `fast_period >= slow_period`.
    pub fn new(
        name: impl Into<String>,
        fast_period: usize,
        slow_period: usize,
    ) -> Result<Self, FinError> {
        if fast_period == 0 || fast_period >= slow_period {
            return Err(FinError::InvalidPeriod(fast_period));
        }
        Ok(Self {
            name: name.into(),
            fast_period,
            slow_period,
            highs: VecDeque::with_capacity(slow_period + 1),
            lows: VecDeque::with_capacity(slow_period + 1),
            closes: VecDeque::with_capacity(slow_period + 2),
        })
    }

    fn atr(
        highs: &VecDeque<Decimal>,
        lows: &VecDeque<Decimal>,
        closes: &VecDeque<Decimal>,
        period: usize,
    ) -> Decimal {
        if highs.len() < period || closes.len() < 2 {
            return Decimal::ZERO;
        }
        let trs: Vec<Decimal> = highs.iter().rev().take(period)
            .zip(lows.iter().rev().take(period))
            .zip(closes.iter().rev().skip(1).take(period))
            .map(|((h, l), pc)| {
                let hl = h - l;
                let hc = (h - pc).abs();
                let lc = (l - pc).abs();
                hl.max(hc).max(lc)
            })
            .collect();
        if trs.is_empty() { return Decimal::ZERO; }
        #[allow(clippy::cast_possible_truncation)]
        let n = trs.len() as u32;
        trs.iter().sum::<Decimal>() / Decimal::from(n)
    }
}

impl Signal for RegimeVolatility {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.slow_period }
    fn is_ready(&self) -> bool { self.highs.len() >= self.slow_period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.highs.push_back(bar.high);
        if self.highs.len() > self.slow_period + 1 { self.highs.pop_front(); }
        self.lows.push_back(bar.low);
        if self.lows.len() > self.slow_period + 1 { self.lows.pop_front(); }
        self.closes.push_back(bar.close);
        if self.closes.len() > self.slow_period + 2 { self.closes.pop_front(); }

        if self.highs.len() < self.slow_period {
            return Ok(SignalValue::Unavailable);
        }

        let fast_atr = Self::atr(&self.highs, &self.lows, &self.closes, self.fast_period);
        let slow_atr = Self::atr(&self.highs, &self.lows, &self.closes, self.slow_period);

        if slow_atr.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(fast_atr / slow_atr))
    }

    fn reset(&mut self) {
        self.highs.clear();
        self.lows.clear();
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

    fn bar(h: &str, l: &str, c: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_rv_invalid_params() {
        assert!(RegimeVolatility::new("rv", 0, 10).is_err());
        assert!(RegimeVolatility::new("rv", 10, 5).is_err());
        assert!(RegimeVolatility::new("rv", 10, 10).is_err());
    }

    #[test]
    fn test_rv_unavailable_before_warm_up() {
        let mut rv = RegimeVolatility::new("rv", 3, 5).unwrap();
        for _ in 0..4 {
            assert_eq!(rv.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_rv_produces_scalar_after_warm_up() {
        let mut rv = RegimeVolatility::new("rv", 3, 5).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..8 {
            last = rv.update_bar(&bar("110", "90", "100")).unwrap();
        }
        assert!(matches!(last, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_rv_expanding_volatility() {
        // Calm bars then volatile bars — fast ATR should spike vs slow
        let mut rv = RegimeVolatility::new("rv", 3, 10).unwrap();
        for _ in 0..10 {
            rv.update_bar(&bar("101", "99", "100")).unwrap();
        }
        // Now feed very wide bars
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            last = rv.update_bar(&bar("130", "70", "100")).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(1), "fast ATR should exceed slow ATR: {}", v);
        }
    }

    #[test]
    fn test_rv_reset() {
        let mut rv = RegimeVolatility::new("rv", 3, 5).unwrap();
        for _ in 0..8 { rv.update_bar(&bar("110", "90", "100")).unwrap(); }
        assert!(rv.is_ready());
        rv.reset();
        assert!(!rv.is_ready());
    }
}
