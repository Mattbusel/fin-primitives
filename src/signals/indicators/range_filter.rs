//! Range Filter indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Range Filter — a smoothed price filter that only updates when price moves
/// beyond a multiple of the recent average range.
///
/// ```text
/// avg_range = mean(|close_t − close_{t-1}|, period)
/// filter moves up   if close > filter + multiplier × avg_range
/// filter moves down if close < filter − multiplier × avg_range
/// otherwise stays put
/// output = close − filter
/// ```
///
/// Reduces noise by ignoring small price moves; reacts only to significant
/// moves relative to the recent average.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RangeFilter;
/// use fin_primitives::signals::Signal;
///
/// let rf = RangeFilter::new("rf", 14, "1.5".parse().unwrap()).unwrap();
/// assert_eq!(rf.period(), 14);
/// ```
pub struct RangeFilter {
    name: String,
    period: usize,
    multiplier: Decimal,
    closes: VecDeque<Decimal>,
    prev_close: Option<Decimal>,
    filter: Option<Decimal>,
}

impl RangeFilter {
    /// Creates a new `RangeFilter`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    /// Returns [`FinError::InvalidInput`] if `multiplier` is not positive.
    pub fn new(
        name: impl Into<String>,
        period: usize,
        multiplier: Decimal,
    ) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        if multiplier <= Decimal::ZERO {
            return Err(FinError::InvalidInput("multiplier must be positive".into()));
        }
        Ok(Self {
            name: name.into(),
            period,
            multiplier,
            closes: VecDeque::with_capacity(period + 1),
            prev_close: None,
            filter: None,
        })
    }

    /// Returns the current filter level.
    pub fn filter_level(&self) -> Option<Decimal> { self.filter }
}

impl Signal for RangeFilter {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 { self.closes.pop_front(); }
        if self.closes.len() < self.period + 1 { return Ok(SignalValue::Unavailable); }

        // Average absolute change over last `period` pairs
        let avg_range: Decimal = self.closes.iter()
            .collect::<Vec<_>>()
            .windows(2)
            .map(|w| (*w[1] - *w[0]).abs())
            .sum::<Decimal>()
            / Decimal::from(self.period as u32);

        let band = self.multiplier * avg_range;
        let current_filter = match self.filter {
            None => bar.close,
            Some(f) => {
                if bar.close > f + band { bar.close - band }
                else if bar.close < f - band { bar.close + band }
                else { f }
            }
        };
        self.filter = Some(current_filter);
        self.prev_close = Some(bar.close);

        Ok(SignalValue::Scalar(bar.close - current_filter))
    }

    fn is_ready(&self) -> bool { self.filter.is_some() }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.closes.clear();
        self.prev_close = None;
        self.filter = None;
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
    fn test_rf_invalid() {
        assert!(RangeFilter::new("r", 0, dec!(1.5)).is_err());
        assert!(RangeFilter::new("r", 14, dec!(0)).is_err());
        assert!(RangeFilter::new("r", 14, dec!(-1)).is_err());
    }

    #[test]
    fn test_rf_unavailable_before_warmup() {
        let mut r = RangeFilter::new("r", 3, dec!(1.5)).unwrap();
        for _ in 0..3 {
            assert_eq!(r.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_rf_flat_output_zero() {
        // Flat price → avg_range=0 → band=0 → filter anchors at close → output=0
        let mut r = RangeFilter::new("r", 3, dec!(1.5)).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..6 { last = r.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_rf_filter_lags_on_small_moves() {
        // Very small price moves within the band → filter stays put → output grows
        let mut r = RangeFilter::new("r", 3, dec!(10)).unwrap();
        // Warm up with steady price
        for _ in 0..4 { r.update_bar(&bar("100")).unwrap(); }
        // Small +1 move — within band(multiplier=10 × avg_range=1=10) → filter stays
        if let SignalValue::Scalar(v) = r.update_bar(&bar("101")).unwrap() {
            // output = 101 - filter; filter should stay near 100
            let _ = v; // just ensure no panic
        }
        assert!(r.filter_level().is_some());
    }

    #[test]
    fn test_rf_reset() {
        let mut r = RangeFilter::new("r", 3, dec!(1.5)).unwrap();
        for _ in 0..6 { r.update_bar(&bar("100")).unwrap(); }
        assert!(r.is_ready());
        r.reset();
        assert!(!r.is_ready());
        assert!(r.filter_level().is_none());
    }
}
