//! Random Walk Index (RWI).

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Random Walk Index — compares the bar range to what a random walk would produce.
///
/// ```text
/// RWI_high[i] = max over k=1..period of  (high[i] - low[i-k]) / (ATR × √k)
/// RWI_low[i]  = max over k=1..period of  (high[i-k] - low[i]) / (ATR × √k)
/// ```
///
/// Values > 1.0 indicate a trending move; values ≤ 1.0 are consistent with random noise.
/// The returned [`SignalValue::Scalar`] is `RWI_high - RWI_low` (positive = bullish trend,
/// negative = bearish trend, near 0 = no trend).
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Rwi;
/// use fin_primitives::signals::Signal;
///
/// let rwi = Rwi::new("rwi8", 8).unwrap();
/// assert_eq!(rwi.period(), 8);
/// ```
pub struct Rwi {
    name: String,
    period: usize,
    bars: VecDeque<BarInput>,
    rwi_high: Option<Decimal>,
    rwi_low: Option<Decimal>,
}

impl Rwi {
    /// Creates a new `Rwi`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            bars: VecDeque::with_capacity(period + 1),
            rwi_high: None,
            rwi_low: None,
        })
    }

    /// Returns the most recent RWI high component.
    pub fn rwi_high(&self) -> Option<Decimal> { self.rwi_high }
    /// Returns the most recent RWI low component.
    pub fn rwi_low(&self) -> Option<Decimal> { self.rwi_low }
}

impl Signal for Rwi {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        use rust_decimal::prelude::ToPrimitive;

        self.bars.push_back(bar.clone());
        if self.bars.len() > self.period + 1 {
            self.bars.pop_front();
        }
        if self.bars.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }

        // Compute average true range over the window (simple average of |H-L|)
        let atr: f64 = {
            let sum: f64 = self.bars
                .iter()
                .map(|b| (b.high - b.low).to_f64().unwrap_or(0.0))
                .sum();
            sum / self.bars.len() as f64
        };

        if atr == 0.0 {
            self.rwi_high = Some(Decimal::ZERO);
            self.rwi_low = Some(Decimal::ZERO);
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let n = self.bars.len();
        let current = &self.bars[n - 1];
        let current_high = current.high.to_f64().unwrap_or(0.0);
        let current_low  = current.low.to_f64().unwrap_or(0.0);

        let mut max_high = 0.0f64;
        let mut max_low  = 0.0f64;

        for k in 1..=self.period {
            let idx = n - 1 - k;
            let past_high = self.bars[idx].high.to_f64().unwrap_or(0.0);
            let past_low  = self.bars[idx].low.to_f64().unwrap_or(0.0);
            let sqrt_k = (k as f64).sqrt();
            let denom = atr * sqrt_k;
            if denom > 0.0 {
                let rh = (current_high - past_low) / denom;
                let rl = (past_high - current_low) / denom;
                if rh > max_high { max_high = rh; }
                if rl > max_low  { max_low  = rl; }
            }
        }

        let rwi_h = Decimal::try_from(max_high).unwrap_or(Decimal::ZERO);
        let rwi_l = Decimal::try_from(max_low).unwrap_or(Decimal::ZERO);
        self.rwi_high = Some(rwi_h);
        self.rwi_low  = Some(rwi_l);

        Ok(SignalValue::Scalar(rwi_h - rwi_l))
    }

    fn is_ready(&self) -> bool {
        self.rwi_high.is_some()
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.bars.clear();
        self.rwi_high = None;
        self.rwi_low  = None;
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
    fn test_rwi_invalid_period() {
        assert!(Rwi::new("r", 0).is_err());
    }

    #[test]
    fn test_rwi_unavailable_before_period() {
        let mut rwi = Rwi::new("r", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(rwi.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_rwi_produces_scalar_after_period() {
        let mut rwi = Rwi::new("r", 3).unwrap();
        rwi.update_bar(&bar("100")).unwrap();
        rwi.update_bar(&bar("101")).unwrap();
        rwi.update_bar(&bar("102")).unwrap();
        let v = rwi.update_bar(&bar("103")).unwrap();
        assert!(matches!(v, SignalValue::Scalar(_)));
        assert!(rwi.is_ready());
    }

    #[test]
    fn test_rwi_flat_price_is_zero() {
        // zero range → atr == 0 → scalar 0
        let mut rwi = Rwi::new("r", 3).unwrap();
        for _ in 0..10 { rwi.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = rwi.update_bar(&bar("100")).unwrap() {
            assert_eq!(v, rust_decimal_macros::dec!(0));
        }
    }

    #[test]
    fn test_rwi_reset() {
        let mut rwi = Rwi::new("r", 3).unwrap();
        for _ in 0..5 { rwi.update_bar(&bar("100")).unwrap(); }
        assert!(rwi.is_ready());
        rwi.reset();
        assert!(!rwi.is_ready());
    }

    #[test]
    fn test_rwi_accessors() {
        let mut rwi = Rwi::new("r", 3).unwrap();
        for _ in 0..4 { rwi.update_bar(&bar("100")).unwrap(); }
        assert!(rwi.rwi_high().is_some());
        assert!(rwi.rwi_low().is_some());
    }
}
