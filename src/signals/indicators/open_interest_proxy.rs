//! Open Interest Proxy indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Open Interest Proxy.
///
/// Approximates open interest momentum using volume and price direction.
/// When price moves up on higher volume, new long positions are likely being opened.
/// When price moves down on higher volume, new short positions are likely being opened.
///
/// Formula per bar:
/// - `signed_vol = volume * sign(close - open)`
/// - `oip = rolling_sum(signed_vol, period) / rolling_sum(volume, period)`
///
/// Result is a ratio ∈ [−1, +1]:
/// - +1: all volume is on bullish bars (net long pressure).
/// - −1: all volume is on bearish bars (net short pressure).
/// - 0: balanced or zero volume.
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
/// Returns `SignalValue::Scalar(0.0)` when total volume is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::OpenInterestProxy;
/// use fin_primitives::signals::Signal;
/// let oip = OpenInterestProxy::new("oip_14", 14).unwrap();
/// assert_eq!(oip.period(), 14);
/// ```
pub struct OpenInterestProxy {
    name: String,
    period: usize,
    signed_vols: VecDeque<Decimal>,
    volumes: VecDeque<Decimal>,
}

impl OpenInterestProxy {
    /// Constructs a new `OpenInterestProxy`.
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
            signed_vols: VecDeque::with_capacity(period),
            volumes: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for OpenInterestProxy {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let direction = if bar.close > bar.open {
            Decimal::ONE
        } else if bar.close < bar.open {
            Decimal::NEGATIVE_ONE
        } else {
            Decimal::ZERO
        };

        let signed_vol = bar.volume
            .checked_mul(direction)
            .ok_or(FinError::ArithmeticOverflow)?;

        self.signed_vols.push_back(signed_vol);
        self.volumes.push_back(bar.volume);

        if self.signed_vols.len() > self.period {
            self.signed_vols.pop_front();
            self.volumes.pop_front();
        }
        if self.signed_vols.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let total_vol: Decimal = self.volumes.iter().copied().sum();
        if total_vol.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let net_vol: Decimal = self.signed_vols.iter().copied().sum();
        let ratio = net_vol.checked_div(total_vol).ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(ratio))
    }

    fn is_ready(&self) -> bool {
        self.signed_vols.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.signed_vols.clear();
        self.volumes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, c: &str, vol: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cl = Price::new(c.parse().unwrap()).unwrap();
        let hi = if op > cl { op } else { cl };
        let lo = if op < cl { op } else { cl };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hi, low: lo, close: cl,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_period_zero_fails() {
        assert!(matches!(OpenInterestProxy::new("oip", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut oip = OpenInterestProxy::new("oip", 3).unwrap();
        assert_eq!(oip.update_bar(&bar("10", "12", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_all_bullish_gives_one() {
        let mut oip = OpenInterestProxy::new("oip", 3).unwrap();
        for _ in 0..3 {
            oip.update_bar(&bar("10", "12", "100")).unwrap();
        }
        let v = oip.update_bar(&bar("10", "12", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_all_bearish_gives_neg_one() {
        let mut oip = OpenInterestProxy::new("oip", 3).unwrap();
        for _ in 0..3 {
            oip.update_bar(&bar("12", "10", "100")).unwrap();
        }
        let v = oip.update_bar(&bar("12", "10", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_zero_volume_gives_zero() {
        let mut oip = OpenInterestProxy::new("oip", 3).unwrap();
        for _ in 0..3 {
            oip.update_bar(&bar("10", "12", "0")).unwrap();
        }
        let v = oip.update_bar(&bar("10", "12", "0")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_reset() {
        let mut oip = OpenInterestProxy::new("oip", 2).unwrap();
        oip.update_bar(&bar("10", "12", "100")).unwrap();
        oip.update_bar(&bar("10", "12", "100")).unwrap();
        assert!(oip.is_ready());
        oip.reset();
        assert!(!oip.is_ready());
    }
}
