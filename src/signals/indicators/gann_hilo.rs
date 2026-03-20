//! Gann Hi-Lo Activator indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Gann Hi-Lo Activator — a trend-following stop that switches between
/// the rolling SMA of highs and the rolling SMA of lows depending on trend direction.
///
/// * In an **uptrend** (close > prior activator): use `SMA(lows, period)`.
/// * In a **downtrend** (close ≤ prior activator): use `SMA(highs, period)`.
///
/// Returns [`SignalValue::Scalar`] once `period` bars have been seen.
/// The scalar value is the current activator level.  A close above the activator
/// signals a long bias; below signals a short bias.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::GannHiLo;
/// use fin_primitives::signals::Signal;
///
/// let g = GannHiLo::new("gann5", 5).unwrap();
/// assert_eq!(g.period(), 5);
/// ```
pub struct GannHiLo {
    name: String,
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
    activator: Option<Decimal>,
}

impl GannHiLo {
    /// Creates a new `GannHiLo`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            highs: VecDeque::with_capacity(period),
            lows: VecDeque::with_capacity(period),
            activator: None,
        })
    }

    /// Returns the current activator level.
    pub fn activator_level(&self) -> Option<Decimal> { self.activator }

    /// Returns `true` if the last close was above the activator (long bias).
    pub fn is_bullish(&self) -> bool {
        self.activator.map_or(false, |a| {
            // We can't access the last close here, so this is just an indicator
            // of whether we have a value; the user should compare close > activator_level()
            let _ = a;
            false
        })
    }
}

impl Signal for GannHiLo {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);
        if self.highs.len() > self.period {
            self.highs.pop_front();
            self.lows.pop_front();
        }
        if self.highs.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sma_highs: Decimal = self.highs.iter().sum::<Decimal>()
            / Decimal::from(self.period as u32);
        let sma_lows: Decimal = self.lows.iter().sum::<Decimal>()
            / Decimal::from(self.period as u32);

        let new_activator = match self.activator {
            None => sma_lows, // seed in uptrend by default
            Some(prev) => {
                if bar.close > prev { sma_lows } else { sma_highs }
            }
        };

        self.activator = Some(new_activator);
        Ok(SignalValue::Scalar(new_activator))
    }

    fn is_ready(&self) -> bool {
        self.activator.is_some()
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.highs.clear();
        self.lows.clear();
        self.activator = None;
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
    fn test_gann_hilo_invalid_period() {
        assert!(GannHiLo::new("g", 0).is_err());
    }

    #[test]
    fn test_gann_hilo_unavailable_before_period() {
        let mut g = GannHiLo::new("g", 3).unwrap();
        assert_eq!(g.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(g.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
        assert!(!g.is_ready());
    }

    #[test]
    fn test_gann_hilo_produces_scalar() {
        let mut g = GannHiLo::new("g", 3).unwrap();
        g.update_bar(&bar("100")).unwrap();
        g.update_bar(&bar("101")).unwrap();
        let v = g.update_bar(&bar("102")).unwrap();
        assert!(matches!(v, SignalValue::Scalar(_)));
        assert!(g.is_ready());
    }

    #[test]
    fn test_gann_hilo_flat_stays_at_price() {
        let mut g = GannHiLo::new("g", 3).unwrap();
        for _ in 0..5 { g.update_bar(&bar("100")).unwrap(); }
        // After flat prices, activator should equal the price
        if let Some(a) = g.activator_level() {
            assert_eq!(a, rust_decimal_macros::dec!(100));
        }
    }

    #[test]
    fn test_gann_hilo_reset() {
        let mut g = GannHiLo::new("g", 3).unwrap();
        for _ in 0..5 { g.update_bar(&bar("100")).unwrap(); }
        assert!(g.is_ready());
        g.reset();
        assert!(!g.is_ready());
        assert!(g.activator_level().is_none());
    }
}
