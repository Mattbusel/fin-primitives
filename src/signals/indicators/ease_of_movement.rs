//! Ease of Movement — volume-adjusted directional movement oscillator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Ease of Movement (EMV) — `((H+L)/2 - (pH+pL)/2) / (volume / (H-L))`.
///
/// Measures how easily price moves by combining directional midpoint change
/// with volume-to-range ratio. A smoothed (SMA) version is returned:
/// - **Positive**: price rising with ease (low volume, wide range moves up).
/// - **Negative**: price falling with ease (low volume, wide range moves down).
/// - **Near zero**: price movement requires high volume effort.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated,
/// or when `high == low` or `volume == 0` on the current bar.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::EaseOfMovement;
/// use fin_primitives::signals::Signal;
/// let emv = EaseOfMovement::new("emv_14", 14).unwrap();
/// assert_eq!(emv.period(), 14);
/// ```
pub struct EaseOfMovement {
    name: String,
    period: usize,
    prev_mid: Option<Decimal>,
    raw_window: VecDeque<Decimal>,
    raw_sum: Decimal,
}

impl EaseOfMovement {
    /// Constructs a new `EaseOfMovement`.
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
            prev_mid: None,
            raw_window: VecDeque::with_capacity(period),
            raw_sum: Decimal::ZERO,
        })
    }
}

impl Signal for EaseOfMovement {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.raw_window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let mid = (bar.high + bar.low)
            .checked_div(Decimal::TWO)
            .ok_or(FinError::ArithmeticOverflow)?;

        let raw_emv = if let Some(prev_mid) = self.prev_mid {
            let range = bar.high - bar.low;
            if range.is_zero() || bar.volume.is_zero() {
                self.prev_mid = Some(mid);
                return Ok(SignalValue::Unavailable);
            }
            let mid_move = mid - prev_mid;
            let box_ratio = bar.volume
                .checked_div(range)
                .ok_or(FinError::ArithmeticOverflow)?;
            mid_move
                .checked_div(box_ratio)
                .ok_or(FinError::ArithmeticOverflow)?
        } else {
            self.prev_mid = Some(mid);
            return Ok(SignalValue::Unavailable);
        };

        self.prev_mid = Some(mid);
        self.raw_sum += raw_emv;
        self.raw_window.push_back(raw_emv);

        if self.raw_window.len() > self.period {
            let removed = self.raw_window.pop_front().unwrap();
            self.raw_sum -= removed;
        }

        if self.raw_window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sma = self.raw_sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(sma))
    }

    fn reset(&mut self) {
        self.prev_mid = None;
        self.raw_window.clear();
        self.raw_sum = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str, c: &str, vol: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: cp,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_emv_invalid_period() {
        assert!(EaseOfMovement::new("emv", 0).is_err());
    }

    #[test]
    fn test_emv_unavailable_initially() {
        let mut s = EaseOfMovement::new("emv", 3).unwrap();
        // First bar: no prev_mid → Unavailable
        assert_eq!(s.update_bar(&bar("110","90","100","1000")).unwrap(), SignalValue::Unavailable);
        assert!(!s.is_ready());
    }

    #[test]
    fn test_emv_positive_on_rising_prices() {
        let mut s = EaseOfMovement::new("emv", 2).unwrap();
        // Seed bar
        s.update_bar(&bar("110","90","100","1000")).unwrap();
        // Rising bar: mid goes from 100 to 110, decent range, low volume
        s.update_bar(&bar("120","100","110","500")).unwrap();
        let v = s.update_bar(&bar("130","110","120","500")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!(r > dec!(0), "rising low-volume should give positive EMV: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_emv_zero_range_gives_unavailable() {
        let mut s = EaseOfMovement::new("emv", 2).unwrap();
        s.update_bar(&bar("110","90","100","1000")).unwrap();
        // Same high and low → range=0 → Unavailable
        let v = s.update_bar(&bar("100","100","100","1000")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_emv_reset() {
        let mut s = EaseOfMovement::new("emv", 2).unwrap();
        s.update_bar(&bar("110","90","100","1000")).unwrap();
        s.update_bar(&bar("120","100","110","500")).unwrap();
        s.update_bar(&bar("130","110","120","500")).unwrap();
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
