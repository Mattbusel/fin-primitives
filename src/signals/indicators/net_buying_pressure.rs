//! Net Buying Pressure indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Net Buying Pressure.
///
/// Estimates net buying pressure per bar by measuring:
/// - Where the close is within the range: `(close - low) / (high - low)`
/// - Multiplied by volume, compared to average.
///
/// Per-bar buying pressure: `bp = volume * (close - low) / (high - low)`
/// Per-bar selling pressure: `sp = volume * (high - close) / (high - low)`
/// Net: `nbp = bp - sp = volume * (2*close - high - low) / (high - low)`
///
/// Rolling: `sum(nbp, period) / sum(volume, period)` → normalized ∈ [−1, +1].
///
/// - +1: all volume into closes at the high.
/// - −1: all volume into closes at the low.
/// - 0: balanced or zero-range/zero-volume.
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::NetBuyingPressure;
/// use fin_primitives::signals::Signal;
/// let nbp = NetBuyingPressure::new("nbp_14", 14).unwrap();
/// assert_eq!(nbp.period(), 14);
/// ```
pub struct NetBuyingPressure {
    name: String,
    period: usize,
    net_pressures: VecDeque<Decimal>,
    volumes: VecDeque<Decimal>,
}

impl NetBuyingPressure {
    /// Constructs a new `NetBuyingPressure`.
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
            net_pressures: VecDeque::with_capacity(period),
            volumes: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for NetBuyingPressure {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        let net_p = if range.is_zero() {
            Decimal::ZERO
        } else {
            let numerator = (bar.close + bar.close) - bar.high - bar.low;
            let ratio = numerator.checked_div(range).ok_or(FinError::ArithmeticOverflow)?;
            bar.volume.checked_mul(ratio).ok_or(FinError::ArithmeticOverflow)?
        };

        self.net_pressures.push_back(net_p);
        self.volumes.push_back(bar.volume);

        if self.net_pressures.len() > self.period {
            self.net_pressures.pop_front();
            self.volumes.pop_front();
        }
        if self.net_pressures.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let total_vol: Decimal = self.volumes.iter().copied().sum();
        if total_vol.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let net_sum: Decimal = self.net_pressures.iter().copied().sum();
        let normalized = net_sum.checked_div(total_vol).ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(normalized))
    }

    fn is_ready(&self) -> bool {
        self.net_pressures.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.net_pressures.clear();
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

    fn bar(h: &str, l: &str, c: &str, vol: &str) -> OhlcvBar {
        let hi = Price::new(h.parse().unwrap()).unwrap();
        let lo = Price::new(l.parse().unwrap()).unwrap();
        let cl = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lo, high: hi, low: lo, close: cl,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_period_zero_fails() {
        assert!(matches!(NetBuyingPressure::new("nbp", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut nbp = NetBuyingPressure::new("nbp", 3).unwrap();
        assert_eq!(nbp.update_bar(&bar("12", "10", "11", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_close_at_high_gives_one() {
        let mut nbp = NetBuyingPressure::new("nbp", 3).unwrap();
        for _ in 0..3 {
            nbp.update_bar(&bar("12", "10", "12", "100")).unwrap(); // close=high → +1
        }
        let v = nbp.update_bar(&bar("12", "10", "12", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_close_at_low_gives_neg_one() {
        let mut nbp = NetBuyingPressure::new("nbp", 3).unwrap();
        for _ in 0..3 {
            nbp.update_bar(&bar("12", "10", "10", "100")).unwrap(); // close=low → -1
        }
        let v = nbp.update_bar(&bar("12", "10", "10", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_reset() {
        let mut nbp = NetBuyingPressure::new("nbp", 2).unwrap();
        nbp.update_bar(&bar("12", "10", "11", "100")).unwrap();
        nbp.update_bar(&bar("12", "10", "11", "100")).unwrap();
        assert!(nbp.is_ready());
        nbp.reset();
        assert!(!nbp.is_ready());
    }
}
