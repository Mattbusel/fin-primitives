//! Schaff Trend Cycle (STC) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Schaff Trend Cycle — MACD values passed through a double Stochastic filter.
///
/// ```text
/// 1. MACD line  = EMA(close, fast) - EMA(close, slow)
/// 2. First  %K  = Stochastic of MACD over `stoch_period` bars
/// 3. First  %D  = EMA(%K, factor)          (Schaff smoothing factor)
/// 4. Second %K  = Stochastic of %D over `stoch_period` bars
/// 5. STC        = EMA(second %K, factor)
/// ```
///
/// Defaults: `fast=23`, `slow=50`, `stoch_period=10`, `factor=0.5`.
/// Returns [`SignalValue::Unavailable`] until enough bars have accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Stc;
/// use fin_primitives::signals::Signal;
///
/// let stc = Stc::new("stc", 23, 50, 10, "0.5".parse().unwrap()).unwrap();
/// assert_eq!(stc.period(), 50);
/// ```
pub struct Stc {
    name: String,
    fast: usize,
    slow: usize,
    stoch_period: usize,
    factor: Decimal,
    // EMA state
    ema_fast: Option<Decimal>,
    ema_slow: Option<Decimal>,
    fast_k: Decimal,
    slow_k: Decimal,
    count: usize,
    // Rolling MACD window for stoch 1
    macd_window: VecDeque<Decimal>,
    // Rolling %D window for stoch 2
    d_window: VecDeque<Decimal>,
    // Smoothed values
    pct_d: Option<Decimal>,
    stc: Option<Decimal>,
}

impl Stc {
    /// Constructs a new `Stc`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if any period is 0, or if `fast >= slow`.
    pub fn new(
        name: impl Into<String>,
        fast: usize,
        slow: usize,
        stoch_period: usize,
        factor: Decimal,
    ) -> Result<Self, FinError> {
        if fast == 0 { return Err(FinError::InvalidPeriod(fast)); }
        if slow == 0 { return Err(FinError::InvalidPeriod(slow)); }
        if stoch_period == 0 { return Err(FinError::InvalidPeriod(stoch_period)); }
        if fast >= slow {
            return Err(FinError::InvalidInput(format!(
                "fast ({fast}) must be < slow ({slow})"
            )));
        }
        Ok(Self {
            name: name.into(),
            fast,
            slow,
            stoch_period,
            factor,
            ema_fast: None,
            ema_slow: None,
            fast_k: Decimal::from(2u32) / Decimal::from((fast + 1) as u32),
            slow_k: Decimal::from(2u32) / Decimal::from((slow + 1) as u32),
            count: 0,
            macd_window: VecDeque::with_capacity(stoch_period),
            d_window: VecDeque::with_capacity(stoch_period),
            pct_d: None,
            stc: None,
        })
    }

    /// Returns the fast EMA period.
    pub fn fast_period(&self) -> usize { self.fast }

    fn stochastic_k(window: &VecDeque<Decimal>, current: Decimal) -> Decimal {
        let high = window.iter().copied().fold(current, Decimal::max);
        let low  = window.iter().copied().fold(current, Decimal::min);
        let range = high - low;
        if range.is_zero() {
            return Decimal::ZERO;
        }
        (current - low) / range * Decimal::ONE_HUNDRED
    }
}

impl Signal for Stc {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let close = bar.close;
        self.count += 1;

        // Update EMAs
        self.ema_fast = Some(match self.ema_fast {
            None => close,
            Some(prev) => prev + self.fast_k * (close - prev),
        });
        self.ema_slow = Some(match self.ema_slow {
            None => close,
            Some(prev) => prev + self.slow_k * (close - prev),
        });

        if self.count < self.slow {
            return Ok(SignalValue::Unavailable);
        }

        let macd = self.ema_fast.unwrap_or(close) - self.ema_slow.unwrap_or(close);

        // Stochastic 1: %K of MACD
        self.macd_window.push_back(macd);
        if self.macd_window.len() > self.stoch_period {
            self.macd_window.pop_front();
        }

        if self.macd_window.len() < self.stoch_period {
            return Ok(SignalValue::Unavailable);
        }

        let k1 = Self::stochastic_k(&self.macd_window, macd);

        // Smooth %K → %D via EMA(factor)
        self.pct_d = Some(match self.pct_d {
            None => k1,
            Some(prev) => prev + self.factor * (k1 - prev),
        });
        let pct_d = self.pct_d.unwrap();

        // Stochastic 2: %K of %D
        self.d_window.push_back(pct_d);
        if self.d_window.len() > self.stoch_period {
            self.d_window.pop_front();
        }

        let k2 = Self::stochastic_k(&self.d_window, pct_d);

        // Final STC = EMA(factor) of second %K
        self.stc = Some(match self.stc {
            None => k2,
            Some(prev) => prev + self.factor * (k2 - prev),
        });

        Ok(SignalValue::Scalar(self.stc.unwrap()))
    }

    fn is_ready(&self) -> bool {
        self.stc.is_some()
    }

    fn period(&self) -> usize {
        self.slow
    }

    fn reset(&mut self) {
        self.ema_fast = None;
        self.ema_slow = None;
        self.count = 0;
        self.macd_window.clear();
        self.d_window.clear();
        self.pct_d = None;
        self.stc = None;
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
        let cl = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cl, high: cl, low: cl, close: cl,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_stc_invalid_period() {
        assert!(Stc::new("s", 0, 50, 10, dec!(0.5)).is_err());
        assert!(Stc::new("s", 50, 23, 10, dec!(0.5)).is_err()); // fast >= slow
    }

    #[test]
    fn test_stc_unavailable_initially() {
        let mut stc = Stc::new("s", 5, 10, 3, dec!(0.5)).unwrap();
        for _ in 0..10 {
            assert_eq!(stc.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_stc_ready_after_warmup() {
        let mut stc = Stc::new("s", 5, 10, 3, dec!(0.5)).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..30 {
            last = stc.update_bar(&bar("100")).unwrap();
        }
        assert!(stc.is_ready());
        assert!(matches!(last, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_stc_reset() {
        let mut stc = Stc::new("s", 5, 10, 3, dec!(0.5)).unwrap();
        for _ in 0..30 { stc.update_bar(&bar("100")).unwrap(); }
        assert!(stc.is_ready());
        stc.reset();
        assert!(!stc.is_ready());
    }
}
