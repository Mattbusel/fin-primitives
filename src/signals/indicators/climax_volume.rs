//! Climax Volume indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Climax Volume — identifies volume climax bars where price extension AND
/// abnormal volume coincide, which often precede reversals.
///
/// Returns:
/// * `+1` when close > open AND volume ≥ `vol_mult × avg_volume(period)`
///   AND body ≥ `range_mult × avg_body(period)`
/// * `−1` when close < open under same conditions
/// * `0` otherwise
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ClimaxVolume;
/// use fin_primitives::signals::Signal;
///
/// let cv = ClimaxVolume::new("cv", 20, "2.0".parse().unwrap(), "1.5".parse().unwrap()).unwrap();
/// assert_eq!(cv.period(), 20);
/// ```
pub struct ClimaxVolume {
    name: String,
    period: usize,
    vol_mult: Decimal,
    range_mult: Decimal,
    volumes: VecDeque<Decimal>,
    bodies: VecDeque<Decimal>,
}

impl ClimaxVolume {
    /// Creates a new `ClimaxVolume`.
    ///
    /// - `vol_mult`: volume must be at least this multiple of the average.
    /// - `range_mult`: body size must be at least this multiple of the average.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(
        name: impl Into<String>,
        period: usize,
        vol_mult: Decimal,
        range_mult: Decimal,
    ) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            vol_mult,
            range_mult,
            volumes: VecDeque::with_capacity(period),
            bodies: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for ClimaxVolume {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let vol = bar.volume;
        let body = bar.body_size();

        self.volumes.push_back(vol);
        self.bodies.push_back(body);
        if self.volumes.len() > self.period {
            self.volumes.pop_front();
            self.bodies.pop_front();
        }
        if self.volumes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let avg_vol = self.volumes.iter().sum::<Decimal>() / Decimal::from(self.period as u32);
        let avg_body = self.bodies.iter().sum::<Decimal>() / Decimal::from(self.period as u32);

        let vol_climax = vol >= avg_vol * self.vol_mult;
        let range_climax = avg_body.is_zero() || body >= avg_body * self.range_mult;

        let value = if vol_climax && range_climax {
            if bar.is_bullish() {
                Decimal::ONE
            } else if bar.is_bearish() {
                Decimal::NEGATIVE_ONE
            } else {
                Decimal::ZERO
            }
        } else {
            Decimal::ZERO
        };

        Ok(SignalValue::Scalar(value))
    }

    fn is_ready(&self) -> bool {
        self.volumes.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.volumes.clear();
        self.bodies.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, c: &str, vol: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        let hp = if cp >= op { cp } else { op };
        let lp = if cp <= op { cp } else { op };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hp, low: lp, close: cp,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_climax_invalid_period() {
        assert!(ClimaxVolume::new("c", 0, dec!(2), dec!(1.5)).is_err());
    }

    #[test]
    fn test_climax_unavailable_before_period() {
        let mut cv = ClimaxVolume::new("c", 3, dec!(2), dec!(1.5)).unwrap();
        assert_eq!(cv.update_bar(&bar("100", "105", "1000")).unwrap(), SignalValue::Unavailable);
        assert_eq!(cv.update_bar(&bar("100", "105", "1000")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_climax_normal_bar_zero() {
        let mut cv = ClimaxVolume::new("c", 3, dec!(2), dec!(1.5)).unwrap();
        // All bars same volume — none qualifies as climax
        for _ in 0..3 { cv.update_bar(&bar("100", "105", "100")).unwrap(); }
        if let SignalValue::Scalar(v) = cv.update_bar(&bar("100", "105", "100")).unwrap() {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_climax_bullish_climax() {
        // Base bars with low volume and small body, then a spike
        let mut cv = ClimaxVolume::new("c", 3, dec!(2), dec!(1)).unwrap();
        cv.update_bar(&bar("100", "101", "100")).unwrap();
        cv.update_bar(&bar("100", "101", "100")).unwrap();
        cv.update_bar(&bar("100", "101", "100")).unwrap();
        // Spike bar is in its own window: avg=(100+100+500)/3=233, need vol>=2*233=467; 500>=467 ✓
        // body avg=(1+1+10)/3=4, range_mult=1 => need body>=4; body=10 ✓
        if let SignalValue::Scalar(v) = cv.update_bar(&bar("100", "110", "500")).unwrap() {
            assert_eq!(v, dec!(1), "bullish climax should be +1: {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_climax_reset() {
        let mut cv = ClimaxVolume::new("c", 3, dec!(2), dec!(1.5)).unwrap();
        for _ in 0..3 { cv.update_bar(&bar("100", "105", "100")).unwrap(); }
        assert!(cv.is_ready());
        cv.reset();
        assert!(!cv.is_ready());
    }
}
