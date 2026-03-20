//! Relative Strength Index (RSI) indicator, Wilder smoothing.
//!
//! RSI is a momentum oscillator that measures the speed and change of price movements.
//! It oscillates between 0 and 100. Traditional interpretation:
//! - RSI >= 70: potentially overbought
//! - RSI <= 30: potentially oversold

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Relative Strength Index using Wilder's smoothing method.
///
/// Requires `period + 1` bars before producing a value: the first bar sets the
/// previous close, and then `period` price changes are needed to seed the initial
/// average gain/loss. Subsequent bars apply Wilder smoothing:
/// `avg_gain = (prev_avg_gain * (period - 1) + gain) / period`.
///
/// Always returns a value in `[0, 100]`. Returns [`SignalValue::Unavailable`] until
/// the warm-up period is complete.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Rsi;
/// use fin_primitives::signals::Signal;
/// let rsi = Rsi::new("rsi14", 14).unwrap();
/// assert_eq!(rsi.period(), 14);
/// assert!(!rsi.is_ready());
/// ```
pub struct Rsi {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    avg_gain: Option<Decimal>,
    avg_loss: Option<Decimal>,
    /// Number of price changes accumulated during the seed phase.
    count: usize,
    /// Accumulator for gains during the seed phase.
    seed_gain: Decimal,
    /// Accumulator for losses during the seed phase.
    seed_loss: Decimal,
}

impl Rsi {
    /// Constructs a new `Rsi` with the given name and period.
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
            prev_close: None,
            avg_gain: None,
            avg_loss: None,
            count: 0,
            seed_gain: Decimal::ZERO,
            seed_loss: Decimal::ZERO,
        })
    }

    /// Computes RSI from average gain and average loss.
    ///
    /// Returns 100 when `avg_loss` is zero (all gains), 0 when `avg_gain` is zero (all losses).
    fn compute_rsi(avg_gain: Decimal, avg_loss: Decimal) -> Result<Decimal, FinError> {
        if avg_loss == Decimal::ZERO {
            return Ok(Decimal::ONE_HUNDRED);
        }
        let rs = avg_gain
            .checked_div(avg_loss)
            .ok_or(FinError::ArithmeticOverflow)?;
        // RSI = 100 - (100 / (1 + RS))
        let one_plus_rs = Decimal::ONE
            .checked_add(rs)
            .ok_or(FinError::ArithmeticOverflow)?;
        let hundred_div = Decimal::ONE_HUNDRED
            .checked_div(one_plus_rs)
            .ok_or(FinError::ArithmeticOverflow)?;
        let rsi = Decimal::ONE_HUNDRED
            .checked_sub(hundred_div)
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(rsi)
    }
}

impl Signal for Rsi {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let close = bar.close;

        let Some(prev) = self.prev_close else {
            // First bar: just record close, no change yet.
            self.prev_close = Some(close);
            return Ok(SignalValue::Unavailable);
        };
        self.prev_close = Some(close);

        let change = close - prev;
        let gain = if change > Decimal::ZERO {
            change
        } else {
            Decimal::ZERO
        };
        let loss = if change < Decimal::ZERO {
            -change
        } else {
            Decimal::ZERO
        };

        if self.avg_gain.is_none() {
            // Seed phase: accumulate `period` changes.
            self.seed_gain += gain;
            self.seed_loss += loss;
            self.count += 1;

            if self.count < self.period {
                return Ok(SignalValue::Unavailable);
            }

            // Seed complete: compute initial averages.
            #[allow(clippy::cast_possible_truncation)]
            let period_d = Decimal::from(self.period as u32);
            let ag = self
                .seed_gain
                .checked_div(period_d)
                .ok_or(FinError::ArithmeticOverflow)?;
            let al = self
                .seed_loss
                .checked_div(period_d)
                .ok_or(FinError::ArithmeticOverflow)?;
            self.avg_gain = Some(ag);
            self.avg_loss = Some(al);

            let rsi = Self::compute_rsi(ag, al)?;
            return Ok(SignalValue::Scalar(rsi));
        }

        // Wilder smoothing phase.
        #[allow(clippy::similar_names)]
        let prev_avg_gain = self.avg_gain.unwrap_or(Decimal::ZERO);
        #[allow(clippy::similar_names)]
        let prev_avg_loss = self.avg_loss.unwrap_or(Decimal::ZERO);
        #[allow(clippy::cast_possible_truncation)]
        let period_d = Decimal::from(self.period as u32);
        let period_minus_1 = period_d
            .checked_sub(Decimal::ONE)
            .ok_or(FinError::ArithmeticOverflow)?;

        let ag = prev_avg_gain
            .checked_mul(period_minus_1)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_add(gain)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_div(period_d)
            .ok_or(FinError::ArithmeticOverflow)?;

        let al = prev_avg_loss
            .checked_mul(period_minus_1)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_add(loss)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_div(period_d)
            .ok_or(FinError::ArithmeticOverflow)?;

