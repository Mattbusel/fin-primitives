//! Price Path Efficiency — net price move divided by total price path over N bars.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Path Efficiency — `|close[t] - close[t-N]| / sum(|close[i] - close[i-1]|)`.
///
/// Measures how efficiently price has moved toward its destination over the last
/// `period` bars:
/// - **1.0**: perfectly efficient (price moved in a straight line).
/// - **Near 0**: very noisy/choppy (much back-and-forth relative to net move).
///
/// This is the same concept as the Kaufman Efficiency Ratio but applied directly
/// to close prices rather than to the ATR/price-change calculation.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen,
/// or when the total price path is zero (all closes equal).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PricePathEfficiency;
/// use fin_primitives::signals::Signal;
/// let ppe = PricePathEfficiency::new("ppe_10", 10).unwrap();
/// assert_eq!(ppe.period(), 10);
/// ```
pub struct PricePathEfficiency {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl PricePathEfficiency {
    /// Constructs a new `PricePathEfficiency`.
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

impl Signal for PricePathEfficiency {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.closes.len() > self.period
    }

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

        let total_path: Decimal = self.closes.iter().zip(self.closes.iter().skip(1))
            .map(|(a, b)| (*b - *a).abs())
            .sum();

        if total_path.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let efficiency = net_move.checked_div(total_path).ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(efficiency.min(Decimal::ONE)))
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

    fn bar(close: &str) -> OhlcvBar {
        let p = Price::new(close.parse().unwrap()).unwrap();
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
    fn test_ppe_invalid_period() {
        assert!(PricePathEfficiency::new("ppe", 0).is_err());
        assert!(PricePathEfficiency::new("ppe", 1).is_err());
    }

    #[test]
    fn test_ppe_unavailable_before_period_plus_1() {
        let mut ppe = PricePathEfficiency::new("ppe", 3).unwrap();
        for p in &["100", "101", "102"] {
            assert_eq!(ppe.update_bar(&bar(p)).unwrap(), SignalValue::Unavailable);
        }
        assert!(!ppe.is_ready());
    }

    #[test]
    fn test_ppe_perfectly_trending_gives_one() {
        // Monotonic increase → efficiency = 1.0
        let mut ppe = PricePathEfficiency::new("ppe", 3).unwrap();
        ppe.update_bar(&bar("100")).unwrap();
        ppe.update_bar(&bar("101")).unwrap();
        ppe.update_bar(&bar("102")).unwrap();
        let v = ppe.update_bar(&bar("103")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_ppe_output_in_unit_interval() {
        let mut ppe = PricePathEfficiency::new("ppe", 4).unwrap();
        let prices = ["100", "102", "101", "103", "102", "104"];
        for p in &prices {
            if let SignalValue::Scalar(v) = ppe.update_bar(&bar(p)).unwrap() {
                assert!(v >= dec!(0), "efficiency must be >= 0: {v}");
                assert!(v <= dec!(1), "efficiency must be <= 1: {v}");
            }
        }
    }

    #[test]
    fn test_ppe_flat_prices_unavailable() {
        // All same price → total path = 0 → Unavailable
        let mut ppe = PricePathEfficiency::new("ppe", 3).unwrap();
        for _ in 0..5 {
            ppe.update_bar(&bar("100")).unwrap();
        }
        let v = ppe.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_ppe_reset() {
        let mut ppe = PricePathEfficiency::new("ppe", 3).unwrap();
        for p in &["100", "101", "102", "103"] {
            ppe.update_bar(&bar(p)).unwrap();
        }
        assert!(ppe.is_ready());
        ppe.reset();
        assert!(!ppe.is_ready());
    }

    #[test]
    fn test_ppe_period_and_name() {
        let ppe = PricePathEfficiency::new("my_ppe", 10).unwrap();
        assert_eq!(ppe.period(), 10);
        assert_eq!(ppe.name(), "my_ppe");
    }
}
