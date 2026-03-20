//! Volume-Weighted RSI indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Volume-Weighted RSI — RSI where each bar's gain/loss is weighted by volume.
///
/// ```text
/// change_t = close_t − close_{t−1}
/// gain_t   = max(change_t, 0) × volume_t
/// loss_t   = max(−change_t, 0) × volume_t
///
/// avg_gain = Wilder_smooth(gain, period)
/// avg_loss = Wilder_smooth(loss, period)
/// VW_RSI   = 100 − 100 / (1 + avg_gain / avg_loss)
/// ```
///
/// High-volume moves receive more weight, making this more sensitive to
/// volume-confirmed price action than standard RSI.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeWeightedRsi;
/// use fin_primitives::signals::Signal;
///
/// let vwr = VolumeWeightedRsi::new("vwr", 14).unwrap();
/// assert_eq!(vwr.period(), 14);
/// ```
pub struct VolumeWeightedRsi {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    avg_gain: Option<Decimal>,
    avg_loss: Option<Decimal>,
    seed_gains: Vec<Decimal>,
    seed_losses: Vec<Decimal>,
}

impl VolumeWeightedRsi {
    /// Creates a new `VolumeWeightedRsi`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 { return Err(FinError::InvalidPeriod(period)); }
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

impl Signal for VolumeWeightedRsi {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let prev = match self.prev_close {
            None => {
                self.prev_close = Some(bar.close);
                return Ok(SignalValue::Unavailable);
            }
            Some(p) => p,
        };
        self.prev_close = Some(bar.close);

        let change = bar.close - prev;
        let gain = if change > Decimal::ZERO { change * bar.volume } else { Decimal::ZERO };
        let loss = if change < Decimal::ZERO { (-change) * bar.volume } else { Decimal::ZERO };

        if self.avg_gain.is_none() {
            self.seed_gains.push(gain);
            self.seed_losses.push(loss);
            if self.seed_gains.len() == self.period {
                let ag = self.seed_gains.iter().sum::<Decimal>()
                    / Decimal::from(self.period as u32);
                let al = self.seed_losses.iter().sum::<Decimal>()
                    / Decimal::from(self.period as u32);
                self.avg_gain = Some(ag);
                self.avg_loss = Some(al);
            }
            return Ok(SignalValue::Unavailable);
        }

        let k = Decimal::ONE / Decimal::from(self.period as u32);
        let ag = self.avg_gain.unwrap() * (Decimal::ONE - k) + gain * k;
        let al = self.avg_loss.unwrap() * (Decimal::ONE - k) + loss * k;
        self.avg_gain = Some(ag);
        self.avg_loss = Some(al);

        if al.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::from(100u32)));
        }
        let rs = ag / al;
        let rsi = Decimal::from(100u32) - Decimal::from(100u32) / (Decimal::ONE + rs);
        Ok(SignalValue::Scalar(rsi))
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
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar_cv(c: &str, v: &str) -> OhlcvBar {
        let cp = Price::new(c.parse().unwrap()).unwrap();
        let vol = Quantity::new(v.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cp, high: cp, low: cp, close: cp,
            volume: vol,
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    fn bar(c: &str) -> OhlcvBar { bar_cv(c, "1000") }

    #[test]
    fn test_vwrsi_invalid() {
        assert!(VolumeWeightedRsi::new("v", 0).is_err());
        assert!(VolumeWeightedRsi::new("v", 1).is_err());
    }

    #[test]
    fn test_vwrsi_unavailable_before_warmup() {
        let mut v = VolumeWeightedRsi::new("v", 5).unwrap();
        assert_eq!(v.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_vwrsi_all_up_is_100() {
        let mut v = VolumeWeightedRsi::new("v", 5).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0u32..15 {
            let p = format!("{}", 100 + i);
            last = v.update_bar(&bar(&p)).unwrap();
        }
        if let SignalValue::Scalar(val) = last {
            assert_eq!(val, dec!(100));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_vwrsi_range_0_to_100() {
        let mut v = VolumeWeightedRsi::new("v", 5).unwrap();
        for (c, vol) in [("100","1000"),("105","2000"),("102","500"),("108","1500"),
                          ("103","800"),("110","3000"),("106","600"),("112","2500")] {
            if let SignalValue::Scalar(val) = v.update_bar(&bar_cv(c, vol)).unwrap() {
                assert!(val >= dec!(0) && val <= dec!(100), "out of range: {val}");
            }
        }
    }

    #[test]
    fn test_vwrsi_reset() {
        let mut v = VolumeWeightedRsi::new("v", 5).unwrap();
        for i in 0u32..15 {
            let p = format!("{}", 100 + i);
            v.update_bar(&bar(&p)).unwrap();
        }
        assert!(v.is_ready());
        v.reset();
        assert!(!v.is_ready());
    }
}
