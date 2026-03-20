//! EMA Slope indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// EMA Slope — rate of change of an EMA expressed as percentage per bar.
///
/// ```text
/// EMA_t  = EMA(close, period)
/// output = (EMA_t − EMA_{t−1}) / EMA_{t−1} × 100
/// ```
///
/// Positive output indicates an accelerating uptrend; negative a downtrend.
/// Returns 0 when the previous EMA is zero.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen
/// (needs two consecutive EMA values to compute a slope).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::EmaSlope;
/// use fin_primitives::signals::Signal;
///
/// let es = EmaSlope::new("es", 14).unwrap();
/// assert_eq!(es.period(), 14);
/// ```
pub struct EmaSlope {
    name: String,
    period: usize,
    ema: Option<Decimal>,
    prev_ema: Option<Decimal>,
    seed: Vec<Decimal>,
}

impl EmaSlope {
    /// Creates a new `EmaSlope`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            ema: None,
            prev_ema: None,
            seed: Vec::with_capacity(period),
        })
    }
}

impl Signal for EmaSlope {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let k = Decimal::from(2u32) / Decimal::from((self.period + 1) as u32);

        if self.ema.is_none() {
            self.seed.push(bar.close);
            if self.seed.len() == self.period {
                let sma = self.seed.iter().sum::<Decimal>()
                    / Decimal::from(self.period as u32);
                self.ema = Some(sma);
            }
            return Ok(SignalValue::Unavailable);
        }

        self.prev_ema = self.ema;
        let new_ema = self.ema.unwrap() + k * (bar.close - self.ema.unwrap());
        self.ema = Some(new_ema);

        match self.prev_ema {
            None => Ok(SignalValue::Unavailable),
            Some(pe) if pe.is_zero() => Ok(SignalValue::Scalar(Decimal::ZERO)),
            Some(pe) => {
                let slope = (new_ema - pe) / pe * Decimal::from(100u32);
                Ok(SignalValue::Scalar(slope))
            }
        }
    }

    fn is_ready(&self) -> bool { self.prev_ema.is_some() }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.ema = None;
        self.prev_ema = None;
        self.seed.clear();
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
    fn test_ema_slope_invalid() {
        assert!(EmaSlope::new("e", 0).is_err());
    }

    #[test]
    fn test_ema_slope_unavailable_before_warmup() {
        let mut e = EmaSlope::new("e", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(e.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_ema_slope_flat_is_zero() {
        // Flat prices → EMA doesn't change → slope = 0
        let mut e = EmaSlope::new("e", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..10 { last = e.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_ema_slope_uptrend_positive() {
        // Rising prices → EMA rising → positive slope
        let mut e = EmaSlope::new("e", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0u32..15 {
            let p = format!("{}", 100 + i);
            last = e.update_bar(&bar(&p)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(0), "expected positive slope, got {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_ema_slope_downtrend_negative() {
        let mut e = EmaSlope::new("e", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0u32..15 {
            let p = format!("{}", 200 - i);
            last = e.update_bar(&bar(&p)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v < dec!(0), "expected negative slope, got {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_ema_slope_reset() {
        let mut e = EmaSlope::new("e", 3).unwrap();
        for _ in 0..10 { e.update_bar(&bar("100")).unwrap(); }
        assert!(e.is_ready());
        e.reset();
        assert!(!e.is_ready());
    }
}
