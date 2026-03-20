//! Stochastic %K oscillator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Stochastic %K oscillator.
///
/// Measures the position of the current close relative to the price range over `period` bars:
///
/// ```text
/// %K = (close - lowest_low(period)) / (highest_high(period) - lowest_low(period)) * 100
/// ```
///
/// Output is in the range `[0, 100]`:
/// - `100` → close is at the highest high of the period
/// - `0`   → close is at the lowest low of the period
/// - `50`  → close is at the midpoint of the range
///
/// When `highest_high == lowest_low` (flat price, zero range), returns `50`.
///
/// Returns `SignalValue::Unavailable` until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::StochasticK;
/// use fin_primitives::signals::Signal;
/// let stoch = StochasticK::new("stoch14", 14).unwrap();
/// assert_eq!(stoch.period(), 14);
/// ```
pub struct StochasticK {
    name: String,
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
}

impl StochasticK {
    /// Constructs a new `StochasticK` indicator.
    ///
    /// # Errors
    /// Returns [`crate::error::FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, crate::error::FinError> {
        if period == 0 {
            return Err(crate::error::FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            highs: VecDeque::with_capacity(period),
            lows: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for StochasticK {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);
        if self.highs.len() > self.period {
            self.highs.pop_front();
            self.lows.pop_front();
        }
        if self.highs.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let highest_high = self
            .highs
            .iter()
            .copied()
            .fold(Decimal::MIN, Decimal::max);
        let lowest_low = self
            .lows
            .iter()
            .copied()
            .fold(Decimal::MAX, Decimal::min);

        let range = highest_high - lowest_low;
        if range == Decimal::ZERO {
            // Flat price — return 50 (midpoint).
            return Ok(SignalValue::Scalar(
                Decimal::from(50u32),
            ));
        }

        let pct_k = (bar.close - lowest_low)
            .checked_div(range)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_mul(Decimal::ONE_HUNDRED)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(pct_k))
    }

    fn is_ready(&self) -> bool {
        self.highs.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
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

    fn close_bar(c: &str) -> OhlcvBar {
        bar(c, c, c, c)
    }

    #[test]
    fn test_stoch_period_0_fails() {
        assert!(StochasticK::new("k", 0).is_err());
    }

    #[test]
    fn test_stoch_unavailable_before_period() {
        let mut k = StochasticK::new("k3", 3).unwrap();
        assert_eq!(k.update_bar(&bar("100", "110", "90", "100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(k.update_bar(&bar("100", "110", "90", "100")).unwrap(), SignalValue::Unavailable);
        assert!(!k.is_ready());
    }

    #[test]
    fn test_stoch_close_at_highest_high_returns_100() {
        let mut k = StochasticK::new("k3", 3).unwrap();
        k.update_bar(&bar("90", "95", "85", "90")).unwrap();
        k.update_bar(&bar("90", "95", "85", "90")).unwrap();
        // Last bar: close = high = 110, low = 85 → %K = (110-85)/(110-85)*100 = 100
        let v = k.update_bar(&bar("90", "110", "85", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_stoch_close_at_lowest_low_returns_0() {
        let mut k = StochasticK::new("k3", 3).unwrap();
        k.update_bar(&bar("90", "95", "85", "90")).unwrap();
        k.update_bar(&bar("90", "95", "85", "90")).unwrap();
        // close = lowest_low = 85
        let v = k.update_bar(&bar("90", "95", "85", "85")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_stoch_flat_price_returns_50() {
        let mut k = StochasticK::new("k3", 3).unwrap();
        k.update_bar(&close_bar("100")).unwrap();
        k.update_bar(&close_bar("100")).unwrap();
        let v = k.update_bar(&close_bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_stoch_output_in_0_to_100() {
        let mut k = StochasticK::new("k3", 3).unwrap();
        k.update_bar(&bar("90", "110", "80", "95")).unwrap();
        k.update_bar(&bar("95", "115", "85", "105")).unwrap();
        let v = k.update_bar(&bar("100", "120", "90", "112")).unwrap();
        if let SignalValue::Scalar(pct) = v {
            assert!(pct >= dec!(0), "%K must be >= 0, got {pct}");
            assert!(pct <= dec!(100), "%K must be <= 100, got {pct}");
        } else {
            panic!("expected Scalar after period bars");
        }
    }

    #[test]
    fn test_stoch_reset_clears_state() {
        let mut k = StochasticK::new("k3", 3).unwrap();
        for _ in 0..3 {
            k.update_bar(&close_bar("100")).unwrap();
        }
        assert!(k.is_ready());
        k.reset();
        assert!(!k.is_ready());
    }
}
