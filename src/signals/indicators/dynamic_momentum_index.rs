//! Dynamic Momentum Index (DYMI) — Tushar Chande's volatility-adaptive RSI.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Dynamic Momentum Index — an RSI whose period adapts to recent volatility.
///
/// The period is calculated each bar as:
/// ```text
/// dynamic_period = clamp(round(base_period * (short_std / long_std)), min_period, max_period)
/// ```
/// where `short_std` is the 5-bar standard deviation of closes and `long_std` is the
/// 10-bar standard deviation. A more volatile market shrinks the period (faster RSI),
/// a quieter market stretches it (slower RSI).
///
/// Defaults: `base_period = 14`, `min_period = 3`, `max_period = 30`.
///
/// Returns [`SignalValue::Unavailable`] until `max_period + 1` bars have been seen (enough
/// history for both the std-dev windows and the worst-case RSI warm-up).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::DynamicMomentumIndex;
/// use fin_primitives::signals::Signal;
/// let dymi = DynamicMomentumIndex::new("dymi", 14, 3, 30).unwrap();
/// assert_eq!(dymi.period(), 30);
/// ```
pub struct DynamicMomentumIndex {
    name: String,
    base_period: usize,
    min_period: usize,
    max_period: usize,
    history: VecDeque<Decimal>,
}

impl DynamicMomentumIndex {
    /// Constructs a new `DynamicMomentumIndex`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `min_period < 2`, `max_period < min_period`,
    /// or `base_period == 0`.
    pub fn new(
        name: impl Into<String>,
        base_period: usize,
        min_period: usize,
        max_period: usize,
    ) -> Result<Self, FinError> {
        if base_period == 0 {
            return Err(FinError::InvalidPeriod(base_period));
        }
        if min_period < 2 {
            return Err(FinError::InvalidPeriod(min_period));
        }
        if max_period < min_period {
            return Err(FinError::InvalidInput(
                "max_period must be >= min_period".to_owned(),
            ));
        }
        Ok(Self {
            name: name.into(),
            base_period,
            min_period,
            max_period,
            history: VecDeque::with_capacity(max_period + 2),
        })
    }

    fn std_dev(values: &[Decimal]) -> f64 {
        use rust_decimal::prelude::ToPrimitive;
        let n = values.len() as f64;
        if n < 2.0 {
            return 0.0;
        }
        let mean: f64 = values.iter().filter_map(|d| d.to_f64()).sum::<f64>() / n;
        let var: f64 = values
            .iter()
            .filter_map(|d| d.to_f64())
            .map(|v| (v - mean).powi(2))
            .sum::<f64>()
            / (n - 1.0);
        var.sqrt()
    }

    fn compute_rsi(closes: &[Decimal], period: usize) -> f64 {
        use rust_decimal::prelude::ToPrimitive;
        if closes.len() < period + 1 {
            return f64::NAN;
        }
        let slice = &closes[closes.len() - (period + 1)..];

        let mut avg_gain = 0.0_f64;
        let mut avg_loss = 0.0_f64;

        // Seed phase.
        for i in 1..=period {
            let chg = slice[i].to_f64().unwrap_or(0.0)
                - slice[i - 1].to_f64().unwrap_or(0.0);
            if chg > 0.0 {
                avg_gain += chg;
            } else {
                avg_loss += -chg;
            }
        }
        let p = period as f64;
        avg_gain /= p;
        avg_loss /= p;

        if avg_loss == 0.0 {
            return 100.0;
        }
        if avg_gain == 0.0 {
            return 0.0;
        }
        100.0 - 100.0 / (1.0 + avg_gain / avg_loss)
    }
}

impl Signal for DynamicMomentumIndex {
    fn name(&self) -> &str {
        &self.name
    }

    /// Reports `max_period` as the warm-up period (worst-case RSI period).
    fn period(&self) -> usize {
        self.max_period
    }

    fn is_ready(&self) -> bool {
        self.history.len() >= self.max_period + 1
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.history.push_back(bar.close);
        // Keep enough history for max_period + 1 closes.
        while self.history.len() > self.max_period + 1 {
            self.history.pop_front();
        }

        if self.history.len() < self.max_period + 1 {
            return Ok(SignalValue::Unavailable);
        }

        let all: Vec<Decimal> = self.history.iter().copied().collect();

        // Short (5-bar) and long (10-bar) std-devs of closes.
        let short_std = Self::std_dev(&all[all.len().saturating_sub(5)..]);
        let long_std = Self::std_dev(&all[all.len().saturating_sub(10)..]);

        let dynamic_period = if long_std == 0.0 || short_std == 0.0 {
            self.base_period
        } else {
            let ratio = short_std / long_std;
            let dp = (self.base_period as f64 * ratio).round() as usize;
            dp.clamp(self.min_period, self.max_period)
        };

        let rsi = Self::compute_rsi(&all, dynamic_period);
        if rsi.is_nan() || rsi.is_infinite() {
            return Ok(SignalValue::Unavailable);
        }
        let result = Decimal::try_from(rsi).unwrap_or(Decimal::ZERO);
        Ok(SignalValue::Scalar(result.clamp(Decimal::ZERO, Decimal::ONE_HUNDRED)))
    }

    fn reset(&mut self) {
        self.history.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};

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
    fn test_dymi_invalid_base_period() {
        assert!(DynamicMomentumIndex::new("d", 0, 3, 30).is_err());
    }

    #[test]
    fn test_dymi_invalid_min_period() {
        assert!(DynamicMomentumIndex::new("d", 14, 1, 30).is_err());
    }

    #[test]
    fn test_dymi_invalid_max_less_than_min() {
        assert!(DynamicMomentumIndex::new("d", 14, 10, 5).is_err());
    }

    #[test]
    fn test_dymi_unavailable_before_warm_up() {
        let mut d = DynamicMomentumIndex::new("d", 14, 3, 10).unwrap();
        for i in 0..10u32 {
            let v = d.update_bar(&bar(&(100 + i).to_string())).unwrap();
            assert_eq!(v, SignalValue::Unavailable);
        }
        assert!(!d.is_ready());
    }

    #[test]
    fn test_dymi_produces_value_after_warm_up() {
        let mut d = DynamicMomentumIndex::new("d", 14, 3, 10).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0..12u32 {
            last = d.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        assert!(last.is_scalar());
    }

    #[test]
    fn test_dymi_output_in_range() {
        use rust_decimal_macros::dec;
        let mut d = DynamicMomentumIndex::new("d", 14, 3, 10).unwrap();
        let prices = [
            "100", "102", "101", "103", "105", "104", "106", "108", "107", "109", "108", "110",
        ];
        for p in &prices {
            if let SignalValue::Scalar(v) = d.update_bar(&bar(p)).unwrap() {
                assert!(v >= dec!(0), "DYMI < 0");
                assert!(v <= dec!(100), "DYMI > 100");
            }
        }
    }

    #[test]
    fn test_dymi_reset() {
        let mut d = DynamicMomentumIndex::new("d", 14, 3, 10).unwrap();
        for i in 0..12u32 {
            d.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        assert!(d.is_ready());
        d.reset();
        assert!(!d.is_ready());
    }

    #[test]
    fn test_dymi_period_is_max_period() {
        let d = DynamicMomentumIndex::new("d", 14, 3, 30).unwrap();
        assert_eq!(d.period(), 30);
    }
}
