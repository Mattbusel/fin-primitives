//! RSX — Rapid Smoothed RSI indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// RSX — Rapid Smoothed RSI (Jurik-inspired smoothing applied to RSI logic).
///
/// Uses a faster Wilder-style accumulator:
/// ```text
/// avg_gain = (prev_avg_gain × (period-1) + gain) / period
/// avg_loss = (prev_avg_loss × (period-1) + loss) / period
/// RSX = 100 - 100 / (1 + avg_gain/avg_loss)
/// ```
///
/// Seeds with the first `period` bars to compute initial averages.
/// Returns a value in `[0, 100]`.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Rsx;
/// use fin_primitives::signals::Signal;
///
/// let r = Rsx::new("rsx14", 14).unwrap();
/// assert_eq!(r.period(), 14);
/// assert!(!r.is_ready());
/// ```
pub struct Rsx {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    avg_gain: Option<Decimal>,
    avg_loss: Option<Decimal>,
    seed_gains: Vec<Decimal>,
    seed_losses: Vec<Decimal>,
}

impl Rsx {
    /// Constructs a new `Rsx`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            avg_gain: None,
            avg_loss: None,
            seed_gains: Vec::with_capacity(period),
            seed_losses: Vec::with_capacity(period),
        })
    }
}

impl Signal for Rsx {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let Some(prev) = self.prev_close else {
            self.prev_close = Some(bar.close);
            return Ok(SignalValue::Unavailable);
        };
        let change = bar.close - prev;
        self.prev_close = Some(bar.close);
        let gain = if change > Decimal::ZERO { change } else { Decimal::ZERO };
        let loss = if change < Decimal::ZERO { -change } else { Decimal::ZERO };

        if self.avg_gain.is_none() {
            self.seed_gains.push(gain);
            self.seed_losses.push(loss);
            if self.seed_gains.len() < self.period {
                return Ok(SignalValue::Unavailable);
            }
            #[allow(clippy::cast_possible_truncation)]
            let n = Decimal::from(self.period as u32);
            let ag = self.seed_gains.iter().copied().sum::<Decimal>() / n;
            let al = self.seed_losses.iter().copied().sum::<Decimal>() / n;
            self.avg_gain = Some(ag);
            self.avg_loss = Some(al);
        } else {
            #[allow(clippy::cast_possible_truncation)]
            let n = Decimal::from(self.period as u32);
            let prev_ag = self.avg_gain.unwrap();
            let prev_al = self.avg_loss.unwrap();
            self.avg_gain = Some((prev_ag * (n - Decimal::ONE) + gain) / n);
            self.avg_loss = Some((prev_al * (n - Decimal::ONE) + loss) / n);
        }

        let ag = self.avg_gain.unwrap();
        let al = self.avg_loss.unwrap();
        let rsx = if al.is_zero() {
            if ag.is_zero() { Decimal::from(50u32) } else { Decimal::ONE_HUNDRED }
        } else {
            let rs = ag / al;
            Decimal::ONE_HUNDRED - Decimal::ONE_HUNDRED / (Decimal::ONE + rs)
        };
        Ok(SignalValue::Scalar(rsx))
    }

    fn is_ready(&self) -> bool { self.avg_gain.is_some() }

    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.prev_close = None;
        self.avg_gain = None;
        self.avg_loss = None;
        self.seed_gains.clear();
        self.seed_losses.clear();
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
    fn test_rsx_invalid_period() {
        assert!(Rsx::new("r", 0).is_err());
    }

    #[test]
    fn test_rsx_unavailable_before_warmup() {
        let mut r = Rsx::new("r", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(r.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_rsx_always_rising_near_100() {
        let mut r = Rsx::new("r", 5).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0..20 {
            let p = format!("{}", 100 + i);
            last = r.update_bar(&bar(&p)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(90), "RSX should be high on rising prices: {v}");
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_rsx_always_falling_near_0() {
        let mut r = Rsx::new("r", 5).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0..20 {
            let p = format!("{}", 200 - i);
            last = r.update_bar(&bar(&p)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v < dec!(10), "RSX should be low on falling prices: {v}");
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_rsx_range() {
        let mut r = Rsx::new("r", 5).unwrap();
        for i in 0..20 {
            if let SignalValue::Scalar(v) = r.update_bar(&bar(&format!("{}", 100 + (i % 5)))).unwrap() {
                assert!(v >= dec!(0) && v <= dec!(100), "RSX out of range: {v}");
            }
        }
    }

    #[test]
    fn test_rsx_reset() {
        let mut r = Rsx::new("r", 3).unwrap();
        for _ in 0..10 { r.update_bar(&bar("100")).unwrap(); }
        assert!(r.is_ready());
        r.reset();
        assert!(!r.is_ready());
    }
}
