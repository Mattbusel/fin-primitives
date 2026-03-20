//! Commodity Channel Index (CCI) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Commodity Channel Index over `period` bars.
///
/// CCI measures how far the typical price has deviated from its average:
///
/// ```text
/// typical_price = (high + low + close) / 3
/// CCI = (typical_price - SMA(typical_price, period)) / (0.015 * mean_deviation)
/// mean_deviation = Σ|typical_price_i - SMA| / period
/// ```
///
/// The constant `0.015` scales CCI so that roughly 70–80% of readings fall in
/// the range `[-100, +100]` under normal market conditions.
///
/// Returns `SignalValue::Unavailable` until `period` bars have been seen.
///
/// When `mean_deviation == 0` (all typical prices identical), returns `0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Cci;
/// use fin_primitives::signals::Signal;
/// let cci = Cci::new("cci_20", 20).unwrap();
/// assert_eq!(cci.period(), 20);
/// ```
pub struct Cci {
    name: String,
    period: usize,
    typical_prices: VecDeque<Decimal>,
}

impl Cci {
    /// Constructs a new `Cci` indicator.
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
            typical_prices: VecDeque::with_capacity(period),
        })
    }
}

/// Scale factor used in the CCI formula.
const CCI_SCALE: &str = "0.015";

impl Signal for Cci {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tp = (bar.high + bar.low + bar.close) / Decimal::from(3u32);
        self.typical_prices.push_back(tp);
        if self.typical_prices.len() > self.period {
            self.typical_prices.pop_front();
        }
        if self.typical_prices.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        #[allow(clippy::cast_possible_truncation)]
        let n = Decimal::from(self.period as u32);
        let sum: Decimal = self.typical_prices.iter().copied().sum();
        let sma = sum.checked_div(n).ok_or(FinError::ArithmeticOverflow)?;

        let mean_dev: Decimal = self
            .typical_prices
            .iter()
            .map(|&p| (p - sma).abs())
            .sum::<Decimal>()
            .checked_div(n)
            .ok_or(FinError::ArithmeticOverflow)?;

        if mean_dev == Decimal::ZERO {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let scale: Decimal = CCI_SCALE.parse().unwrap_or(Decimal::new(15, 3));
        let cci = (tp - sma)
            .checked_div(scale * mean_dev)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(cci))
    }

    fn is_ready(&self) -> bool {
        self.typical_prices.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.typical_prices.clear();
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

    fn flat_bar(p: &str) -> OhlcvBar {
        bar(p, p, p, p)
    }

    #[test]
    fn test_cci_period_0_fails() {
        assert!(Cci::new("cci0", 0).is_err());
    }

    #[test]
    fn test_cci_unavailable_before_period() {
        let mut cci = Cci::new("cci3", 3).unwrap();
        assert_eq!(cci.update_bar(&flat_bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(cci.update_bar(&flat_bar("100")).unwrap(), SignalValue::Unavailable);
        assert!(!cci.is_ready());
    }

    #[test]
    fn test_cci_constant_prices_returns_zero() {
        let mut cci = Cci::new("cci3", 3).unwrap();
        cci.update_bar(&flat_bar("100")).unwrap();
        cci.update_bar(&flat_bar("100")).unwrap();
        let v = cci.update_bar(&flat_bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cci_positive_when_close_above_average() {
        let mut cci = Cci::new("cci3", 3).unwrap();
        cci.update_bar(&flat_bar("90")).unwrap();
        cci.update_bar(&flat_bar("100")).unwrap();
        // Last bar far above the average of [90, 100, 150]
        let v = cci.update_bar(&flat_bar("150")).unwrap();
        if let SignalValue::Scalar(val) = v {
            assert!(val > dec!(0), "CCI should be positive when close is above SMA, got {val}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_cci_negative_when_close_below_average() {
        let mut cci = Cci::new("cci3", 3).unwrap();
        cci.update_bar(&flat_bar("150")).unwrap();
        cci.update_bar(&flat_bar("100")).unwrap();
        // Last bar far below the average
        let v = cci.update_bar(&flat_bar("50")).unwrap();
        if let SignalValue::Scalar(val) = v {
            assert!(val < dec!(0), "CCI should be negative when close is below SMA, got {val}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_cci_is_ready_after_period() {
        let mut cci = Cci::new("cci3", 3).unwrap();
        cci.update_bar(&flat_bar("100")).unwrap();
        cci.update_bar(&flat_bar("100")).unwrap();
        assert!(!cci.is_ready());
        cci.update_bar(&flat_bar("100")).unwrap();
        assert!(cci.is_ready());
    }

    #[test]
    fn test_cci_reset() {
        let mut cci = Cci::new("cci3", 3).unwrap();
        for _ in 0..3 {
            cci.update_bar(&flat_bar("100")).unwrap();
        }
        assert!(cci.is_ready());
        cci.reset();
        assert!(!cci.is_ready());
        assert_eq!(cci.update_bar(&flat_bar("100")).unwrap(), SignalValue::Unavailable);
    }
}
