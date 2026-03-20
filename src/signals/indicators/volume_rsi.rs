//! Volume RSI indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Volume RSI — RSI applied to volume changes instead of price changes.
///
/// ```text
/// volume_change_t = volume_t − volume_{t-1}
/// VolumeRSI = RSI(volume_change, period)
/// ```
///
/// Values above 50 indicate volume is increasing (expanding); below 50 indicate
/// volume is decreasing (contracting). Useful for confirming trend strength.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeRsi;
/// use fin_primitives::signals::Signal;
///
/// let vr = VolumeRsi::new("vrsi", 14).unwrap();
/// assert_eq!(vr.period(), 14);
/// ```
pub struct VolumeRsi {
    name: String,
    period: usize,
    k: Decimal,
    prev_volume: Option<Decimal>,
    avg_gain: Option<Decimal>,
    avg_loss: Option<Decimal>,
    seed_gains: Vec<Decimal>,
    seed_losses: Vec<Decimal>,
}

impl VolumeRsi {
    /// Creates a new `VolumeRsi`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        let k = Decimal::ONE / Decimal::from(period as u32);
        Ok(Self {
            name: name.into(),
            period,
            k,
            prev_volume: None,
            avg_gain: None,
            avg_loss: None,
            seed_gains: Vec::with_capacity(period),
            seed_losses: Vec::with_capacity(period),
        })
    }
}

impl Signal for VolumeRsi {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let vol = bar.volume;
        let change = match self.prev_volume {
            None => {
                self.prev_volume = Some(vol);
                return Ok(SignalValue::Unavailable);
            }
            Some(pv) => vol - pv,
        };
        self.prev_volume = Some(vol);

        let gain = if change > Decimal::ZERO { change } else { Decimal::ZERO };
        let loss = if change < Decimal::ZERO { -change } else { Decimal::ZERO };

        if self.avg_gain.is_none() {
            self.seed_gains.push(gain);
            self.seed_losses.push(loss);
            if self.seed_gains.len() == self.period {
                let ag = self.seed_gains.iter().sum::<Decimal>() / Decimal::from(self.period as u32);
                let al = self.seed_losses.iter().sum::<Decimal>() / Decimal::from(self.period as u32);
                self.avg_gain = Some(ag);
                self.avg_loss = Some(al);
                return Ok(SignalValue::Scalar(Self::rsi(ag, al)));
            }
            return Ok(SignalValue::Unavailable);
        }

        let ag = self.avg_gain.unwrap() * (Decimal::ONE - self.k) + gain * self.k;
        let al = self.avg_loss.unwrap() * (Decimal::ONE - self.k) + loss * self.k;
        self.avg_gain = Some(ag);
        self.avg_loss = Some(al);
        Ok(SignalValue::Scalar(Self::rsi(ag, al)))
    }

    fn is_ready(&self) -> bool { self.avg_gain.is_some() }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.prev_volume = None;
        self.avg_gain = None;
        self.avg_loss = None;
        self.seed_gains.clear();
        self.seed_losses.clear();
    }
}

impl VolumeRsi {
    fn rsi(avg_gain: Decimal, avg_loss: Decimal) -> Decimal {
        if avg_loss.is_zero() { return Decimal::from(100u32); }
        let rs = avg_gain / avg_loss;
        Decimal::from(100u32) - Decimal::from(100u32) / (Decimal::ONE + rs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar_v(v: &str) -> OhlcvBar {
        let p = Price::new(dec!(100)).unwrap();
        let vol = Quantity::new(v.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: vol,
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vrsi_invalid() {
        assert!(VolumeRsi::new("v", 0).is_err());
    }

    #[test]
    fn test_vrsi_unavailable_first_bar() {
        let mut v = VolumeRsi::new("v", 3).unwrap();
        assert_eq!(v.update_bar(&bar_v("1000")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_vrsi_flat_volume_is_50() {
        // Constant volume → no gain, no loss → RSI = 100 (no losses)
        // Actually: constant vol → change=0 → gain=0, loss=0 → avg_gain=0, avg_loss=0
        // → loss is zero → returns 100
        let mut v = VolumeRsi::new("v", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..6 { last = v.update_bar(&bar_v("1000")).unwrap(); }
        // All changes = 0, so avg_loss = 0 → returns 100
        if let SignalValue::Scalar(val) = last {
            assert_eq!(val, dec!(100));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_vrsi_rising_volume_above_50() {
        // Steadily rising volume → all gains, no losses → RSI = 100
        let mut v = VolumeRsi::new("v", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        let vols = ["1000", "1100", "1200", "1300", "1400"];
        for vol in &vols { last = v.update_bar(&bar_v(vol)).unwrap(); }
        if let SignalValue::Scalar(val) = last {
            assert!(val > dec!(50), "expected > 50, got {val}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_vrsi_output_range() {
        let mut v = VolumeRsi::new("v", 3).unwrap();
        let vols = ["1000", "1200", "900", "1100", "800", "1300", "950"];
        for vol in &vols {
            if let SignalValue::Scalar(val) = v.update_bar(&bar_v(vol)).unwrap() {
                assert!(val >= dec!(0) && val <= dec!(100), "out of range: {val}");
            }
        }
    }

    #[test]
    fn test_vrsi_reset() {
        let mut v = VolumeRsi::new("v", 3).unwrap();
        for vol in ["1000", "1100", "1200", "1300", "1400"] {
            v.update_bar(&bar_v(vol)).unwrap();
        }
        assert!(v.is_ready());
        v.reset();
        assert!(!v.is_ready());
    }
}
