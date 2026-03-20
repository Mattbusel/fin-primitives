//! Elder Impulse System indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Elder Impulse System — identifies impulse bars where EMA direction and
/// MACD histogram direction agree.
///
/// ```text
/// ema_t       = EMA(close, ema_period)
/// fast_ema    = EMA(close, fast)
/// slow_ema    = EMA(close, slow)
/// macd_line   = fast_ema - slow_ema
/// signal_line = EMA(macd_line, signal)
/// histogram   = macd_line - signal_line
///
/// impulse = +1  if ema_t > ema_{t-1}  AND  histogram_t > histogram_{t-1}
///           -1  if ema_t < ema_{t-1}  AND  histogram_t < histogram_{t-1}
///            0  otherwise (mixed signals)
/// ```
///
/// Returns [`SignalValue::Unavailable`] until all EMAs have warmed up.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ElderImpulse;
/// use fin_primitives::signals::Signal;
///
/// let ei = ElderImpulse::new("ei", 13, 12, 26, 9).unwrap();
/// assert_eq!(ei.period(), 26);
/// ```
pub struct ElderImpulse {
    name: String,
    ema_k: Decimal,
    fast_k: Decimal,
    slow_k: Decimal,
    sig_k: Decimal,
    // EMA state
    ema: Option<Decimal>,
    ema_seed: VecDeque<Decimal>,
    ema_period: usize,
    fast_ema: Option<Decimal>,
    fast_seed: VecDeque<Decimal>,
    fast_period: usize,
    slow_ema: Option<Decimal>,
    slow_seed: VecDeque<Decimal>,
    slow_period: usize,
    // Signal line
    sig_ema: Option<Decimal>,
    sig_seed: VecDeque<Decimal>,
    sig_period: usize,
    // Previous values for direction
    prev_ema: Option<Decimal>,
    prev_hist: Option<Decimal>,
}

impl ElderImpulse {
    /// Creates a new `ElderImpulse`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if any period is zero or `fast >= slow`.
    pub fn new(
        name: impl Into<String>,
        ema_period: usize,
        fast: usize,
        slow: usize,
        signal: usize,
    ) -> Result<Self, FinError> {
        if ema_period == 0 { return Err(FinError::InvalidPeriod(ema_period)); }
        if fast == 0       { return Err(FinError::InvalidPeriod(fast)); }
        if slow == 0       { return Err(FinError::InvalidPeriod(slow)); }
        if signal == 0     { return Err(FinError::InvalidPeriod(signal)); }
        if fast >= slow    { return Err(FinError::InvalidPeriod(fast)); }
        #[allow(clippy::cast_possible_truncation)]
        let ema_k  = Decimal::TWO / Decimal::from((ema_period + 1) as u32);
        #[allow(clippy::cast_possible_truncation)]
        let fast_k = Decimal::TWO / Decimal::from((fast + 1) as u32);
        #[allow(clippy::cast_possible_truncation)]
        let slow_k = Decimal::TWO / Decimal::from((slow + 1) as u32);
        #[allow(clippy::cast_possible_truncation)]
        let sig_k  = Decimal::TWO / Decimal::from((signal + 1) as u32);
        Ok(Self {
            name: name.into(),
            ema_k, fast_k, slow_k, sig_k,
            ema: None, ema_seed: VecDeque::with_capacity(ema_period), ema_period,
            fast_ema: None, fast_seed: VecDeque::with_capacity(fast), fast_period: fast,
            slow_ema: None, slow_seed: VecDeque::with_capacity(slow), slow_period: slow,
            sig_ema: None, sig_seed: VecDeque::with_capacity(signal), sig_period: signal,
            prev_ema: None, prev_hist: None,
        })
    }

    fn ema_step(prev: &mut Option<Decimal>, seed: &mut VecDeque<Decimal>, period: usize, k: Decimal, val: Decimal) -> Option<Decimal> {
        match *prev {
            None => {
                seed.push_back(val);
                if seed.len() >= period {
                    let sma = seed.iter().sum::<Decimal>() / Decimal::from(period as u32);
                    *prev = Some(sma);
                    Some(sma)
                } else { None }
            }
            Some(p) => {
                let v = val * k + p * (Decimal::ONE - k);
                *prev = Some(v);
                Some(v)
            }
        }
    }
}

impl Signal for ElderImpulse {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let close = bar.close;

        let ema_val = Self::ema_step(&mut self.ema, &mut self.ema_seed, self.ema_period, self.ema_k, close);
        let fast_val = Self::ema_step(&mut self.fast_ema, &mut self.fast_seed, self.fast_period, self.fast_k, close);
        let slow_val = Self::ema_step(&mut self.slow_ema, &mut self.slow_seed, self.slow_period, self.slow_k, close);

        let (ema_val, fast_val, slow_val) = match (ema_val, fast_val, slow_val) {
            (Some(e), Some(f), Some(s)) => (e, f, s),
            _ => return Ok(SignalValue::Unavailable),
        };

        let macd_line = fast_val - slow_val;
        let hist_val = match Self::ema_step(&mut self.sig_ema, &mut self.sig_seed, self.sig_period, self.sig_k, macd_line) {
            None => return Ok(SignalValue::Unavailable),
            Some(sig) => macd_line - sig,
        };

        let signal = match (self.prev_ema, self.prev_hist) {
            (Some(pe), Some(ph)) => {
                let ema_up = ema_val > pe;
                let ema_dn = ema_val < pe;
                let hist_up = hist_val > ph;
                let hist_dn = hist_val < ph;
                if ema_up && hist_up {
                    Decimal::ONE
                } else if ema_dn && hist_dn {
                    -Decimal::ONE
                } else {
                    Decimal::ZERO
                }
            }
            _ => Decimal::ZERO,
        };

        self.prev_ema = Some(ema_val);
        self.prev_hist = Some(hist_val);
        Ok(SignalValue::Scalar(signal))
    }

    fn is_ready(&self) -> bool {
        self.sig_ema.is_some()
    }

    fn period(&self) -> usize {
        self.slow_period
    }

    fn reset(&mut self) {
        self.ema = None; self.ema_seed.clear();
        self.fast_ema = None; self.fast_seed.clear();
        self.slow_ema = None; self.slow_seed.clear();
        self.sig_ema = None; self.sig_seed.clear();
        self.prev_ema = None; self.prev_hist = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};

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
    fn test_elder_impulse_invalid() {
        assert!(ElderImpulse::new("e", 0, 12, 26, 9).is_err());
        assert!(ElderImpulse::new("e", 13, 26, 12, 9).is_err()); // fast >= slow
    }

    #[test]
    fn test_elder_impulse_unavailable_before_warmup() {
        let mut ei = ElderImpulse::new("e", 3, 2, 4, 2).unwrap();
        for _ in 0..4 {
            assert_eq!(ei.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_elder_impulse_produces_scalar() {
        let mut ei = ElderImpulse::new("e", 3, 2, 4, 2).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0..30usize {
            last = ei.update_bar(&bar(&(100 + i % 5).to_string())).unwrap();
        }
        assert!(matches!(last, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_elder_impulse_reset() {
        let mut ei = ElderImpulse::new("e", 3, 2, 4, 2).unwrap();
        for i in 0..30usize { ei.update_bar(&bar(&(100 + i % 5).to_string())).unwrap(); }
        assert!(ei.is_ready());
        ei.reset();
        assert!(!ei.is_ready());
    }
}
