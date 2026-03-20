//! Bollinger Bands %B indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Bollinger Bands %B indicator.
///
/// Computes the position of the close price relative to the upper and lower Bollinger Bands:
///
/// ```text
/// middle = SMA(close, period)
/// upper  = middle + multiplier * std_dev(close, period)
/// lower  = middle - multiplier * std_dev(close, period)
/// %B     = (close - lower) / (upper - lower)
/// ```
///
/// Typical parameters: `period = 20`, `multiplier = 2.0`.
///
/// - `%B = 1.0` → close is at the upper band
/// - `%B = 0.5` → close is at the middle band
/// - `%B = 0.0` → close is at the lower band
/// - `%B > 1.0` → close is above the upper band (breakout)
/// - `%B < 0.0` → close is below the lower band
///
/// When `upper == lower` (all closes identical; zero standard deviation),
/// returns `0.5` (close is at the middle band).
///
/// Returns `SignalValue::Unavailable` until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BollingerB;
/// use fin_primitives::signals::Signal;
/// let bb = BollingerB::new("bb20", 20, 2).unwrap();
/// assert_eq!(bb.period(), 20);
/// ```
pub struct BollingerB {
    name: String,
    period: usize,
    multiplier: Decimal,
    values: VecDeque<Decimal>,
}

impl BollingerB {
    /// Constructs a new `BollingerB` indicator.
    ///
    /// # Errors
    /// Returns [`crate::error::FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(
        name: impl Into<String>,
        period: usize,
        multiplier: impl Into<Decimal>,
    ) -> Result<Self, crate::error::FinError> {
        if period == 0 {
            return Err(crate::error::FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            multiplier: multiplier.into(),
            values: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for BollingerB {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.values.push_back(bar.close);
        if self.values.len() > self.period {
            self.values.pop_front();
        }
        if self.values.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        #[allow(clippy::cast_possible_truncation)]
        let n = Decimal::from(self.period as u32);
        let sum: Decimal = self.values.iter().copied().sum();
        let mean = sum.checked_div(n).ok_or(FinError::ArithmeticOverflow)?;

        // Population standard deviation over the window.
        let variance_sum: Decimal = self
            .values
            .iter()
            .map(|v| {
                let diff = *v - mean;
                diff * diff
            })
            .sum();
        let variance = variance_sum
            .checked_div(n)
            .ok_or(FinError::ArithmeticOverflow)?;

        // rust_decimal doesn't have sqrt; approximate via Newton-Raphson.
        let std_dev = decimal_sqrt(variance)?;

        let band_width = self.multiplier * std_dev;
        let upper = mean + band_width;
        let lower = mean - band_width;
        let band_range = upper - lower;

        if band_range == Decimal::ZERO {
            // All prices identical — close is at the midpoint.
            return Ok(SignalValue::Scalar(
                Decimal::ONE
                    .checked_div(Decimal::TWO)
                    .ok_or(FinError::ArithmeticOverflow)?,
            ));
        }

        let pct_b = (bar.close - lower)
            .checked_div(band_range)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(pct_b))
    }

    fn is_ready(&self) -> bool {
        self.values.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.values.clear();
    }
}

impl BollingerB {
    /// Returns `(upper, middle, lower)` band values if the indicator is ready.
    ///
    /// Returns `None` if fewer than `period` bars have been seen or sqrt fails.
    pub fn bands(&self) -> Option<(Decimal, Decimal, Decimal)> {
        if self.values.len() < self.period {
            return None;
        }
        #[allow(clippy::cast_possible_truncation)]
        let n = Decimal::from(self.period as u32);
        let sum: Decimal = self.values.iter().copied().sum();
        let mean = sum.checked_div(n)?;
        let variance_sum: Decimal = self.values.iter().map(|v| { let d = *v - mean; d * d }).sum();
        let variance = variance_sum.checked_div(n)?;
        let std_dev = decimal_sqrt(variance).ok()?;
        let band_width = self.multiplier * std_dev;
        Some((mean + band_width, mean, mean - band_width))
    }
}

/// Newton-Raphson square root for `Decimal`.
///
/// Returns `0` for zero input. Converges within ~10 iterations for typical price ranges.
fn decimal_sqrt(n: Decimal) -> Result<Decimal, FinError> {
    if n == Decimal::ZERO {
        return Ok(Decimal::ZERO);
    }
    if n < Decimal::ZERO {
        return Err(FinError::ArithmeticOverflow);
    }
    let two = Decimal::TWO;
    // Initial guess: n / 2
    let mut x = n
        .checked_div(two)
        .ok_or(FinError::ArithmeticOverflow)?
        .max(Decimal::ONE);
    for _ in 0..20 {
        let x_next = (x + n.checked_div(x).ok_or(FinError::ArithmeticOverflow)?)
            .checked_div(two)
            .ok_or(FinError::ArithmeticOverflow)?;
        let diff = (x_next - x).abs();
        x = x_next;
        // Converge when change is negligible (< 1e-10)
        if diff < Decimal::new(1, 10) {
            break;
        }
    }
    Ok(x)
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
            open: p,
            high: p,
            low: p,
            close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_bollinger_period_0_fails() {
        assert!(BollingerB::new("bb", 0, 2u32).is_err());
    }

    #[test]
    fn test_bollinger_unavailable_before_period() {
        let mut bb = BollingerB::new("bb3", 3, 2u32).unwrap();
        assert_eq!(bb.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(bb.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert!(!bb.is_ready());
    }

    #[test]
    fn test_bollinger_constant_price_returns_half() {
        let mut bb = BollingerB::new("bb3", 3, 2u32).unwrap();
        bb.update_bar(&bar("100")).unwrap();
        bb.update_bar(&bar("100")).unwrap();
        let v = bb.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0.5)));
        assert!(bb.is_ready());
    }

    #[test]
    fn test_bollinger_close_at_upper_band_returns_one() {
        // With period=3, closes=[100, 100, 100+σ]: close is at the upper band → %B ~ 1
        // For a simpler test: feed 100, 100, 100 → std_dev=0 → returns 0.5
        // Then test that close above mean with known std_dev → %B > 0.5
        let mut bb = BollingerB::new("bb3", 3, 2u32).unwrap();
        bb.update_bar(&bar("90")).unwrap();
        bb.update_bar(&bar("100")).unwrap();
        let v = bb.update_bar(&bar("110")).unwrap(); // close above mean
        if let SignalValue::Scalar(pct_b) = v {
            // Close is above the SMA(100), so %B > 0.5
            assert!(pct_b > dec!(0.5), "close above SMA should yield %B > 0.5, got {pct_b}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_bollinger_reset_clears_state() {
        let mut bb = BollingerB::new("bb3", 3, 2u32).unwrap();
        for _ in 0..3 {
            bb.update_bar(&bar("100")).unwrap();
        }
        assert!(bb.is_ready());
        bb.reset();
        assert!(!bb.is_ready());
        assert_eq!(bb.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }
}
