//! Trend Following Filter indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Trend Following Filter — dual-EMA trend classifier.
///
/// ```text
/// fast_ema = EMA(close, fast)
/// slow_ema = EMA(close, slow)
///
/// output = +1  if fast > slow  (uptrend)
///          −1  if fast < slow  (downtrend)
///           0  if fast == slow (neutral)
/// ```
///
/// Also provides the trend strength as `(fast - slow) / slow × 100`.
/// Use `trend_strength()` for the magnitude; `update()` for the direction.
///
/// Returns [`SignalValue::Unavailable`] until both EMAs are seeded.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::TrendFollowingFilter;
/// use fin_primitives::signals::Signal;
///
/// let tf = TrendFollowingFilter::new("tff", 5, 20).unwrap();
/// assert_eq!(tf.period(), 20);
/// ```
pub struct TrendFollowingFilter {
    name: String,
    fast_period: usize,
    slow_period: usize,
    fast_k: Decimal,
    slow_k: Decimal,
    fast_ema: Option<Decimal>,
    slow_ema: Option<Decimal>,
    fast_seed: Vec<Decimal>,
    slow_seed: Vec<Decimal>,
    trend_strength: Option<Decimal>,
}

impl TrendFollowingFilter {
    /// Creates a new `TrendFollowingFilter`.
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
            slow_k,
            fast_ema: None,
            slow_ema: None,
            fast_seed: Vec::with_capacity(fast),
            slow_seed: Vec::with_capacity(slow),
            trend_strength: None,
        })
    }

    /// Returns `(fast_ema - slow_ema) / slow_ema × 100` — the trend spread as %.
    pub fn trend_strength(&self) -> Option<Decimal> { self.trend_strength }

    fn ema_update(
        value: Decimal,
        k: Decimal,
        period: usize,
        ema: &mut Option<Decimal>,
        seed: &mut Vec<Decimal>,
    ) -> Option<Decimal> {
        if ema.is_none() {
            seed.push(value);
            if seed.len() == period {
                let sma = seed.iter().sum::<Decimal>() / Decimal::from(period as u32);
                *ema = Some(sma);
                return Some(sma);
            }
            return None;
        }
        let e = ema.unwrap() * (Decimal::ONE - k) + value * k;
        *ema = Some(e);
        Some(e)
    }
}

impl Signal for TrendFollowingFilter {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let fast = Self::ema_update(bar.close, self.fast_k, self.fast_period, &mut self.fast_ema, &mut self.fast_seed);
        let slow = Self::ema_update(bar.close, self.slow_k, self.slow_period, &mut self.slow_ema, &mut self.slow_seed);

        match (fast, slow) {
            (Some(f), Some(s)) => {
                if !s.is_zero() {
                    self.trend_strength = Some((f - s) / s * Decimal::from(100u32));
                }
                let dir = if f > s { Decimal::ONE }
                    else if f < s { -Decimal::ONE }
                    else { Decimal::ZERO };
                Ok(SignalValue::Scalar(dir))
            }
            _ => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool { self.slow_ema.is_some() }
    fn period(&self) -> usize { self.slow_period }

    fn reset(&mut self) {
        self.fast_ema = None;
        self.slow_ema = None;
        self.fast_seed.clear();
        self.slow_seed.clear();
        self.trend_strength = None;
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
    fn test_tff_invalid() {
        assert!(TrendFollowingFilter::new("t", 0, 20).is_err());
        assert!(TrendFollowingFilter::new("t", 20, 5).is_err());
        assert!(TrendFollowingFilter::new("t", 5, 5).is_err());
    }

    #[test]
    fn test_tff_unavailable_before_warmup() {
        let mut t = TrendFollowingFilter::new("t", 2, 4).unwrap();
        assert_eq!(t.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert!(!t.is_ready());
    }

    #[test]
    fn test_tff_flat_is_zero() {
        // Flat price → fast_ema = slow_ema → output = 0
        let mut t = TrendFollowingFilter::new("t", 2, 4).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..10 { last = t.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_tff_uptrend_is_one() {
        let mut t = TrendFollowingFilter::new("t", 2, 4).unwrap();
        // Rising prices → fast EMA ahead of slow → uptrend
        let prices: Vec<u32> = (100..=120).collect();
        let mut last = SignalValue::Unavailable;
        for p in &prices { last = t.update_bar(&bar(&p.to_string())).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(1));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_tff_strength_set_when_ready() {
        let mut t = TrendFollowingFilter::new("t", 2, 4).unwrap();
        for p in 100..=110u32 { t.update_bar(&bar(&p.to_string())).unwrap(); }
        assert!(t.trend_strength().is_some());
    }

    #[test]
    fn test_tff_reset() {
        let mut t = TrendFollowingFilter::new("t", 2, 4).unwrap();
        for _ in 0..10 { t.update_bar(&bar("100")).unwrap(); }
        assert!(t.is_ready());
        t.reset();
        assert!(!t.is_ready());
        assert!(t.trend_strength().is_none());
    }
}
