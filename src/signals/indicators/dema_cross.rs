//! DEMA Cross indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// DEMA Cross — detects crossovers between a fast and slow Double EMA.
///
/// ```text
/// DEMA(period) = 2 × EMA(close, period) − EMA(EMA(close, period), period)
/// output = +100 on bullish cross (fast > slow, was fast ≤ slow)
///          −100 on bearish cross (fast < slow, was fast ≥ slow)
///            0  no cross
/// ```
///
/// Returns [`SignalValue::Unavailable`] until both DEMAs are seeded.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::DemaCross;
/// use fin_primitives::signals::Signal;
///
/// let d = DemaCross::new("dc", 5, 20).unwrap();
/// assert_eq!(d.period(), 20);
/// ```
pub struct DemaCross {
    name: String,
    fast_period: usize,
    slow_period: usize,

    // Fast DEMA state
    fast_k: Decimal,
    fast_ema1: Option<Decimal>,
    fast_ema2: Option<Decimal>,
    fast_seed1: Vec<Decimal>,
    fast_seed2: Vec<Decimal>,
    fast_ema1_ready: bool,

    // Slow DEMA state
    slow_k: Decimal,
    slow_ema1: Option<Decimal>,
    slow_ema2: Option<Decimal>,
    slow_seed1: Vec<Decimal>,
    slow_seed2: Vec<Decimal>,
    slow_ema1_ready: bool,

    prev_fast: Option<Decimal>,
    prev_slow: Option<Decimal>,
}

impl DemaCross {
    /// Creates a new `DemaCross`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if either period is zero or `fast >= slow`.
    pub fn new(name: impl Into<String>, fast: usize, slow: usize) -> Result<Self, FinError> {
        if fast == 0 { return Err(FinError::InvalidPeriod(fast)); }
        if slow == 0 { return Err(FinError::InvalidPeriod(slow)); }
        if fast >= slow { return Err(FinError::InvalidPeriod(fast)); }
        let fast_k = Decimal::from(2u32) / Decimal::from((fast + 1) as u32);
        let slow_k = Decimal::from(2u32) / Decimal::from((slow + 1) as u32);
        Ok(Self {
            name: name.into(),
            fast_period: fast,
            slow_period: slow,
            fast_k,
            fast_ema1: None,
            fast_ema2: None,
            fast_seed1: Vec::with_capacity(fast),
            fast_seed2: Vec::with_capacity(fast),
            fast_ema1_ready: false,
            slow_k,
            slow_ema1: None,
            slow_ema2: None,
            slow_seed1: Vec::with_capacity(slow),
            slow_seed2: Vec::with_capacity(slow),
            slow_ema1_ready: false,
            prev_fast: None,
            prev_slow: None,
        })
    }

    fn update_dema(
        value: Decimal,
        k: Decimal,
        period: usize,
        ema1: &mut Option<Decimal>,
        ema2: &mut Option<Decimal>,
        seed1: &mut Vec<Decimal>,
        seed2: &mut Vec<Decimal>,
        ema1_ready: &mut bool,
    ) -> Option<Decimal> {
        // Stage 1: seed EMA1
        if !*ema1_ready {
            seed1.push(value);
            if seed1.len() == period {
                let sma = seed1.iter().sum::<Decimal>() / Decimal::from(period as u32);
                *ema1 = Some(sma);
                *ema1_ready = true;
            }
            return None;
        }

        // Update EMA1
        let e1 = ema1.unwrap() * (Decimal::ONE - k) + value * k;
        *ema1 = Some(e1);

        // Stage 2: seed EMA2
        if ema2.is_none() {
            seed2.push(e1);
            if seed2.len() == period {
                let sma2 = seed2.iter().sum::<Decimal>() / Decimal::from(period as u32);
                *ema2 = Some(sma2);
                return Some(Decimal::from(2u32) * e1 - sma2);
            }
            return None;
        }

        // Update EMA2
        let e2 = ema2.unwrap() * (Decimal::ONE - k) + e1 * k;
        *ema2 = Some(e2);
        Some(Decimal::from(2u32) * e1 - e2)
    }
}

impl Signal for DemaCross {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let fast_dema = Self::update_dema(
            bar.close, self.fast_k, self.fast_period,
            &mut self.fast_ema1, &mut self.fast_ema2,
            &mut self.fast_seed1, &mut self.fast_seed2,
            &mut self.fast_ema1_ready,
        );
        let slow_dema = Self::update_dema(
            bar.close, self.slow_k, self.slow_period,
            &mut self.slow_ema1, &mut self.slow_ema2,
            &mut self.slow_seed1, &mut self.slow_seed2,
            &mut self.slow_ema1_ready,
        );

        match (fast_dema, slow_dema) {
            (Some(f), Some(s)) => {
                let signal = match (self.prev_fast, self.prev_slow) {
                    (Some(pf), Some(ps)) => {
                        if f > s && pf <= ps { Decimal::from(100u32) }
                        else if f < s && pf >= ps { -Decimal::from(100u32) }
                        else { Decimal::ZERO }
                    }
                    _ => Decimal::ZERO,
                };
                self.prev_fast = Some(f);
                self.prev_slow = Some(s);
                Ok(SignalValue::Scalar(signal))
            }
            _ => {
                if let Some(f) = fast_dema { self.prev_fast = Some(f); }
                if let Some(s) = slow_dema { self.prev_slow = Some(s); }
                Ok(SignalValue::Unavailable)
            }
        }
    }

    fn is_ready(&self) -> bool { self.prev_fast.is_some() && self.prev_slow.is_some() }
    fn period(&self) -> usize { self.slow_period }

    fn reset(&mut self) {
        self.fast_ema1 = None;
        self.fast_ema2 = None;
        self.fast_seed1.clear();
        self.fast_seed2.clear();
        self.fast_ema1_ready = false;
        self.slow_ema1 = None;
        self.slow_ema2 = None;
        self.slow_seed1.clear();
        self.slow_seed2.clear();
        self.slow_ema1_ready = false;
        self.prev_fast = None;
        self.prev_slow = None;
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
    fn test_dema_cross_invalid() {
        assert!(DemaCross::new("d", 0, 20).is_err());
        assert!(DemaCross::new("d", 20, 5).is_err());
        assert!(DemaCross::new("d", 5, 5).is_err());
    }

    #[test]
    fn test_dema_cross_unavailable_before_warmup() {
        let mut d = DemaCross::new("d", 3, 5).unwrap();
        assert_eq!(d.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert!(!d.is_ready());
    }

    #[test]
    fn test_dema_cross_flat_no_cross() {
        let mut d = DemaCross::new("d", 3, 5).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..20 { last = d.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_dema_cross_bullish_cross() {
        let mut d = DemaCross::new("d", 3, 5).unwrap();
        // Warm up with flat price then spike up
        for _ in 0..20 { d.update_bar(&bar("100")).unwrap(); }
        // Feed rising prices to trigger bullish cross
        let mut found_cross = false;
        for i in 1..=30u32 {
            if let SignalValue::Scalar(v) = d.update_bar(&bar(&(100 + i).to_string())).unwrap() {
                if v == dec!(100) { found_cross = true; break; }
            }
        }
        assert!(found_cross);
    }

    #[test]
    fn test_dema_cross_reset() {
        let mut d = DemaCross::new("d", 3, 5).unwrap();
        for _ in 0..20 { d.update_bar(&bar("100")).unwrap(); }
        assert!(d.is_ready());
        d.reset();
        assert!(!d.is_ready());
    }
}
