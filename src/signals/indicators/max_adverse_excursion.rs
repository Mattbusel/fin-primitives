//! Max Adverse Excursion indicator -- rolling peak-to-trough drawdown in a window.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Max Adverse Excursion (MAE) -- the maximum percentage drawdown from the rolling
/// N-period peak close to any subsequent close within the window.
///
/// ```text
/// peak[t]  = max(close[t-period+1..t])
/// trough   = min(close after peak in window)
/// mae[t]   = (trough - peak) / peak x 100    (always <= 0)
/// ```
///
/// A value of -5 means at some point within the window, price fell 5% from its
/// peak. Useful for measuring risk exposure within a lookback window.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::MaxAdverseExcursion;
/// use fin_primitives::signals::Signal;
/// let mae = MaxAdverseExcursion::new("mae", 20).unwrap();
/// assert_eq!(mae.period(), 20);
/// ```
pub struct MaxAdverseExcursion {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
}

impl MaxAdverseExcursion {
    /// Constructs a new `MaxAdverseExcursion`.
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

impl Signal for MaxAdverseExcursion {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.close);
        if self.window.len() > self.period { self.window.pop_front(); }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }

        let prices: Vec<Decimal> = self.window.iter().copied().collect();
        let mut max_dd = Decimal::ZERO;
        let mut peak = prices[0];

        for &p in &prices[1..] {
            if p > peak { peak = p; }
            if peak.is_zero() { continue; }
            let dd = (p - peak) / peak * Decimal::ONE_HUNDRED;
            if dd < max_dd { max_dd = dd; }
        }

        Ok(SignalValue::Scalar(max_dd))
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
    fn test_mae_period_too_small() { assert!(MaxAdverseExcursion::new("m", 1).is_err()); }

    #[test]
    fn test_mae_unavailable_before_period() {
        let mut m = MaxAdverseExcursion::new("m", 3).unwrap();
        assert_eq!(m.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_mae_uptrend_zero_drawdown() {
        // Always going up -> no drawdown from peak
        let mut m = MaxAdverseExcursion::new("m", 3).unwrap();
        m.update_bar(&bar("100")).unwrap();
        m.update_bar(&bar("105")).unwrap();
        let v = m.update_bar(&bar("110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_mae_downtrend() {
        // 100 -> 90 -> 80: peak=100, min=80, drawdown = (80-100)/100*100 = -20
        let mut m = MaxAdverseExcursion::new("m", 3).unwrap();
        m.update_bar(&bar("100")).unwrap();
        m.update_bar(&bar("90")).unwrap();
        let v = m.update_bar(&bar("80")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-20)));
    }

    #[test]
    fn test_mae_recovery_still_tracks_peak() {
        // 100 -> 80 -> 90: peak=100, trough=80, drawdown=-20 even though recovered
        let mut m = MaxAdverseExcursion::new("m", 3).unwrap();
        m.update_bar(&bar("100")).unwrap();
        m.update_bar(&bar("80")).unwrap();
        let v = m.update_bar(&bar("90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-20)));
    }

    #[test]
    fn test_mae_reset() {
        let mut m = MaxAdverseExcursion::new("m", 2).unwrap();
        m.update_bar(&bar("100")).unwrap();
        m.update_bar(&bar("90")).unwrap();
        assert!(m.is_ready());
        m.reset();
        assert!(!m.is_ready());
    }
}
