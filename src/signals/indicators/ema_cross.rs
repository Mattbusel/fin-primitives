//! EMA Crossover signal indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// EMA Crossover — produces a directional signal based on fast/slow EMA crossovers.
///
/// Returns:
/// * `+100` when the fast EMA crosses above the slow EMA (bullish crossover)
/// * `-100` when the fast EMA crosses below the slow EMA (bearish crossover)
/// * `0` when no crossover occurred this bar
///
/// Both EMAs are seeded with their respective SMA averages.
///
/// Returns [`SignalValue::Unavailable`] until the slow EMA has seeded.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::EmaCross;
/// use fin_primitives::signals::Signal;
///
/// let ec = EmaCross::new("ec", 5, 20).unwrap();
/// assert_eq!(ec.period(), 20);
/// ```
pub struct EmaCross {
    name: String,
    fast_period: usize,
    slow_period: usize,
    fast_k: Decimal,
    slow_k: Decimal,
    fast_seed: VecDeque<Decimal>,
    fast_ema: Option<Decimal>,
    slow_seed: VecDeque<Decimal>,
    slow_ema: Option<Decimal>,
    prev_fast: Option<Decimal>,
    prev_slow: Option<Decimal>,
}

impl EmaCross {
    /// Creates a new `EmaCross`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if either period is zero or fast >= slow.
    pub fn new(name: impl Into<String>, fast: usize, slow: usize) -> Result<Self, FinError> {
        if fast == 0 { return Err(FinError::InvalidPeriod(fast)); }
        if slow == 0 { return Err(FinError::InvalidPeriod(slow)); }
        if fast >= slow { return Err(FinError::InvalidPeriod(fast)); }
        #[allow(clippy::cast_possible_truncation)]
        let fast_k = Decimal::TWO / Decimal::from((fast + 1) as u32);
        #[allow(clippy::cast_possible_truncation)]
        let slow_k = Decimal::TWO / Decimal::from((slow + 1) as u32);
        Ok(Self {
            name: name.into(),
            fast_period: fast,
            slow_period: slow,
            fast_k,
            slow_k,
            fast_seed: VecDeque::with_capacity(fast),
            fast_ema: None,
            slow_seed: VecDeque::with_capacity(slow),
            slow_ema: None,
            prev_fast: None,
            prev_slow: None,
        })
    }

    /// Returns the current fast EMA value.
    pub fn fast_ema(&self) -> Option<Decimal> { self.fast_ema }
    /// Returns the current slow EMA value.
    pub fn slow_ema(&self) -> Option<Decimal> { self.slow_ema }
}

impl Signal for EmaCross {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let close = bar.close;

        // Update fast EMA
        let fast_val = match self.fast_ema {
            None => {
                self.fast_seed.push_back(close);
                if self.fast_seed.len() >= self.fast_period {
                    let seed: Decimal = self.fast_seed.iter().sum::<Decimal>()
                        / Decimal::from(self.fast_period as u32);
                    self.fast_ema = Some(seed);
                    seed
                } else {
                    // Not ready yet; update slow too
                    self.slow_seed.push_back(close);
                    if self.slow_seed.len() > self.slow_period { self.slow_seed.pop_front(); }
                    return Ok(SignalValue::Unavailable);
                }
            }
            Some(prev) => {
                let v = close * self.fast_k + prev * (Decimal::ONE - self.fast_k);
                self.fast_ema = Some(v);
                v
            }
        };

        // Update slow EMA
        let slow_val = match self.slow_ema {
            None => {
                self.slow_seed.push_back(close);
                if self.slow_seed.len() < self.slow_period {
                    self.prev_fast = Some(fast_val);
                    return Ok(SignalValue::Unavailable);
                }
                let seed: Decimal = self.slow_seed.iter().sum::<Decimal>()
                    / Decimal::from(self.slow_period as u32);
                self.slow_ema = Some(seed);
                seed
            }
            Some(prev) => {
                let v = close * self.slow_k + prev * (Decimal::ONE - self.slow_k);
                self.slow_ema = Some(v);
                v
            }
        };

        // Detect crossover
        let signal = match (self.prev_fast, self.prev_slow) {
            (Some(pf), Some(ps)) => {
                let was_above = pf > ps;
                let now_above = fast_val > slow_val;
                if !was_above && now_above {
                    Decimal::from(100u32)   // bullish cross
                } else if was_above && !now_above {
                    Decimal::from(-100i32)  // bearish cross
                } else {
                    Decimal::ZERO
                }
            }
            _ => Decimal::ZERO,
        };

        self.prev_fast = Some(fast_val);
        self.prev_slow = Some(slow_val);
        Ok(SignalValue::Scalar(signal))
    }

    fn is_ready(&self) -> bool {
        self.slow_ema.is_some()
    }

    fn period(&self) -> usize {
        self.slow_period
    }

    fn reset(&mut self) {
        self.fast_seed.clear();
        self.fast_ema = None;
        self.slow_seed.clear();
        self.slow_ema = None;
        self.prev_fast = None;
        self.prev_slow = None;
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
    fn test_ema_cross_invalid() {
        assert!(EmaCross::new("e", 0, 20).is_err());
        assert!(EmaCross::new("e", 20, 5).is_err()); // fast >= slow
        assert!(EmaCross::new("e", 5, 5).is_err());  // equal
    }

    #[test]
    fn test_ema_cross_unavailable_before_slow() {
        let mut ec = EmaCross::new("e", 3, 5).unwrap();
        for _ in 0..4 {
            assert_eq!(ec.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_ema_cross_no_cross_flat() {
        // Flat price → EMAs converge, no crossover
        let mut ec = EmaCross::new("e", 3, 5).unwrap();
        for _ in 0..20 { ec.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = ec.update_bar(&bar("100")).unwrap() {
            assert_eq!(v, dec!(0));
        }
    }

    #[test]
    fn test_ema_cross_detects_bullish_cross() {
        // Start low, then spike — fast EMA should eventually cross above slow
        let mut ec = EmaCross::new("e", 3, 10).unwrap();
        for _ in 0..10 { ec.update_bar(&bar("100")).unwrap(); }
        // Now spike high prices to force fast EMA above slow
        let mut found_cross = false;
        for _ in 0..10 {
            if let SignalValue::Scalar(v) = ec.update_bar(&bar("200")).unwrap() {
                if v == dec!(100) { found_cross = true; }
            }
        }
        assert!(found_cross, "Expected bullish crossover signal");
    }

    #[test]
    fn test_ema_cross_reset() {
        let mut ec = EmaCross::new("e", 3, 5).unwrap();
        for _ in 0..10 { ec.update_bar(&bar("100")).unwrap(); }
        assert!(ec.is_ready());
        ec.reset();
        assert!(!ec.is_ready());
    }
}
