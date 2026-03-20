//! Directional Efficiency indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Directional Efficiency — ratio of the net price displacement to the total path length
/// over `period` bars.
///
/// ```text
/// net_move   = |close_now - close_N_ago|
/// total_path = sum(|close_i - close_{i-1}|) for i in 1..=N
/// efficiency = net_move / total_path
/// ```
///
/// - **1.0**: price moved monotonically — perfectly efficient trend.
/// - **Near 0**: price oscillated extensively while ending near where it started — choppy.
/// - Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen
///   (need `period` return samples, which requires `period + 1` closes).
/// - Returns [`SignalValue::Unavailable`] if total_path is zero (flat price).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::DirectionalEfficiency;
/// use fin_primitives::signals::Signal;
///
/// let de = DirectionalEfficiency::new("de", 10).unwrap();
/// assert_eq!(de.period(), 10);
/// ```
pub struct DirectionalEfficiency {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl DirectionalEfficiency {
    /// Constructs a new `DirectionalEfficiency`.
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

impl Signal for DirectionalEfficiency {
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

        let first = *self.closes.front().unwrap();
        let last = *self.closes.back().unwrap();
        let net_move = (last - first).abs();

        let total_path: Decimal = self.closes.iter()
            .zip(self.closes.iter().skip(1))
            .map(|(a, b)| (*b - *a).abs())
            .sum();

        if total_path.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let efficiency = net_move
            .checked_div(total_path)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(efficiency))
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
    fn test_de_invalid_period() {
        assert!(DirectionalEfficiency::new("de", 0).is_err());
    }

    #[test]
    fn test_de_unavailable_during_warmup() {
        let mut de = DirectionalEfficiency::new("de", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(de.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!de.is_ready());
    }

    #[test]
    fn test_de_monotonic_trend_is_one() {
        // Monotonically rising: 100, 101, 102, 103
        // net_move=3, path=1+1+1=3 → efficiency=1
        let mut de = DirectionalEfficiency::new("de", 3).unwrap();
        de.update_bar(&bar("100")).unwrap();
        de.update_bar(&bar("101")).unwrap();
        de.update_bar(&bar("102")).unwrap();
        let result = de.update_bar(&bar("103")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_de_oscillating_is_low() {
        // Oscillating: 100, 110, 100, 110 → net_move=10, path=10+10+10=30 → eff=1/3
        let mut de = DirectionalEfficiency::new("de", 3).unwrap();
        de.update_bar(&bar("100")).unwrap();
        de.update_bar(&bar("110")).unwrap();
        de.update_bar(&bar("100")).unwrap();
        let result = de.update_bar(&bar("110")).unwrap();
        if let SignalValue::Scalar(v) = result {
            assert!(v < dec!(0.5), "oscillating → low efficiency: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_de_flat_is_unavailable() {
        let mut de = DirectionalEfficiency::new("de", 3).unwrap();
        for _ in 0..4 {
            let r = de.update_bar(&bar("100")).unwrap();
            if de.is_ready() {
                assert_eq!(r, SignalValue::Unavailable, "flat price → zero path → Unavailable");
            }
        }
    }

    #[test]
    fn test_de_reset() {
        let mut de = DirectionalEfficiency::new("de", 3).unwrap();
        for i in 0..4 { de.update_bar(&bar(&(100 + i).to_string())).unwrap(); }
        assert!(de.is_ready());
        de.reset();
        assert!(!de.is_ready());
    }
}
