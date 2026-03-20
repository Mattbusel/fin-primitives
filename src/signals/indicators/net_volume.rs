//! Net Volume indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Net Volume — rolling sum of signed volume (up-bars add, down-bars subtract).
///
/// ```text
/// signed_vol_t = +volume_t  if close_t > open_t   (up bar)
///              = −volume_t  if close_t < open_t   (down bar)
///              =  0         if close_t == open_t  (doji)
///
/// output = sum(signed_vol, period)
/// ```
///
/// Positive output indicates net buying over the window; negative net selling.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::NetVolume;
/// use fin_primitives::signals::Signal;
///
/// let nv = NetVolume::new("nv", 10).unwrap();
/// assert_eq!(nv.period(), 10);
/// ```
pub struct NetVolume {
    name: String,
    period: usize,
    signed_vols: VecDeque<Decimal>,
}

impl NetVolume {
    /// Creates a new `NetVolume`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            signed_vols: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for NetVolume {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let signed = if bar.is_bullish() {
            bar.volume
        } else if bar.is_bearish() {
            -bar.volume
        } else {
            Decimal::ZERO
        };

        self.signed_vols.push_back(signed);
        if self.signed_vols.len() > self.period { self.signed_vols.pop_front(); }
        if self.signed_vols.len() < self.period { return Ok(SignalValue::Unavailable); }

        let net = self.signed_vols.iter().sum::<Decimal>();
        Ok(SignalValue::Scalar(net))
    }

    fn is_ready(&self) -> bool { self.signed_vols.len() >= self.period }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.signed_vols.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar_oc_v(o: &str, c: &str, v: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        let vol = Quantity::new(v.parse().unwrap()).unwrap();
        let hp = if cp >= op { cp } else { op };
        let lp = if cp <= op { cp } else { op };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hp, low: lp, close: cp,
            volume: vol,
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_nv_invalid() {
        assert!(NetVolume::new("n", 0).is_err());
    }

    #[test]
    fn test_nv_unavailable_before_warmup() {
        let mut n = NetVolume::new("n", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(n.update_bar(&bar_oc_v("100", "101", "1000")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_nv_all_up_bars_positive() {
        let mut n = NetVolume::new("n", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = n.update_bar(&bar_oc_v("100", "101", "1000")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(3000));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_nv_all_down_bars_negative() {
        let mut n = NetVolume::new("n", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = n.update_bar(&bar_oc_v("101", "100", "1000")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(-3000));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_nv_alternating_cancels() {
        // Alternating up/down with equal volume: net = 0 (for even period)
        let mut n = NetVolume::new("n", 4).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            n.update_bar(&bar_oc_v("100", "101", "1000")).unwrap();
            last = n.update_bar(&bar_oc_v("101", "100", "1000")).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_nv_reset() {
        let mut n = NetVolume::new("n", 3).unwrap();
        for _ in 0..5 { n.update_bar(&bar_oc_v("100", "101", "1000")).unwrap(); }
        assert!(n.is_ready());
        n.reset();
        assert!(!n.is_ready());
    }
}
