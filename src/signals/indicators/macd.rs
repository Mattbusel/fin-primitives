//! Moving Average Convergence Divergence (MACD) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// MACD indicator returning the histogram: `(fast_ema - slow_ema) - signal_ema`.
///
/// Internals:
/// - `fast_ema`: EMA over `fast_period` closes (typically 12)
/// - `slow_ema`: EMA over `slow_period` closes (typically 26)
/// - `signal_ema`: EMA of `(fast - slow)` over `signal_period` (typically 9)
///
/// Returns `SignalValue::Unavailable` until enough bars are accumulated for all three EMAs.
/// Specifically: `slow_period + signal_period - 1` bars are required.
///
/// The histogram is the most actionable single value: positive = bullish momentum,
/// negative = bearish momentum, crossing zero = potential trend change.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Macd;
/// use fin_primitives::signals::Signal;
/// let macd = Macd::new("macd", 12, 26, 9).unwrap();
/// assert_eq!(macd.period(), 26);
/// ```
pub struct Macd {
    name: String,
    fast_period: usize,
    slow_period: usize,
    signal_period: usize,

    // Fast EMA state
    fast_count: usize,
    fast_seed_sum: Decimal,
    fast_ema: Option<Decimal>,
    fast_multiplier: Decimal,

    // Slow EMA state
    slow_count: usize,
    slow_seed_sum: Decimal,
    slow_ema: Option<Decimal>,
    slow_multiplier: Decimal,

    // Signal EMA state (EMA of the MACD line = fast - slow)
    signal_count: usize,
    signal_seed_sum: Decimal,
    signal_ema: Option<Decimal>,
    signal_multiplier: Decimal,
}

impl Macd {
    /// Constructs a new `Macd` indicator.
    ///
    /// # Errors
    /// Returns [`crate::error::FinError::InvalidPeriod`] if any period is zero,
    /// or if `fast_period >= slow_period`.
    pub fn new(
        name: impl Into<String>,
        fast_period: usize,
        slow_period: usize,
        signal_period: usize,
    ) -> Result<Self, crate::error::FinError> {
        if fast_period == 0 {
            return Err(crate::error::FinError::InvalidPeriod(fast_period));
        }
        if slow_period == 0 {
            return Err(crate::error::FinError::InvalidPeriod(slow_period));
        }
        if signal_period == 0 {
            return Err(crate::error::FinError::InvalidPeriod(signal_period));
        }
        if fast_period >= slow_period {
            return Err(crate::error::FinError::InvalidPeriod(fast_period));
        }

        #[allow(clippy::cast_possible_truncation)]
        let fast_multiplier = Decimal::TWO
            .checked_div(Decimal::from((fast_period + 1) as u32))
            .unwrap_or(Decimal::ONE);
        #[allow(clippy::cast_possible_truncation)]
        let slow_multiplier = Decimal::TWO
            .checked_div(Decimal::from((slow_period + 1) as u32))
            .unwrap_or(Decimal::ONE);
        #[allow(clippy::cast_possible_truncation)]
        let signal_multiplier = Decimal::TWO
            .checked_div(Decimal::from((signal_period + 1) as u32))
            .unwrap_or(Decimal::ONE);

        Ok(Self {
            name: name.into(),
            fast_period,
            slow_period,
            signal_period,
            fast_count: 0,
            fast_seed_sum: Decimal::ZERO,
            fast_ema: None,
            fast_multiplier,
            slow_count: 0,
            slow_seed_sum: Decimal::ZERO,
            slow_ema: None,
            slow_multiplier,
            signal_count: 0,
            signal_seed_sum: Decimal::ZERO,
            signal_ema: None,
            signal_multiplier,
        })
    }

    fn ema_step(
        count: &mut usize,
        seed_sum: &mut Decimal,
        current: &mut Option<Decimal>,
        multiplier: Decimal,
        period: usize,
        value: Decimal,
    ) -> Result<Option<Decimal>, FinError> {
        *count += 1;
        if *count <= period {
            *seed_sum += value;
            if *count == period {
                #[allow(clippy::cast_possible_truncation)]
                let seed = seed_sum
                    .checked_div(Decimal::from(period as u32))
                    .ok_or(FinError::ArithmeticOverflow)?;
                *current = Some(seed);
                return Ok(Some(seed));
            }
            return Ok(None);
        }
        let prev = current.unwrap_or(Decimal::ZERO);
        let one_minus_k = Decimal::ONE
            .checked_sub(multiplier)
            .ok_or(FinError::ArithmeticOverflow)?;
        let ema = value
            .checked_mul(multiplier)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_add(
                prev.checked_mul(one_minus_k)
                    .ok_or(FinError::ArithmeticOverflow)?,
            )
            .ok_or(FinError::ArithmeticOverflow)?;
        *current = Some(ema);
        Ok(Some(ema))
    }
}

