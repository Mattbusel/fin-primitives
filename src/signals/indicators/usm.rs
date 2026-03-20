//! Ultimate Smoothed Momentum (USM) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Ultimate Smoothed Momentum — combines three EMA-smoothed momentum streams
/// (short, medium, long) into a single oscillator.
///
/// ```text
/// momentum_t  = close_t - close_{t-1}
/// ema_s       = EMA(momentum, short)
/// ema_m       = EMA(momentum, medium)
/// ema_l       = EMA(momentum, long)
/// USM         = (ema_s + ema_m + ema_l) / 3
/// ```
///
/// Positive values indicate net buying momentum; negative indicate selling.
/// The multi-period structure dampens noise while preserving trend information.
///
/// Returns [`SignalValue::Unavailable`] until the second bar is seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Usm;
/// use fin_primitives::signals::Signal;
///
/// let usm = Usm::new("usm", 5, 13, 34).unwrap();
/// assert_eq!(usm.period(), 34);
/// ```
pub struct Usm {
    name: String,
    short: usize,
    medium: usize,
    long: usize,
    k_s: Decimal,
    k_m: Decimal,
    k_l: Decimal,
    prev_close: Option<Decimal>,
    ema_s: Option<Decimal>,
    ema_m: Option<Decimal>,
    ema_l: Option<Decimal>,
    // seed buffers
    seed_s: VecDeque<Decimal>,
    seed_m: VecDeque<Decimal>,
    seed_l: VecDeque<Decimal>,
}

impl Usm {
    /// Creates a new `Usm`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if any period is zero or if `short >= medium` or `medium >= long`.
    pub fn new(name: impl Into<String>, short: usize, medium: usize, long: usize) -> Result<Self, FinError> {
        if short == 0  { return Err(FinError::InvalidPeriod(short)); }
        if medium == 0 { return Err(FinError::InvalidPeriod(medium)); }
        if long == 0   { return Err(FinError::InvalidPeriod(long)); }
        if short >= medium { return Err(FinError::InvalidPeriod(short)); }
        if medium >= long  { return Err(FinError::InvalidPeriod(medium)); }
        #[allow(clippy::cast_possible_truncation)]
        let k_s = Decimal::TWO / Decimal::from((short + 1) as u32);
        #[allow(clippy::cast_possible_truncation)]
        let k_m = Decimal::TWO / Decimal::from((medium + 1) as u32);
        #[allow(clippy::cast_possible_truncation)]
        let k_l = Decimal::TWO / Decimal::from((long + 1) as u32);
        Ok(Self {
            name: name.into(),
            short,
            medium,
            long,
            k_s,
            k_m,
            k_l,
            prev_close: None,
            ema_s: None,
            ema_m: None,
            ema_l: None,
            seed_s: VecDeque::with_capacity(short),
            seed_m: VecDeque::with_capacity(medium),
            seed_l: VecDeque::with_capacity(long),
        })
    }

    fn ema_update(prev: &mut Option<Decimal>, seed: &mut VecDeque<Decimal>, period: usize, k: Decimal, val: Decimal) -> Option<Decimal> {
        match *prev {
            None => {
                seed.push_back(val);
                if seed.len() >= period {
                    let sma = seed.iter().sum::<Decimal>() / Decimal::from(period as u32);
                    *prev = Some(sma);
                    Some(sma)
                } else {
                    None
                }
            }
            Some(p) => {
                let v = val * k + p * (Decimal::ONE - k);
                *prev = Some(v);
                Some(v)
            }
        }
    }
}

impl Signal for Usm {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let close = bar.close;

        let prev = match self.prev_close {
            None => {
                self.prev_close = Some(close);
                return Ok(SignalValue::Unavailable);
            }
            Some(p) => p,
        };
        self.prev_close = Some(close);
        let mom = close - prev;

        let es = Self::ema_update(&mut self.ema_s, &mut self.seed_s, self.short,  self.k_s, mom);
        let em = Self::ema_update(&mut self.ema_m, &mut self.seed_m, self.medium, self.k_m, mom);
        let el = Self::ema_update(&mut self.ema_l, &mut self.seed_l, self.long,   self.k_l, mom);

        match (es, em, el) {
            (Some(s), Some(m), Some(l)) => Ok(SignalValue::Scalar((s + m + l) / Decimal::from(3u32))),
            _ => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool {
        self.ema_l.is_some()
    }

    fn period(&self) -> usize {
        self.long
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.ema_s = None;
        self.ema_m = None;
        self.ema_l = None;
        self.seed_s.clear();
        self.seed_m.clear();
        self.seed_l.clear();
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
    fn test_usm_invalid() {
        assert!(Usm::new("u", 0, 13, 34).is_err());
        assert!(Usm::new("u", 13, 5, 34).is_err()); // short >= medium
        assert!(Usm::new("u", 5, 34, 13).is_err()); // medium >= long
    }

    #[test]
    fn test_usm_unavailable_before_warmup() {
        let mut usm = Usm::new("u", 2, 3, 5).unwrap();
        for _ in 0..5 {
            assert_eq!(usm.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_usm_produces_scalar() {
        let mut usm = Usm::new("u", 2, 3, 5).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0..20usize {
            last = usm.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        assert!(matches!(last, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_usm_uptrend_positive() {
        let mut usm = Usm::new("u", 2, 3, 5).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0..30usize {
            last = usm.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(0), "uptrend should yield positive USM: {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_usm_reset() {
        let mut usm = Usm::new("u", 2, 3, 5).unwrap();
        for i in 0..20usize { usm.update_bar(&bar(&(100 + i).to_string())).unwrap(); }
        assert!(usm.is_ready());
        usm.reset();
        assert!(!usm.is_ready());
    }
}
