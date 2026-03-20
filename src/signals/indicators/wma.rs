//! Weighted Moving Average (WMA) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Weighted Moving Average over `period` bars.
///
/// Assigns linearly increasing weights to prices in the window:
/// the oldest price receives weight 1, the most recent receives weight `period`.
///
/// ```text
/// WMA = (p1*1 + p2*2 + ... + pN*N) / (1 + 2 + ... + N)
///     = Σ(p_i * i) / (N*(N+1)/2)
/// ```
///
/// Returns `SignalValue::Unavailable` until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Wma;
/// use fin_primitives::signals::Signal;
/// let wma = Wma::new("wma_10", 10).unwrap();
/// assert_eq!(wma.period(), 10);
/// ```
pub struct Wma {
    name: String,
    period: usize,
    values: VecDeque<Decimal>,
    /// Denominator: `period * (period + 1) / 2`
    denominator: Decimal,
}

impl Wma {
    /// Constructs a new `Wma` with the given name and period.
    ///
    /// # Errors
    /// Returns [`crate::error::FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, crate::error::FinError> {
        if period == 0 {
            return Err(crate::error::FinError::InvalidPeriod(period));
        }
        #[allow(clippy::cast_possible_truncation)]
        let n = period as u64;
        let denominator = Decimal::from(n * (n + 1) / 2);
        Ok(Self {
            name: name.into(),
            period,
            values: VecDeque::with_capacity(period),
            denominator,
        })
    }
}

impl Signal for Wma {
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

        let weighted_sum: Decimal = self
            .values
            .iter()
            .enumerate()
            .map(|(i, &price)| {
                #[allow(clippy::cast_possible_truncation)]
                let weight = Decimal::from((i + 1) as u32);
                price * weight
            })
            .sum();

        let wma = weighted_sum
            .checked_div(self.denominator)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(wma))
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
    fn test_wma_period_0_fails() {
        assert!(Wma::new("wma0", 0).is_err());
    }

    #[test]
    fn test_wma_unavailable_before_period() {
        let mut wma = Wma::new("wma3", 3).unwrap();
        assert_eq!(wma.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(wma.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert!(!wma.is_ready());
    }

    #[test]
    fn test_wma_period_1_equals_close() {
        let mut wma = Wma::new("wma1", 1).unwrap();
        let v = wma.update_bar(&bar("42")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(42)));
        assert!(wma.is_ready());
    }

    #[test]
    fn test_wma_period_3_correct_value() {
        // Period 3: weights = [1, 2, 3], denominator = 6
        // Prices = [10, 20, 30]: WMA = (10*1 + 20*2 + 30*3) / 6 = (10+40+90)/6 = 140/6 = 23.333...
        let mut wma = Wma::new("wma3", 3).unwrap();
        wma.update_bar(&bar("10")).unwrap();
        wma.update_bar(&bar("20")).unwrap();
        let v = wma.update_bar(&bar("30")).unwrap();
        let expected = dec!(140) / dec!(6);
        assert_eq!(v, SignalValue::Scalar(expected));
    }

    #[test]
    fn test_wma_weights_recent_price_more_than_sma() {
        // WMA should weight the most recent price more heavily than SMA
        // Feed [10, 10, 100]: WMA gives more weight to 100 → WMA > SMA
        let mut wma = Wma::new("wma3", 3).unwrap();
        wma.update_bar(&bar("10")).unwrap();
        wma.update_bar(&bar("10")).unwrap();
        let wma_val = match wma.update_bar(&bar("100")).unwrap() {
            SignalValue::Scalar(v) => v,
            _ => panic!("expected scalar"),
        };
        // SMA would be (10+10+100)/3 ≈ 40; WMA = (10+20+300)/6 = 330/6 = 55
        assert!(wma_val > dec!(40), "WMA {wma_val} should exceed SMA ≈ 40 on recent spike");
    }

    #[test]
    fn test_wma_reset() {
        let mut wma = Wma::new("wma3", 3).unwrap();
        for _ in 0..3 {
            wma.update_bar(&bar("100")).unwrap();
        }
        assert!(wma.is_ready());
        wma.reset();
        assert!(!wma.is_ready());
        assert_eq!(wma.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }
}
