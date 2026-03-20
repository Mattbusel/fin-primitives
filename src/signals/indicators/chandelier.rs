//! Chandelier Exit indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Chandelier Exit — dynamic trailing stop based on ATR.
///
/// `Long Exit  = highest_high(period) - multiplier × ATR(period)`
/// `Short Exit = lowest_low(period)  + multiplier × ATR(period)`
///
/// This indicator emits the **long** exit level. Use the negated stop for shorts.
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ChandelierExit;
/// use fin_primitives::signals::Signal;
///
/// let mut ce = ChandelierExit::new("ce22", 22, rust_decimal_macros::dec!(3)).unwrap();
/// ```
pub struct ChandelierExit {
    name: String,
    period: usize,
    multiplier: Decimal,
    history: VecDeque<BarInput>,
    // ATR state
    prev_close: Option<Decimal>,
    atr: Option<Decimal>,
    atr_count: usize,
    atr_seed_sum: Decimal,
    atr_k: Decimal,
}

impl ChandelierExit {
    /// Constructs a new `ChandelierExit`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize, multiplier: Decimal) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        #[allow(clippy::cast_possible_truncation)]
        let denom = Decimal::from((period + 1) as u32);
        let atr_k = Decimal::TWO.checked_div(denom).unwrap_or(Decimal::ONE);
        Ok(Self {
            name: name.into(),
            period,
            multiplier,
            history: VecDeque::with_capacity(period),
            prev_close: None,
            atr: None,
            atr_count: 0,
            atr_seed_sum: Decimal::ZERO,
            atr_k,
        })
    }
}

impl Signal for ChandelierExit {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        // True range
        let tr = if let Some(pc) = self.prev_close {
            let hl = bar.high - bar.low;
            let hpc = (bar.high - pc).abs();
            let lpc = (bar.low - pc).abs();
            hl.max(hpc).max(lpc)
        } else {
            bar.high - bar.low
        };
        self.prev_close = Some(bar.close);

        // ATR via EMA seeded with SMA
        self.atr_count += 1;
        if self.atr_count <= self.period {
            self.atr_seed_sum += tr;
            if self.atr_count == self.period {
                #[allow(clippy::cast_possible_truncation)]
                let seed = self.atr_seed_sum / Decimal::from(self.period as u32);
                self.atr = Some(seed);
            }
        } else if let Some(prev_atr) = self.atr {
            let one_minus_k = Decimal::ONE - self.atr_k;
            self.atr = Some(tr * self.atr_k + prev_atr * one_minus_k);
        }

        // Maintain high/low window
        self.history.push_back(*bar);
        if self.history.len() > self.period {
            self.history.pop_front();
        }

        if self.history.len() < self.period || self.atr.is_none() {
            return Ok(SignalValue::Unavailable);
        }

        let highest_high = self
            .history
            .iter()
            .map(|b| b.high)
            .fold(Decimal::MIN, Decimal::max);
        let long_exit = highest_high - self.multiplier * self.atr.unwrap();
        Ok(SignalValue::Scalar(long_exit))
    }

    fn is_ready(&self) -> bool {
        self.history.len() >= self.period && self.atr.is_some()
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.history.clear();
        self.prev_close = None;
        self.atr = None;
        self.atr_count = 0;
        self.atr_seed_sum = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, h: &str, l: &str, c: &str) -> OhlcvBar {
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(o.parse().unwrap()).unwrap(),
            high: Price::new(h.parse().unwrap()).unwrap(),
            low: Price::new(l.parse().unwrap()).unwrap(),
            close: Price::new(c.parse().unwrap()).unwrap(),
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_chandelier_period_0_error() {
        assert!(ChandelierExit::new("ce", 0, dec!(3)).is_err());
    }

    #[test]
    fn test_chandelier_unavailable_before_period() {
        let mut ce = ChandelierExit::new("ce3", 3, dec!(2)).unwrap();
        assert_eq!(ce.update_bar(&bar("100", "110", "90", "105")).unwrap(), SignalValue::Unavailable);
        assert_eq!(ce.update_bar(&bar("105", "115", "95", "110")).unwrap(), SignalValue::Unavailable);
        assert!(ce.update_bar(&bar("110", "120", "100", "115")).unwrap().is_scalar());
    }

    #[test]
    fn test_chandelier_long_exit_below_highest_high() {
        let mut ce = ChandelierExit::new("ce3", 3, dec!(1)).unwrap();
        ce.update_bar(&bar("100", "110", "90", "105")).unwrap();
        ce.update_bar(&bar("105", "115", "95", "110")).unwrap();
        let v = ce.update_bar(&bar("110", "120", "100", "115")).unwrap();
        // long exit = highest_high - 1 * ATR < highest_high
        match v {
            SignalValue::Scalar(d) => assert!(d < dec!(120), "exit should be below highest high"),
            _ => panic!("expected Scalar"),
        }
    }

    #[test]
    fn test_chandelier_reset_clears_state() {
        let mut ce = ChandelierExit::new("ce3", 3, dec!(2)).unwrap();
        for _ in 0..5 {
            ce.update_bar(&bar("100", "110", "90", "105")).unwrap();
        }
        assert!(ce.is_ready());
        ce.reset();
        assert!(!ce.is_ready());
    }
}
