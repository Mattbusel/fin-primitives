//! Volume Zone Oscillator (VZO).

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Volume Zone Oscillator — classifies volume as positive or negative based on
/// price direction and computes their EMA ratio.
///
/// ```text
/// r_t = +volume  if close > prev_close
///       -volume  if close < prev_close
///       0        if close == prev_close
///
/// VZO = 100 × EMA(r, period) / EMA(|volume|, period)
/// ```
///
/// Values > 0 indicate dominant buying pressure; < 0 indicate selling pressure.
/// Common zones: above +40 = overbought, below -40 = oversold.
///
/// Returns [`SignalValue::Unavailable`] until the first close-to-close change is observed.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Vzo;
/// use fin_primitives::signals::Signal;
///
/// let v = Vzo::new("vzo", 14).unwrap();
/// assert_eq!(v.period(), 14);
/// ```
pub struct Vzo {
    name: String,
    period: usize,
    k: Decimal,
    prev_close: Option<Decimal>,
    ema_r: Option<Decimal>,
    ema_vol: Option<Decimal>,
}

impl Vzo {
    /// Creates a new `Vzo`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        #[allow(clippy::cast_possible_truncation)]
        let k = Decimal::TWO / Decimal::from((period + 1) as u32);
        Ok(Self {
            name: name.into(),
            period,
            k,
            prev_close: None,
            ema_r: None,
            ema_vol: None,
        })
    }
}

impl Signal for Vzo {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let close = bar.close;
        let vol = bar.volume;

        let prev = match self.prev_close {
            None => {
                self.prev_close = Some(close);
                return Ok(SignalValue::Unavailable);
            }
            Some(p) => p,
        };
        self.prev_close = Some(close);

        let r = if close > prev { vol } else if close < prev { -vol } else { Decimal::ZERO };

        let ema_r = match self.ema_r {
            None => { self.ema_r = Some(r); r }
            Some(prev_er) => {
                let v = r * self.k + prev_er * (Decimal::ONE - self.k);
                self.ema_r = Some(v);
                v
            }
        };

        let ema_vol = match self.ema_vol {
            None => { self.ema_vol = Some(vol); vol }
            Some(prev_ev) => {
                let v = vol * self.k + prev_ev * (Decimal::ONE - self.k);
                self.ema_vol = Some(v);
                v
            }
        };

        if ema_vol.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        Ok(SignalValue::Scalar(
            Decimal::from(100u32) * ema_r / ema_vol,
        ))
    }

    fn is_ready(&self) -> bool {
        self.ema_r.is_some()
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.ema_r = None;
        self.ema_vol = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(c: &str, vol: &str) -> OhlcvBar {
        let p = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vzo_invalid() {
        assert!(Vzo::new("v", 0).is_err());
    }

    #[test]
    fn test_vzo_first_bar_unavailable() {
        let mut v = Vzo::new("v", 14).unwrap();
        assert_eq!(v.update_bar(&bar("100", "1000")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_vzo_second_bar_produces_scalar() {
        let mut v = Vzo::new("v", 14).unwrap();
        v.update_bar(&bar("100", "1000")).unwrap();
        let s = v.update_bar(&bar("101", "1000")).unwrap();
        assert!(matches!(s, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_vzo_all_up_bars_positive() {
        let mut v = Vzo::new("v", 5).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 100..115u32 {
            last = v.update_bar(&bar(&i.to_string(), "1000")).unwrap();
        }
        if let SignalValue::Scalar(val) = last {
            assert!(val > dec!(0), "all-up bars should yield positive VZO: {val}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vzo_reset() {
        let mut v = Vzo::new("v", 5).unwrap();
        v.update_bar(&bar("100", "1000")).unwrap();
        v.update_bar(&bar("101", "1000")).unwrap();
        assert!(v.is_ready());
        v.reset();
        assert!(!v.is_ready());
        assert_eq!(v.update_bar(&bar("100", "1000")).unwrap(), SignalValue::Unavailable);
    }
}
