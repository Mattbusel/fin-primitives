//! Chande Momentum Oscillator (CMO) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Chande Momentum Oscillator over `period` bars.
///
/// `CMO = (up_sum - down_sum) / (up_sum + down_sum) × 100`
///
/// Where `up_sum` is the sum of gains and `down_sum` is the sum of losses
/// over the rolling `period`. Values range from -100 to +100.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Cmo;
/// use fin_primitives::signals::Signal;
///
/// let mut cmo = Cmo::new("cmo14", 14).unwrap();
/// assert_eq!(cmo.period(), 14);
/// ```
pub struct Cmo {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    changes: VecDeque<Decimal>,
}

impl Cmo {
    /// Constructs a new `Cmo`.
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
            changes: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for Cmo {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let Some(pc) = self.prev_close else {
            self.prev_close = Some(bar.close);
            return Ok(SignalValue::Unavailable);
        };
        let change = bar.close - pc;
        self.prev_close = Some(bar.close);

        self.changes.push_back(change);
        if self.changes.len() > self.period {
            self.changes.pop_front();
        }
        if self.changes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let up_sum: Decimal = self.changes.iter().filter(|&&d| d > Decimal::ZERO).sum();
        let down_sum: Decimal = self.changes.iter().filter(|&&d| d < Decimal::ZERO).map(|d| d.abs()).sum();
        let denom = up_sum + down_sum;
        if denom == Decimal::ZERO {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        Ok(SignalValue::Scalar((up_sum - down_sum) / denom * Decimal::ONE_HUNDRED))
    }

    fn is_ready(&self) -> bool {
        self.changes.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.changes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(c: &str) -> OhlcvBar {
        let p = Price::new(c.parse().unwrap()).unwrap();
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
    fn test_cmo_period_0_error() {
        assert!(Cmo::new("cmo", 0).is_err());
    }

    #[test]
    fn test_cmo_unavailable_before_period() {
        let mut cmo = Cmo::new("cmo3", 3).unwrap();
        assert_eq!(cmo.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(cmo.update_bar(&bar("102")).unwrap(), SignalValue::Unavailable);
        assert_eq!(cmo.update_bar(&bar("104")).unwrap(), SignalValue::Unavailable);
        // 4th bar completes period=3 window
        assert!(cmo.update_bar(&bar("106")).unwrap().is_scalar());
    }

    #[test]
    fn test_cmo_all_up_returns_100() {
        let mut cmo = Cmo::new("cmo3", 3).unwrap();
        cmo.update_bar(&bar("100")).unwrap();
        cmo.update_bar(&bar("101")).unwrap();
        cmo.update_bar(&bar("102")).unwrap();
        let v = cmo.update_bar(&bar("103")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_cmo_all_down_returns_neg100() {
        let mut cmo = Cmo::new("cmo3", 3).unwrap();
        cmo.update_bar(&bar("103")).unwrap();
        cmo.update_bar(&bar("102")).unwrap();
        cmo.update_bar(&bar("101")).unwrap();
        let v = cmo.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-100)));
    }

    #[test]
    fn test_cmo_flat_returns_zero() {
        let mut cmo = Cmo::new("cmo3", 3).unwrap();
        for _ in 0..4 {
            cmo.update_bar(&bar("100")).unwrap();
        }
        let v = cmo.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(Decimal::ZERO));
    }

    #[test]
    fn test_cmo_reset() {
        let mut cmo = Cmo::new("cmo3", 3).unwrap();
        for i in 0..5 { cmo.update_bar(&bar(&format!("{}", 100 + i))).unwrap(); }
        assert!(cmo.is_ready());
        cmo.reset();
        assert!(!cmo.is_ready());
    }
}
