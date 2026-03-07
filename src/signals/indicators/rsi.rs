//! Relative Strength Index (RSI) indicator.

use crate::error::FinError;
use crate::ohlcv::OhlcvBar;
use crate::signals::{Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Relative Strength Index over `period` bars.
///
/// Returns `SignalValue::Unavailable` until `period + 1` bars have been processed
/// (one extra bar is needed to compute the first price change).
///
/// Result is always in `[0, 100]`.
pub struct Rsi {
    name: String,
    period: usize,
    gains: VecDeque<Decimal>,
    losses: VecDeque<Decimal>,
    prev_close: Option<Decimal>,
    count: usize,
}

impl Rsi {
    /// Constructs a new `Rsi` with the given name and period.
    pub fn new(name: impl Into<String>, period: usize) -> Self {
        Self {
            name: name.into(),
            period,
            gains: VecDeque::with_capacity(period),
            losses: VecDeque::with_capacity(period),
            prev_close: None,
            count: 0,
        }
    }
}

impl Signal for Rsi {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &OhlcvBar) -> Result<SignalValue, FinError> {
        let close = bar.close.value();

        if let Some(prev) = self.prev_close {
            let change = close - prev;
            let (gain, loss) = if change >= Decimal::ZERO {
                (change, Decimal::ZERO)
            } else {
                (Decimal::ZERO, change.abs())
            };
            self.gains.push_back(gain);
            self.losses.push_back(loss);
            if self.gains.len() > self.period {
                self.gains.pop_front();
            }
            if self.losses.len() > self.period {
                self.losses.pop_front();
            }
            self.count += 1;
        }

        self.prev_close = Some(close);

        if self.count < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let avg_gain: Decimal = self.gains.iter().copied().sum::<Decimal>()
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        let avg_loss: Decimal = self.losses.iter().copied().sum::<Decimal>()
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        if avg_loss == Decimal::ZERO {
            // All gains, no losses → RSI = 100
            return Ok(SignalValue::Scalar(Decimal::ONE_HUNDRED));
        }

        let rs = avg_gain
            .checked_div(avg_loss)
            .ok_or(FinError::ArithmeticOverflow)?;
        let rsi = Decimal::ONE_HUNDRED
            - Decimal::ONE_HUNDRED
                .checked_div(Decimal::ONE + rs)
                .ok_or(FinError::ArithmeticOverflow)?;

        // Clamp to [0, 100] to guard against floating-point-style precision edge cases.
        let rsi = rsi.max(Decimal::ZERO).min(Decimal::ONE_HUNDRED);
        Ok(SignalValue::Scalar(rsi))
    }

    fn is_ready(&self) -> bool {
        self.count >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
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
            ts_open: NanoTimestamp(0),
            ts_close: NanoTimestamp(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_rsi_not_ready_before_period() {
        let mut rsi = Rsi::new("rsi3", 3);
        rsi.update(&bar("100")).unwrap();
        let v = rsi.update(&bar("105")).unwrap();
        assert!(matches!(v, SignalValue::Unavailable));
        assert!(!rsi.is_ready());
    }

    #[test]
    fn test_rsi_value_in_range_0_to_100() {
        let mut rsi = Rsi::new("rsi3", 3);
        let prices = ["100", "102", "101", "103", "105"];
        let mut last_val = Decimal::ZERO;
        for p in &prices {
            if let SignalValue::Scalar(v) = rsi.update(&bar(p)).unwrap() {
                last_val = v;
            }
        }
        assert!(last_val >= Decimal::ZERO);
        assert!(last_val <= Decimal::ONE_HUNDRED);
    }

    #[test]
    fn test_rsi_all_gains_returns_100() {
        let mut rsi = Rsi::new("rsi3", 3);
        // Monotonically increasing → RSI should be 100.
        rsi.update(&bar("100")).unwrap();
        rsi.update(&bar("110")).unwrap();
        rsi.update(&bar("120")).unwrap();
        let v = rsi.update(&bar("130")).unwrap();
        if let SignalValue::Scalar(val) = v {
            assert_eq!(val, dec!(100));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rsi_is_ready_after_period_plus_one() {
        let mut rsi = Rsi::new("rsi3", 3);
        rsi.update(&bar("100")).unwrap();
        rsi.update(&bar("101")).unwrap();
        rsi.update(&bar("102")).unwrap();
        assert!(!rsi.is_ready());
        rsi.update(&bar("103")).unwrap();
        assert!(rsi.is_ready());
    }

    #[test]
    fn test_rsi_mixed_values_bounded() {
        let mut rsi = Rsi::new("rsi14", 14);
        let prices = [
            "44.34", "44.09", "44.15", "43.61", "44.33", "44.83", "45.10", "45.15",
            "43.61", "44.33", "44.83", "45.10", "45.15", "43.61", "44.33",
        ];
        let mut val = SignalValue::Unavailable;
        for p in &prices {
            val = rsi.update(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(v) = val {
            assert!(v >= Decimal::ZERO, "RSI below 0: {v}");
            assert!(v <= Decimal::ONE_HUNDRED, "RSI above 100: {v}");
        }
    }
}