        self.avg_gain = Some(ag);
        self.avg_loss = Some(al);

        let rsi = Self::compute_rsi(ag, al)?;
        Ok(SignalValue::Scalar(rsi))
    }

    fn is_ready(&self) -> bool {
        self.avg_gain.is_some()
    }

    fn period(&self) -> usize {
        self.period
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
    fn test_rsi_unavailable_before_period_plus_one() {
        let mut rsi = Rsi::new("rsi3", 3).unwrap();
        let v1 = rsi.update_bar(&bar("100")).unwrap();
        let v2 = rsi.update_bar(&bar("102")).unwrap();
        let v3 = rsi.update_bar(&bar("104")).unwrap();
        assert_eq!(v1, SignalValue::Unavailable);
        assert_eq!(v2, SignalValue::Unavailable);
        assert_eq!(v3, SignalValue::Unavailable);
        assert!(!rsi.is_ready());
    }

    #[test]
    fn test_rsi_ready_after_period_plus_one_bars() {
        let mut rsi = Rsi::new("rsi3", 3).unwrap();
        for p in &["100", "102", "104", "106"] {
            rsi.update_bar(&bar(p)).unwrap();
        }
        assert!(rsi.is_ready());
    }

    #[test]
    fn test_rsi_scalar_value_on_period_plus_one_bar() {
        let mut rsi = Rsi::new("rsi3", 3).unwrap();
        let _ = rsi.update_bar(&bar("100")).unwrap();
        let _ = rsi.update_bar(&bar("102")).unwrap();
        let _ = rsi.update_bar(&bar("104")).unwrap();
        let v = rsi.update_bar(&bar("106")).unwrap();
        assert!(matches!(v, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_rsi_all_gains_equals_100() {
        let mut rsi = Rsi::new("rsi3", 3).unwrap();
        for p in &["100", "101", "102", "103", "104"] {
            let _ = rsi.update_bar(&bar(p)).unwrap();
        }
        let v = rsi.update_bar(&bar("105")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_rsi_all_losses_equals_zero() {
        let mut rsi = Rsi::new("rsi3", 3).unwrap();
        for p in &["100", "99", "98", "97", "96"] {
            let _ = rsi.update_bar(&bar(p)).unwrap();
        }
        let v = rsi.update_bar(&bar("95")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rsi_in_bounds() {
        let mut rsi = Rsi::new("rsi5", 5).unwrap();
        let closes = [
            "100", "102", "101", "103", "102", "104", "103", "105", "104", "106",
        ];
        for &c in &closes {
            if let SignalValue::Scalar(v) = rsi.update_bar(&bar(c)).unwrap() {
                assert!(v >= dec!(0), "RSI < 0: {v}");
                assert!(v <= dec!(100), "RSI > 100: {v}");
            }
        }
    }

    #[test]
    fn test_rsi_period_returns_configured_value() {
        let rsi = Rsi::new("rsi14", 14).unwrap();
        assert_eq!(rsi.period(), 14);
    }

    #[test]
    fn test_rsi_name_returns_configured_value() {
        let rsi = Rsi::new("my_rsi", 14).unwrap();
        assert_eq!(rsi.name(), "my_rsi");
    }

    #[test]
    fn test_rsi_equal_up_down_moves_stays_in_range() {
        let mut rsi = Rsi::new("rsi4", 4).unwrap();
        let prices = [
            "100", "110", "100", "110", "100", "110", "100", "110", "100", "110", "100", "110",
            "100", "110", "100", "110",
        ];
        let mut last_val: Option<Decimal> = None;
        for p in &prices {
            if let SignalValue::Scalar(v) = rsi.update_bar(&bar(p)).unwrap() {
                last_val = Some(v);
            }
        }
        let val = last_val.expect("RSI must produce a value");
        assert!(val >= dec!(0));
        assert!(val <= dec!(100));
    }

    #[test]
    fn test_rsi_overbought_at_70() {
        let mut rsi = Rsi::new("rsi14", 14).unwrap();
        for i in 0u32..=14 {
            rsi.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        assert!(rsi.is_ready());
        let v = rsi.update_bar(&bar("116")).unwrap();
        if let SignalValue::Scalar(val) = v {
            assert!(val >= dec!(70));
        } else {
            panic!("expected Scalar after period+1 bars, got Unavailable");
        }
    }

    #[test]
    fn test_rsi_fewer_bars_than_period_returns_unavailable() {
        let mut rsi = Rsi::new("rsi14", 14).unwrap();
        let mut any_scalar = false;
        for i in 0..10u32 {
            if let SignalValue::Scalar(_) = rsi.update_bar(&bar(&(100 + i).to_string())).unwrap() {
                any_scalar = true;
            }
        }
        assert!(!any_scalar);
        assert!(!rsi.is_ready());
    }
}
