//! Relative Momentum Index (RMI) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Relative Momentum Index — RSI variant using a multi-bar momentum look-back.
///
/// ```text
/// up   = max(close[i] - close[i - momentum_period], 0)
/// down = max(close[i - momentum_period] - close[i], 0)
/// avg_up / avg_down: Wilder smoothing over `period` bars
/// RMI = 100 - 100 / (1 + avg_up / avg_down)
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `momentum_period + period` bars have accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Rmi;
/// use fin_primitives::signals::Signal;
///
/// let rmi = Rmi::new("rmi", 14, 4).unwrap();
/// assert_eq!(rmi.period(), 14);
/// ```
pub struct Rmi {
    name: String,
    period: usize,
    momentum_period: usize,
    close_window: VecDeque<Decimal>,
    avg_up: Option<Decimal>,
    avg_down: Option<Decimal>,
    seed_up: Decimal,
    seed_down: Decimal,
    seed_count: usize,
}

impl Rmi {
    /// Constructs a new `Rmi`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0` or `momentum_period == 0`.
    pub fn new(
        name: impl Into<String>,
        period: usize,
        momentum_period: usize,
    ) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        if momentum_period == 0 {
            return Err(FinError::InvalidPeriod(momentum_period));
        }
        Ok(Self {
            name: name.into(),
            period,
            momentum_period,
            close_window: VecDeque::with_capacity(momentum_period + 1),
            avg_up: None,
            avg_down: None,
            seed_up: Decimal::ZERO,
            seed_down: Decimal::ZERO,
            seed_count: 0,
        })
    }
}

impl Signal for Rmi {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let close = bar.close;
        self.close_window.push_back(close);
        if self.close_window.len() > self.momentum_period + 1 {
            self.close_window.pop_front();
        }

        // Need momentum_period + 1 closes to compute a momentum diff.
        if self.close_window.len() <= self.momentum_period {
            return Ok(SignalValue::Unavailable);
        }

        let prior = self.close_window[0];
        let diff  = close - prior;
        let up    = if diff > Decimal::ZERO { diff } else { Decimal::ZERO };
        let down  = if diff < Decimal::ZERO { -diff } else { Decimal::ZERO };

        if self.avg_up.is_none() {
            // Seed phase: accumulate `period` momentum values.
            self.seed_up   += up;
            self.seed_down += down;
            self.seed_count += 1;

            if self.seed_count < self.period {
                return Ok(SignalValue::Unavailable);
            }

            #[allow(clippy::cast_possible_truncation)]
            let p = Decimal::from(self.period as u32);
            let au = self.seed_up   / p;
            let ad = self.seed_down / p;
            self.avg_up   = Some(au);
            self.avg_down = Some(ad);

            if ad.is_zero() {
                return Ok(SignalValue::Scalar(Decimal::ONE_HUNDRED));
            }
            let rs = au / ad;
            return Ok(SignalValue::Scalar(
                Decimal::ONE_HUNDRED - Decimal::ONE_HUNDRED / (Decimal::ONE + rs),
            ));
        }

        // Wilder smoothing phase.
        #[allow(clippy::cast_possible_truncation)]
        let p   = Decimal::from(self.period as u32);
        let pm1 = p - Decimal::ONE;
        let au  = (self.avg_up.unwrap()   * pm1 + up)   / p;
        let ad  = (self.avg_down.unwrap() * pm1 + down) / p;
        self.avg_up   = Some(au);
        self.avg_down = Some(ad);

        if ad.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ONE_HUNDRED));
        }
        let rs = au / ad;
        Ok(SignalValue::Scalar(
            Decimal::ONE_HUNDRED - Decimal::ONE_HUNDRED / (Decimal::ONE + rs),
        ))
    }

    fn is_ready(&self) -> bool {
        self.avg_up.is_some()
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.close_window.clear();
        self.avg_up    = None;
        self.avg_down  = None;
        self.seed_up   = Decimal::ZERO;
        self.seed_down = Decimal::ZERO;
        self.seed_count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
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
    fn test_rmi_invalid_period() {
        assert!(Rmi::new("r", 0, 4).is_err());
        assert!(Rmi::new("r", 14, 0).is_err());
    }

    #[test]
    fn test_rmi_unavailable_initially() {
        let mut rmi = Rmi::new("r", 3, 2).unwrap();
        // Need 2+1=3 closes for first momentum, then 3 more for seed → 5 bars for first value.
        for _ in 0..4 {
            assert_eq!(rmi.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_rmi_all_up_equals_100() {
        let mut rmi = Rmi::new("r", 3, 1).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 1u32..=10 {
            last = rmi.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_rmi_all_down_equals_0() {
        let mut rmi = Rmi::new("r", 3, 1).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0u32..=10 {
            last = rmi.update_bar(&bar(&(110 - i).to_string())).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rmi_reset() {
        let mut rmi = Rmi::new("r", 3, 1).unwrap();
        for i in 1u32..=10 {
            rmi.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        assert!(rmi.is_ready());
        rmi.reset();
        assert!(!rmi.is_ready());
    }

    #[test]
    fn test_rmi_in_bounds() {
        let mut rmi = Rmi::new("r", 5, 2).unwrap();
        let prices = ["100", "102", "101", "103", "102", "104", "103", "105", "101", "106", "102", "107"];
        for &c in &prices {
            if let SignalValue::Scalar(v) = rmi.update_bar(&bar(c)).unwrap() {
                assert!(v >= dec!(0) && v <= dec!(100), "RMI out of bounds: {v}");
            }
        }
    }
}
