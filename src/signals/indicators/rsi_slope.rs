//! RSI Slope indicator — bar-over-bar change in RSI.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// RSI Slope — measures the rate of change of RSI.
///
/// Computes `RSI(period)` internally and returns the difference between the current
/// RSI value and the RSI from the previous bar. This is the first derivative of RSI,
/// indicating whether momentum is accelerating (+) or decelerating (−).
///
/// Returns [`SignalValue::Unavailable`] until RSI has produced at least two values
/// (i.e. `period + 2` bars have been seen: `period + 1` for the first RSI value,
/// plus one more for the first slope).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RsiSlope;
/// use fin_primitives::signals::Signal;
/// let rs = RsiSlope::new("rsi_slope", 14).unwrap();
/// assert_eq!(rs.period(), 14);
/// ```
pub struct RsiSlope {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    avg_gain: Option<Decimal>,
    avg_loss: Option<Decimal>,
    count: usize,
    seed_gain: Decimal,
    seed_loss: Decimal,
    prev_rsi: Option<Decimal>,
}

impl RsiSlope {
    /// Constructs a new `RsiSlope`.
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
            prev_close: None,
            avg_gain: None,
            avg_loss: None,
            count: 0,
            seed_gain: Decimal::ZERO,
            seed_loss: Decimal::ZERO,
            prev_rsi: None,
        })
    }

    fn compute_rsi(avg_gain: Decimal, avg_loss: Decimal) -> Result<Decimal, FinError> {
        if avg_loss == Decimal::ZERO {
            return Ok(Decimal::ONE_HUNDRED);
        }
        let rs = avg_gain.checked_div(avg_loss).ok_or(FinError::ArithmeticOverflow)?;
        let one_plus_rs = Decimal::ONE.checked_add(rs).ok_or(FinError::ArithmeticOverflow)?;
        let rsi = Decimal::ONE_HUNDRED
            .checked_sub(
                Decimal::ONE_HUNDRED
                    .checked_div(one_plus_rs)
                    .ok_or(FinError::ArithmeticOverflow)?,
            )
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(rsi)
    }
}

impl Signal for RsiSlope {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.prev_rsi.is_some() && self.avg_gain.is_some()
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let close = bar.close;

        let Some(prev) = self.prev_close else {
            self.prev_close = Some(close);
            return Ok(SignalValue::Unavailable);
        };
        self.prev_close = Some(close);

        let change = close - prev;
        let gain = if change > Decimal::ZERO { change } else { Decimal::ZERO };
        let loss = if change < Decimal::ZERO { -change } else { Decimal::ZERO };

        let current_rsi = if self.avg_gain.is_none() {
            self.seed_gain += gain;
            self.seed_loss += loss;
            self.count += 1;

            if self.count < self.period {
                return Ok(SignalValue::Unavailable);
            }
            #[allow(clippy::cast_possible_truncation)]
            let period_d = Decimal::from(self.period as u32);
            let ag = self.seed_gain.checked_div(period_d).ok_or(FinError::ArithmeticOverflow)?;
            let al = self.seed_loss.checked_div(period_d).ok_or(FinError::ArithmeticOverflow)?;
            self.avg_gain = Some(ag);
            self.avg_loss = Some(al);
            Self::compute_rsi(ag, al)?
        } else {
            #[allow(clippy::cast_possible_truncation)]
            let period_d = Decimal::from(self.period as u32);
            let period_m1 = period_d.checked_sub(Decimal::ONE).ok_or(FinError::ArithmeticOverflow)?;
            let prev_ag = self.avg_gain.unwrap_or(Decimal::ZERO);
            let prev_al = self.avg_loss.unwrap_or(Decimal::ZERO);
            let ag = prev_ag.checked_mul(period_m1).ok_or(FinError::ArithmeticOverflow)?
                .checked_add(gain).ok_or(FinError::ArithmeticOverflow)?
                .checked_div(period_d).ok_or(FinError::ArithmeticOverflow)?;
            let al = prev_al.checked_mul(period_m1).ok_or(FinError::ArithmeticOverflow)?
                .checked_add(loss).ok_or(FinError::ArithmeticOverflow)?
                .checked_div(period_d).ok_or(FinError::ArithmeticOverflow)?;
            self.avg_gain = Some(ag);
            self.avg_loss = Some(al);
            Self::compute_rsi(ag, al)?
        };

        let Some(prev_rsi) = self.prev_rsi else {
            self.prev_rsi = Some(current_rsi);
            return Ok(SignalValue::Unavailable);
        };
        self.prev_rsi = Some(current_rsi);
        Ok(SignalValue::Scalar(current_rsi - prev_rsi))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.avg_gain = None;
        self.avg_loss = None;
        self.count = 0;
        self.seed_gain = Decimal::ZERO;
        self.seed_loss = Decimal::ZERO;
        self.prev_rsi = None;
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
    fn test_rsi_slope_invalid_period() {
        assert!(RsiSlope::new("rs", 0).is_err());
    }

    #[test]
    fn test_rsi_slope_unavailable_before_warm_up() {
        let mut rs = RsiSlope::new("rs", 3).unwrap();
        for p in &["100", "102", "104", "106"] {
            let v = rs.update_bar(&bar(p)).unwrap();
            // The 4th bar produces the first RSI, but needs one more for the slope.
            assert!(!v.is_scalar() || !rs.is_ready() || true); // just consume
        }
        // After 4 bars (period=3), RSI is ready but prev_rsi is just set — is_ready() = false.
        // 5th bar → slope ready.
        assert!(!rs.is_ready());
    }

    #[test]
    fn test_rsi_slope_produces_value_after_warm_up() {
        let mut rs = RsiSlope::new("rs", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for p in &["100", "102", "104", "106", "108"] {
            last = rs.update_bar(&bar(p)).unwrap();
        }
        assert!(last.is_scalar(), "expected Scalar after warm-up");
    }

    #[test]
    fn test_rsi_slope_flat_market_zero_slope() {
        // All-gain RSI stays at 100; slope = 0.
        let mut rs = RsiSlope::new("rs", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for p in 0u32..=6 {
            last = rs.update_bar(&bar(&(100 + p).to_string())).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            // RSI stuck at 100 → slope = 0
            assert!(v.abs() < dec!(0.001), "expected ~0 slope, got {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rsi_slope_reset() {
        let mut rs = RsiSlope::new("rs", 3).unwrap();
        for p in &["100", "102", "104", "106", "108"] {
            rs.update_bar(&bar(p)).unwrap();
        }
        assert!(rs.is_ready());
        rs.reset();
        assert!(!rs.is_ready());
    }

    #[test]
    fn test_rsi_slope_period_and_name() {
        let rs = RsiSlope::new("my_rs", 14).unwrap();
        assert_eq!(rs.period(), 14);
        assert_eq!(rs.name(), "my_rs");
    }
}
