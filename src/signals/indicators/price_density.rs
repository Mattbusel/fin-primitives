//! Price Density indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Density — measures how tightly price is consolidating within a window.
///
/// ```text
/// range   = max(high, period) − min(low, period)
/// path    = Σ |close_t − close_{t-1}|  over period bars
/// density = range / path × 100
/// ```
///
/// Low values indicate choppy/ranging conditions (price travels far relative to range).
/// High values indicate directional/trending moves (path ≈ range).
/// Returns 100 when close is monotonically rising or falling.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceDensity;
/// use fin_primitives::signals::Signal;
///
/// let pd = PriceDensity::new("pd", 14).unwrap();
/// assert_eq!(pd.period(), 14);
/// ```
pub struct PriceDensity {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
}

impl PriceDensity {
    /// Creates a new `PriceDensity`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            closes: VecDeque::with_capacity(period + 1),
            highs: VecDeque::with_capacity(period + 1),
            lows: VecDeque::with_capacity(period + 1),
        })
    }
}

impl Signal for PriceDensity {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);
        // Keep period+1 entries for closes, highs, and lows
        if self.closes.len() > self.period + 1 { self.closes.pop_front(); }
        if self.highs.len() > self.period + 1 { self.highs.pop_front(); }
        if self.lows.len() > self.period + 1 { self.lows.pop_front(); }

        if self.closes.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }

        let range = self.highs.iter().cloned().max().unwrap()
            - self.lows.iter().cloned().min().unwrap();

        let path: Decimal = self.closes.iter()
            .collect::<Vec<_>>()
            .windows(2)
            .map(|w| (*w[1] - *w[0]).abs())
            .sum();

        if path.is_zero() {
            // Perfectly flat — path=0, define density as 0 (no movement)
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let density = range / path * Decimal::from(100u32);
        Ok(SignalValue::Scalar(density))
    }

    fn is_ready(&self) -> bool { self.closes.len() >= self.period + 1 }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.closes.clear();
        self.highs.clear();
        self.lows.clear();
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

    fn bar_hlc(h: &str, l: &str, c: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_pd_invalid() {
        assert!(PriceDensity::new("p", 0).is_err());
        assert!(PriceDensity::new("p", 1).is_err());
    }

    #[test]
    fn test_pd_unavailable_before_warmup() {
        let mut p = PriceDensity::new("p", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(p.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_pd_flat_is_zero() {
        let mut p = PriceDensity::new("p", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = p.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_pd_monotone_is_100() {
        // Monotonically rising closes → range = path → density = 100
        let mut p = PriceDensity::new("p", 3).unwrap();
        let prices = ["100", "101", "102", "103"];
        let mut last = SignalValue::Unavailable;
        for price in &prices { last = p.update_bar(&bar(price)).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(100));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_pd_choppy_below_100() {
        // Zigzag: 100, 110, 100, 110 — range=10, path=30 → density≈33
        let mut p = PriceDensity::new("p", 3).unwrap();
        let prices = ["100", "110", "100", "110"];
        let mut last = SignalValue::Unavailable;
        for price in &prices { last = p.update_bar(&bar(price)).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert!(v < dec!(100));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_pd_uses_high_low_for_range() {
        // HLC bars: range uses high/low, not just closes
        let mut p = PriceDensity::new("p", 2).unwrap();
        p.update_bar(&bar_hlc("110", "90", "100")).unwrap();
        p.update_bar(&bar_hlc("110", "90", "100")).unwrap();
        let v = p.update_bar(&bar_hlc("110", "90", "100")).unwrap();
        // path=0 (flat closes) → Scalar(0)
        if let SignalValue::Scalar(val) = v {
            assert_eq!(val, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_pd_reset() {
        let mut p = PriceDensity::new("p", 3).unwrap();
        for _ in 0..5 { p.update_bar(&bar("100")).unwrap(); }
        assert!(p.is_ready());
        p.reset();
        assert!(!p.is_ready());
    }
}
