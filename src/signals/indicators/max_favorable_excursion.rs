//! Max Favorable Excursion indicator -- rolling trough-to-peak rally in a window.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Max Favorable Excursion (MFE) -- the maximum percentage rally from the rolling
/// N-period trough close to any subsequent close within the window.
///
/// ```text
/// trough[t] = min(close[t-period+1..t])
/// peak      = max(close after trough in window)
/// mfe[t]    = (peak - trough) / trough × 100    (always >= 0)
/// ```
///
/// A value of 8 means at some point within the window, price rallied 8% from its
/// trough. Useful for measuring upside potential within a lookback window, and
/// as the complement of [`MaxAdverseExcursion`](crate::signals::indicators::MaxAdverseExcursion).
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period < 2`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::MaxFavorableExcursion;
/// use fin_primitives::signals::Signal;
/// let mfe = MaxFavorableExcursion::new("mfe", 20).unwrap();
/// assert_eq!(mfe.period(), 20);
/// ```
pub struct MaxFavorableExcursion {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
}

impl MaxFavorableExcursion {
    /// Constructs a new `MaxFavorableExcursion`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            window: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for MaxFavorableExcursion {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.close);
        if self.window.len() > self.period { self.window.pop_front(); }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }

        let prices: Vec<Decimal> = self.window.iter().copied().collect();
        let mut max_mfe = Decimal::ZERO;
        let mut trough = prices[0];

        for &p in &prices[1..] {
            if p < trough { trough = p; }
            if trough.is_zero() { continue; }
            let excursion = (p - trough)
                .checked_div(trough)
                .ok_or(FinError::ArithmeticOverflow)?
                * Decimal::ONE_HUNDRED;
            if excursion > max_mfe { max_mfe = excursion; }
        }

        Ok(SignalValue::Scalar(max_mfe))
    }

    fn reset(&mut self) {
        self.window.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
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
    fn test_mfe_period_too_small() { assert!(MaxFavorableExcursion::new("m", 1).is_err()); }

    #[test]
    fn test_mfe_unavailable_before_period() {
        let mut m = MaxFavorableExcursion::new("m", 3).unwrap();
        assert_eq!(m.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_mfe_downtrend_zero() {
        // Always going down -> no rally from trough
        let mut m = MaxFavorableExcursion::new("m", 3).unwrap();
        m.update_bar(&bar("110")).unwrap();
        m.update_bar(&bar("105")).unwrap();
        let v = m.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_mfe_uptrend() {
        // 80 -> 90 -> 100: trough=80, peak after trough=100, mfe = (100-80)/80*100 = 25
        let mut m = MaxFavorableExcursion::new("m", 3).unwrap();
        m.update_bar(&bar("80")).unwrap();
        m.update_bar(&bar("90")).unwrap();
        let v = m.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(25)));
    }

    #[test]
    fn test_mfe_pullback_tracks_rally() {
        // 100 -> 120 -> 110: trough=100, best price after=120, mfe=20
        let mut m = MaxFavorableExcursion::new("m", 3).unwrap();
        m.update_bar(&bar("100")).unwrap();
        m.update_bar(&bar("120")).unwrap();
        let v = m.update_bar(&bar("110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(20)));
    }

    #[test]
    fn test_mfe_reset() {
        let mut m = MaxFavorableExcursion::new("m", 2).unwrap();
        m.update_bar(&bar("90")).unwrap();
        m.update_bar(&bar("100")).unwrap();
        assert!(m.is_ready());
        m.reset();
        assert!(!m.is_ready());
    }
}
