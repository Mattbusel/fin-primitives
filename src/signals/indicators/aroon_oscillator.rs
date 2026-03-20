//! Aroon Oscillator indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Aroon Oscillator — single-value measure of trend direction and strength.
///
/// ```text
/// Aroon Up   = (period - bars_since_period_high) / period * 100
/// Aroon Down = (period - bars_since_period_low)  / period * 100
/// Aroon Osc  = Aroon Up - Aroon Down
/// ```
///
/// Range: -100 (strong downtrend) to +100 (strong uptrend).
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::AroonOscillator;
/// use fin_primitives::signals::Signal;
///
/// let ao = AroonOscillator::new("aroon_osc", 14).unwrap();
/// assert_eq!(ao.period(), 14);
/// ```
pub struct AroonOscillator {
    name: String,
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
}

impl AroonOscillator {
    /// Constructs a new `AroonOscillator`.
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
            highs: VecDeque::with_capacity(period + 1),
            lows: VecDeque::with_capacity(period + 1),
        })
    }
}

impl Signal for AroonOscillator {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.highs.len() >= self.period + 1
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);

        if self.highs.len() > self.period + 1 {
            self.highs.pop_front();
            self.lows.pop_front();
        }

        if self.highs.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }

        // bars_since_high: position from the end (0 = current bar, period = oldest bar)
        let bars_since_high = self
            .highs
            .iter()
            .rev()
            .enumerate()
            .max_by_key(|(_, &h)| h)
            .map(|(i, _)| i)
            .unwrap_or(0);

        let bars_since_low = self
            .lows
            .iter()
            .rev()
            .enumerate()
            .min_by_key(|(_, &l)| l)
            .map(|(i, _)| i)
            .unwrap_or(0);

        #[allow(clippy::cast_possible_truncation)]
        let p = Decimal::from(self.period as u32);
        let aroon_up = (p - Decimal::from(bars_since_high as u32)) / p * Decimal::from(100u32);
        let aroon_down = (p - Decimal::from(bars_since_low as u32)) / p * Decimal::from(100u32);

        Ok(SignalValue::Scalar(aroon_up - aroon_down))
    }

    fn reset(&mut self) {
        self.highs.clear();
        self.lows.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let mid = Price::new(((hp.value() + lp.value()) / Decimal::TWO).max(rust_decimal::Decimal::ONE)).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: mid, high: hp, low: lp, close: mid,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_aroon_osc_invalid_period() {
        assert!(AroonOscillator::new("ao", 0).is_err());
    }

    #[test]
    fn test_aroon_osc_unavailable_before_period() {
        let mut ao = AroonOscillator::new("ao", 3).unwrap();
        assert_eq!(ao.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert_eq!(ao.update_bar(&bar("108", "92")).unwrap(), SignalValue::Unavailable);
        assert!(!ao.is_ready());
    }

    #[test]
    fn test_aroon_osc_recent_high_bullish() {
        // period=2: need 3 bars. If current bar is highest high and lowest low is oldest:
        // bars_since_high=0, bars_since_low=2
        // aroon_up = (2-0)/2*100 = 100, aroon_down = (2-2)/2*100 = 0, osc = 100
        let mut ao = AroonOscillator::new("ao", 2).unwrap();
        ao.update_bar(&bar("100", "50")).unwrap(); // oldest
        ao.update_bar(&bar("105", "60")).unwrap();
        let v = ao.update_bar(&bar("120", "70")).unwrap(); // highest high, not lowest low
        if let SignalValue::Scalar(osc) = v {
            assert!(osc > dec!(0), "expected bullish oscillator, got {osc}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_aroon_osc_reset() {
        let mut ao = AroonOscillator::new("ao", 2).unwrap();
        ao.update_bar(&bar("100", "90")).unwrap();
        ao.update_bar(&bar("101", "89")).unwrap();
        ao.update_bar(&bar("102", "88")).unwrap();
        assert!(ao.is_ready());
        ao.reset();
        assert!(!ao.is_ready());
    }
}
