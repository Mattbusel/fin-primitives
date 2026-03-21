//! Price Trend Quality indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Trend Quality.
///
/// Measures the "quality" of a trend by computing the ratio of the net price move
/// to the total path traveled. A perfectly smooth trend would have a ratio of 1.0;
/// choppy price action results in a lower ratio.
///
/// Formula:
/// - `net_move = |close_t - close_{t-period}|`
/// - `total_path = Σ|close_i - close_{i-1}|` over period bars
/// - `quality = net_move / total_path`
///
/// - 1.0: perfectly straight trend (all moves in one direction).
/// - ~0: very choppy (many reversals, net move near zero).
/// - Returns 0 when total_path is zero.
///
/// Returns `SignalValue::Unavailable` until `period + 1` closes accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceTrendQuality;
/// use fin_primitives::signals::Signal;
/// let ptq = PriceTrendQuality::new("ptq_14", 14).unwrap();
/// assert_eq!(ptq.period(), 14);
/// ```
pub struct PriceTrendQuality {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl PriceTrendQuality {
    /// Constructs a new `PriceTrendQuality`.
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

impl Signal for PriceTrendQuality {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }

        let first = *self.closes.front().unwrap();
        let last = *self.closes.back().unwrap();
        let net_move = (last - first).abs();

        let mut total_path = Decimal::ZERO;
        for i in 0..self.closes.len() - 1 {
            total_path += (self.closes[i + 1] - self.closes[i]).abs();
        }

        if total_path.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let quality = net_move.checked_div(total_path).ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(quality))
    }

    fn is_ready(&self) -> bool {
        self.closes.len() >= self.period + 1
    }

    fn period(&self) -> usize {
        self.period
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
    fn test_period_zero_fails() {
        assert!(matches!(PriceTrendQuality::new("ptq", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut ptq = PriceTrendQuality::new("ptq", 3).unwrap();
        assert_eq!(ptq.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_straight_trend_gives_one() {
        // Monotonically increasing → total_path = net_move → quality = 1
        let mut ptq = PriceTrendQuality::new("ptq", 3).unwrap();
        ptq.update_bar(&bar("100")).unwrap();
        ptq.update_bar(&bar("101")).unwrap();
        ptq.update_bar(&bar("102")).unwrap();
        let v = ptq.update_bar(&bar("103")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_alternating_choppy_low_quality() {
        // Alternating → total_path >> net_move
        let mut ptq = PriceTrendQuality::new("ptq", 4).unwrap();
        ptq.update_bar(&bar("100")).unwrap();
        ptq.update_bar(&bar("102")).unwrap();
        ptq.update_bar(&bar("100")).unwrap();
        ptq.update_bar(&bar("102")).unwrap();
        let v = ptq.update_bar(&bar("100")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s < dec!(1));
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_reset() {
        let mut ptq = PriceTrendQuality::new("ptq", 2).unwrap();
        for _ in 0..3 {
            ptq.update_bar(&bar("100")).unwrap();
        }
        assert!(ptq.is_ready());
        ptq.reset();
        assert!(!ptq.is_ready());
    }
}
