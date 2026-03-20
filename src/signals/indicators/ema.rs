//! Exponential Moving Average (EMA) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Exponential Moving Average over `period` bars.
///
/// Uses an SMA seed for the first `period` bars, then applies:
/// `EMA = close * k + prev_EMA * (1 - k)` where `k = 2 / (period + 1)`.
///
/// Returns `SignalValue::Unavailable` until `period` bars have been seen.
pub struct Ema {
    name: String,
    period: usize,
    current: Option<Decimal>,
    count: usize,
    /// Multiplier: `2 / (period + 1)`
    multiplier: Decimal,
    /// Accumulator for SMA seed phase.
    seed_sum: Decimal,
}

impl Ema {
    /// Constructs a new `Ema` with the given name and period.
    ///
    /// Uses the standard smoothing factor `α = 2 / (period + 1)`.
    ///
    /// # Errors
    /// Returns [`crate::error::FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, crate::error::FinError> {
        if period == 0 {
            return Err(crate::error::FinError::InvalidPeriod(period));
        }
        #[allow(clippy::cast_possible_truncation)]
        let denom = Decimal::from((period + 1) as u32);
        let multiplier = Decimal::TWO.checked_div(denom).unwrap_or(Decimal::ONE);
        Ok(Self {
            name: name.into(),
            period,
            current: None,
            count: 0,
            multiplier,
            seed_sum: Decimal::ZERO,
        })
    }

    /// Constructs an `Ema` using **Wilder smoothing**: `α = 1 / period`.
    ///
    /// Wilder's smoothing is used in RSI, ATR, and ADX calculations.
    /// It converges more slowly than the standard `2/(n+1)` form,
    /// giving more weight to historical values.
    ///
    /// # Errors
    /// Returns [`crate::error::FinError::InvalidPeriod`] if `period == 0`.
    pub fn wilder(name: impl Into<String>, period: usize) -> Result<Self, crate::error::FinError> {
        if period == 0 {
            return Err(crate::error::FinError::InvalidPeriod(period));
        }
        #[allow(clippy::cast_possible_truncation)]
        let denom = Decimal::from(period as u32);
        let multiplier = Decimal::ONE.checked_div(denom).unwrap_or(Decimal::ONE);
        Ok(Self {
            name: name.into(),
            period,
            current: None,
            count: 0,
            multiplier,
            seed_sum: Decimal::ZERO,
        })
    }
}

impl Signal for Ema {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let close = bar.close;
        self.count += 1;

        if self.count <= self.period {
            // SMA seed phase.
            self.seed_sum += close;
            if self.count == self.period {
                #[allow(clippy::cast_possible_truncation)]
                let seed = self
                    .seed_sum
                    .checked_div(Decimal::from(self.period as u32))
                    .ok_or(FinError::ArithmeticOverflow)?;
                self.current = Some(seed);
                return Ok(SignalValue::Scalar(seed));
            }
            return Ok(SignalValue::Unavailable);
        }

        // EMA phase.
        let prev = self.current.unwrap_or(Decimal::ZERO);
        let one_minus_k = Decimal::ONE
            .checked_sub(self.multiplier)
            .ok_or(FinError::ArithmeticOverflow)?;
        let ema = close
            .checked_mul(self.multiplier)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_add(
                prev.checked_mul(one_minus_k)
                    .ok_or(FinError::ArithmeticOverflow)?,
            )
            .ok_or(FinError::ArithmeticOverflow)?;
        self.current = Some(ema);
        Ok(SignalValue::Scalar(ema))
    }

    fn is_ready(&self) -> bool {
        self.count >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.current = None;
        self.count = 0;
        self.seed_sum = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::indicators::Sma;
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
    fn test_ema_not_ready_before_period() {
        let mut ema = Ema::new("ema3", 3).unwrap();
        let v = ema.update_bar(&bar("10")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
        assert!(!ema.is_ready());
    }

    #[test]
    fn test_ema_first_value_equals_sma_seed() {
        // period=3: SMA of first 3 bars = (10+20+30)/3 = 20
        let mut ema = Ema::new("ema3", 3).unwrap();
        ema.update_bar(&bar("10")).unwrap();
        ema.update_bar(&bar("20")).unwrap();
        let v = ema.update_bar(&bar("30")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(20)));
    }

    #[test]
    fn test_ema_subsequent_values_weighted() {
        // period=3, k = 2/4 = 0.5
        // seed = (10+20+30)/3 = 20
        // 4th bar close=40: EMA = 40*0.5 + 20*0.5 = 30
        let mut ema = Ema::new("ema3", 3).unwrap();
        ema.update_bar(&bar("10")).unwrap();
        ema.update_bar(&bar("20")).unwrap();
        ema.update_bar(&bar("30")).unwrap();
        let v = ema.update_bar(&bar("40")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(30)));
    }

    #[test]
    fn test_ema_is_ready_after_period() {
        let mut ema = Ema::new("ema3", 3).unwrap();
        ema.update_bar(&bar("10")).unwrap();
        ema.update_bar(&bar("20")).unwrap();
        assert!(!ema.is_ready());
        ema.update_bar(&bar("30")).unwrap();
        assert!(ema.is_ready());
    }

    #[test]
    fn test_ema_period_0_returns_invalid_period_error() {
        let result = Ema::new("ema0", 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_ema_single_value_period_1() {
        let mut ema = Ema::new("ema1", 1).unwrap();
        let v = ema.update_bar(&bar("42")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(42)));
        assert!(ema.is_ready());
    }

    #[test]
    fn test_ema_convergence_to_constant_series() {
        let mut ema = Ema::new("ema5", 5).unwrap();
        for p in &["10", "20", "30", "40", "50"] {
            ema.update_bar(&bar(p)).unwrap();
        }
        let mut last = dec!(0);
        for _ in 0..30 {
            if let SignalValue::Scalar(v) = ema.update_bar(&bar("100")).unwrap() {
                last = v;
            }
        }
        let diff = (last - dec!(100)).abs();
        assert!(
            diff < dec!(1),
            "EMA must converge within 1 of 100 after 30 bars, got {last}"
        );
    }

    #[test]
    fn test_ema_faster_than_sma() {
        let period = 5;
        let mut ema = Ema::new("ema5", period).unwrap();
        let mut sma = Sma::new("sma5", period).unwrap();

        for _ in 0..period {
            ema.update_bar(&bar("100")).unwrap();
            sma.update_bar(&bar("100")).unwrap();
        }

        let ema_val = match ema.update_bar(&bar("200")).unwrap() {
            SignalValue::Scalar(v) => v,
            _ => panic!("EMA should be ready after period bars"),
        };
        let sma_val = match sma.update_bar(&bar("200")).unwrap() {
            SignalValue::Scalar(v) => v,
            _ => panic!("SMA should be ready after period bars"),
        };

        assert!(
            ema_val > sma_val,
            "EMA ({ema_val}) should exceed SMA ({sma_val}) immediately after a spike"
        );
    }

    #[test]
    fn test_ema_reset_clears_state() {
        let mut ema = Ema::new("ema3", 3).unwrap();
        for p in &["10", "20", "30"] {
            ema.update_bar(&bar(p)).unwrap();
        }
        assert!(ema.is_ready());
        ema.reset();
        assert!(!ema.is_ready());
        let v = ema.update_bar(&bar("10")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_ema_small_positive_values_no_panic() {
        let mut ema = Ema::new("ema3", 3).unwrap();
        for p in &["0.001", "0.002", "0.003", "0.004"] {
            let result = ema.update_bar(&bar(p));
            assert!(result.is_ok());
        }
    }
}
