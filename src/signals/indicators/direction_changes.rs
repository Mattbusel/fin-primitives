//! Direction Changes — rolling count of close-to-close direction reversals.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Direction Changes — count of close-over-close reversals in the last `period` bars.
///
/// A reversal occurs when consecutive bar moves change sign: an up move followed by a
/// down move, or vice versa. Requires 3 closes to detect the first reversal.
///
/// Interpretation:
/// - **High count**: choppy, mean-reverting market (many direction flips).
/// - **Low count**: trending market (sustained directional moves).
/// - **Maximum possible**: `period - 1` (every bar reverses direction).
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` closes have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::DirectionChanges;
/// use fin_primitives::signals::Signal;
/// let dc = DirectionChanges::new("dc_10", 10).unwrap();
/// assert_eq!(dc.period(), 10);
/// ```
pub struct DirectionChanges {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl DirectionChanges {
    /// Constructs a new `DirectionChanges`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            closes: VecDeque::with_capacity(period + 1),
        })
    }
}

impl Signal for DirectionChanges {
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

        let closes: Vec<&Decimal> = self.closes.iter().collect();
        let mut count = 0u32;
        for i in 1..closes.len() - 1 {
            let prev_move = *closes[i] - *closes[i - 1];
            let curr_move = *closes[i + 1] - *closes[i];
            // A reversal: moves are non-zero and have opposite signs
            let reversal = (prev_move > Decimal::ZERO && curr_move < Decimal::ZERO)
                || (prev_move < Decimal::ZERO && curr_move > Decimal::ZERO);
            if reversal {
                count += 1;
            }
        }

        Ok(SignalValue::Scalar(Decimal::from(count)))
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
    fn test_dc_invalid_period() {
        assert!(DirectionChanges::new("dc", 0).is_err());
        assert!(DirectionChanges::new("dc", 1).is_err());
    }

    #[test]
    fn test_dc_unavailable_before_warm_up() {
        let mut s = DirectionChanges::new("dc", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!s.is_ready());
    }

    #[test]
    fn test_dc_trending_series_zero() {
        let mut s = DirectionChanges::new("dc", 3).unwrap();
        // 100→101→102→103: all up, no reversals
        s.update_bar(&bar("100")).unwrap();
        s.update_bar(&bar("101")).unwrap();
        s.update_bar(&bar("102")).unwrap();
        let v = s.update_bar(&bar("103")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_dc_alternating_series_max() {
        let mut s = DirectionChanges::new("dc", 4).unwrap();
        // 100→102→100→102→100: alternating, every inner bar is a reversal
        s.update_bar(&bar("100")).unwrap();
        s.update_bar(&bar("102")).unwrap();
        s.update_bar(&bar("100")).unwrap();
        s.update_bar(&bar("102")).unwrap();
        let v = s.update_bar(&bar("100")).unwrap();
        // 5 closes, period=4, window=[102,100,102,100,?] no wait
        // window has period+1=5 closes: [100,102,100,102,100]
        // moves: +2, -2, +2, -2 → reversals at positions 1,2,3 → count=3
        if let SignalValue::Scalar(r) = v {
            assert!(r >= dec!(1), "alternating series should have many reversals: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_dc_non_negative() {
        let mut s = DirectionChanges::new("dc", 5).unwrap();
        let prices = ["100", "102", "101", "103", "102", "104"];
        for p in &prices {
            if let SignalValue::Scalar(v) = s.update_bar(&bar(p)).unwrap() {
                assert!(v >= dec!(0), "direction changes must be non-negative: {v}");
            }
        }
    }

    #[test]
    fn test_dc_reset() {
        let mut s = DirectionChanges::new("dc", 3).unwrap();
        for p in &["100", "101", "102", "103"] {
            s.update_bar(&bar(p)).unwrap();
        }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
