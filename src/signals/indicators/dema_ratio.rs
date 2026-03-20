//! DEMA Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// DEMA Ratio — ratio of a fast DEMA to a slow DEMA.
///
/// ```text
/// DEMA(n) = 2 × EMA(n) − EMA(EMA(n))
/// output  = fast_DEMA / slow_DEMA
/// ```
///
/// Values > 1 indicate uptrend; < 1 downtrend; exactly 1 at crossover.
///
/// Returns [`SignalValue::Unavailable`] until both DEMAs are warm.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::DemaRatio;
/// use fin_primitives::signals::Signal;
///
/// let dr = DemaRatio::new("dr", 5, 20).unwrap();
/// assert_eq!(dr.period(), 20);
/// ```
pub struct DemaRatio {
    name: String,
    fast: usize,
    slow: usize,
    // fast DEMA state
    fast_ema1: Option<Decimal>,
    fast_ema2: Option<Decimal>,
    fast_seed: Vec<Decimal>,
    fast_seed2: Vec<Decimal>,
    fast_ema1_ready: bool,
    // slow DEMA state
    slow_ema1: Option<Decimal>,
    slow_ema2: Option<Decimal>,
    slow_seed: Vec<Decimal>,
    slow_seed2: Vec<Decimal>,
    slow_ema1_ready: bool,
}

impl DemaRatio {
    /// Creates a new `DemaRatio`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `fast == 0`.
    /// Returns [`FinError::InvalidInput`] if `fast >= slow`.
    pub fn new(name: impl Into<String>, fast: usize, slow: usize) -> Result<Self, FinError> {
        if fast == 0 { return Err(FinError::InvalidPeriod(fast)); }
        if fast >= slow {
            return Err(FinError::InvalidInput("fast must be less than slow".into()));
        }
        Ok(Self {
            name: name.into(),
            fast,
            slow,
            fast_ema1: None,
            fast_ema2: None,
            fast_seed: Vec::with_capacity(fast),
            fast_seed2: Vec::with_capacity(fast),
            fast_ema1_ready: false,
            slow_ema1: None,
            slow_ema2: None,
            slow_seed: Vec::with_capacity(slow),
            slow_seed2: Vec::with_capacity(slow),
            slow_ema1_ready: false,
        })
    }

    fn ema_update(
        seed: &mut Vec<Decimal>,
        seed2: &mut Vec<Decimal>,
        ema1: &mut Option<Decimal>,
        ema2: &mut Option<Decimal>,
        ema1_ready: &mut bool,
        period: usize,
        value: Decimal,
    ) -> Option<Decimal> {
        let k = Decimal::from(2u32) / Decimal::from((period + 1) as u32);

        if !*ema1_ready {
            seed.push(value);
            if seed.len() == period {
                let sma: Decimal = seed.iter().sum::<Decimal>() / Decimal::from(period as u32);
                *ema1 = Some(sma);
                *ema1_ready = true;
            }
            return None;
        }

        let e1 = ema1.unwrap() + k * (value - ema1.unwrap());
        *ema1 = Some(e1);

        if ema2.is_none() {
            seed2.push(e1);
            if seed2.len() == period {
                let sma2: Decimal = seed2.iter().sum::<Decimal>() / Decimal::from(period as u32);
                *ema2 = Some(sma2);
            }
            return None;
        }

        let e2 = ema2.unwrap() + k * (e1 - ema2.unwrap());
        *ema2 = Some(e2);
        Some(Decimal::from(2u32) * e1 - e2)
    }
}

impl Signal for DemaRatio {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let fast_dema = Self::ema_update(
            &mut self.fast_seed, &mut self.fast_seed2,
            &mut self.fast_ema1, &mut self.fast_ema2,
            &mut self.fast_ema1_ready,
            self.fast, bar.close,
        );
        let slow_dema = Self::ema_update(
            &mut self.slow_seed, &mut self.slow_seed2,
            &mut self.slow_ema1, &mut self.slow_ema2,
            &mut self.slow_ema1_ready,
            self.slow, bar.close,
        );

        match (fast_dema, slow_dema) {
            (Some(f), Some(s)) if !s.is_zero() => Ok(SignalValue::Scalar(f / s)),
            (Some(_), Some(_)) => Ok(SignalValue::Scalar(Decimal::ONE)),
            _ => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool { self.fast_ema2.is_some() && self.slow_ema2.is_some() }
    fn period(&self) -> usize { self.slow }

    fn reset(&mut self) {
        self.fast_ema1 = None;
        self.fast_ema2 = None;
        self.fast_seed.clear();
        self.fast_seed2.clear();
        self.fast_ema1_ready = false;
        self.slow_ema1 = None;
        self.slow_ema2 = None;
        self.slow_seed.clear();
        self.slow_seed2.clear();
        self.slow_ema1_ready = false;
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
    fn test_dr_invalid() {
        assert!(DemaRatio::new("d", 0, 20).is_err());
        assert!(DemaRatio::new("d", 20, 10).is_err());
        assert!(DemaRatio::new("d", 10, 10).is_err());
    }

    #[test]
    fn test_dr_unavailable_before_warmup() {
        let mut d = DemaRatio::new("d", 3, 5).unwrap();
        for _ in 0..4 {
            assert_eq!(d.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_dr_flat_is_one() {
        // Flat price: fast_DEMA = slow_DEMA = price → ratio = 1
        let mut d = DemaRatio::new("d", 3, 5).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..20 { last = d.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            // Allow small floating point epsilon
            let diff = (v - dec!(1)).abs();
            assert!(diff < dec!(0.0001), "expected ~1, got {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_dr_step_up_above_one() {
        // Flat at 100, then 3 bars at 200: fast DEMA reacts faster than slow → ratio > 1
        let mut d = DemaRatio::new("d", 3, 5).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..20 { d.update_bar(&bar("100")).unwrap(); }
        for _ in 0..3 { last = d.update_bar(&bar("200")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(1), "expected ratio > 1 right after step-up, got {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_dr_reset() {
        let mut d = DemaRatio::new("d", 3, 5).unwrap();
        for _ in 0..20 { d.update_bar(&bar("100")).unwrap(); }
        assert!(d.is_ready());
        d.reset();
        assert!(!d.is_ready());
    }
}