impl Signal for Macd {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let close = bar.close;

        let fast_val = Self::ema_step(
            &mut self.fast_count,
            &mut self.fast_seed_sum,
            &mut self.fast_ema,
            self.fast_multiplier,
            self.fast_period,
            close,
        )?;

        let slow_val = Self::ema_step(
            &mut self.slow_count,
            &mut self.slow_seed_sum,
            &mut self.slow_ema,
            self.slow_multiplier,
            self.slow_period,
            close,
        )?;

        // MACD line is only available once the slow EMA is ready.
        let (fast, slow) = match (fast_val, slow_val) {
            (Some(f), Some(s)) => (f, s),
            _ => return Ok(SignalValue::Unavailable),
        };

        let macd_line = fast
            .checked_sub(slow)
            .ok_or(FinError::ArithmeticOverflow)?;

        let signal_val = Self::ema_step(
            &mut self.signal_count,
            &mut self.signal_seed_sum,
            &mut self.signal_ema,
            self.signal_multiplier,
            self.signal_period,
            macd_line,
        )?;

        match signal_val {
            None => Ok(SignalValue::Unavailable),
            Some(sig) => {
                let histogram = macd_line
                    .checked_sub(sig)
                    .ok_or(FinError::ArithmeticOverflow)?;
                Ok(SignalValue::Scalar(histogram))
            }
        }
    }

    fn is_ready(&self) -> bool {
        self.signal_ema.is_some()
    }

    /// Returns `slow_period`, the dominant warm-up period.
    fn period(&self) -> usize {
        self.slow_period
    }

    fn reset(&mut self) {
        self.fast_count = 0;
        self.fast_seed_sum = Decimal::ZERO;
        self.fast_ema = None;
        self.slow_count = 0;
        self.slow_seed_sum = Decimal::ZERO;
        self.slow_ema = None;
        self.signal_count = 0;
        self.signal_seed_sum = Decimal::ZERO;
        self.signal_ema = None;
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
    fn test_macd_period_0_fails() {
        assert!(Macd::new("m", 0, 26, 9).is_err());
        assert!(Macd::new("m", 12, 0, 9).is_err());
        assert!(Macd::new("m", 12, 26, 0).is_err());
    }

    #[test]
    fn test_macd_fast_ge_slow_fails() {
        assert!(Macd::new("m", 26, 12, 9).is_err());
        assert!(Macd::new("m", 12, 12, 9).is_err());
    }

    #[test]
    fn test_macd_unavailable_before_warmup() {
        let mut macd = Macd::new("macd", 3, 5, 2).unwrap();
        // Need slow(5) + signal(2) - 1 = 6 bars before first value
        for _ in 0..5 {
            let v = macd.update_bar(&bar("100")).unwrap();
            assert_eq!(v, SignalValue::Unavailable);
        }
        assert!(!macd.is_ready());
    }

    #[test]
    fn test_macd_ready_after_warmup() {
        let mut macd = Macd::new("macd", 3, 5, 2).unwrap();
        let mut last = SignalValue::Unavailable;
        // slow(5) + signal(2) - 1 = 6 bars needed
        for _ in 0..6 {
            last = macd.update_bar(&bar("100")).unwrap();
        }
        assert!(macd.is_ready());
        // Constant price → MACD line = 0, histogram = 0
        assert_eq!(last, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_macd_histogram_positive_on_rising_prices() {
        let mut macd = Macd::new("macd", 3, 5, 2).unwrap();
        // Warm up
        for _ in 0..6 {
            macd.update_bar(&bar("100")).unwrap();
        }
        // Feed rising prices — fast EMA should lead slow EMA upward
        for i in 1..=10i32 {
            let p = format!("{}", 100 + i * 2);
            macd.update_bar(&bar(&p)).unwrap();
        }
        let v = macd.update_bar(&bar("130")).unwrap();
        if let SignalValue::Scalar(h) = v {
            assert!(h > dec!(0), "histogram should be positive on sustained rise, got {h}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_macd_period_returns_slow_period() {
        let macd = Macd::new("macd", 12, 26, 9).unwrap();
        assert_eq!(macd.period(), 26);
    }
}
