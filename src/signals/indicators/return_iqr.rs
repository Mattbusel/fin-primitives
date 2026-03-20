//! Return IQR — interquartile range of rolling close-to-close returns.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Return IQR — `Q75 - Q25` of close-to-close returns over a rolling window.
///
/// The interquartile range is a robust measure of return dispersion / volatility that
/// is not affected by outliers or extreme events:
/// - **Large IQR**: returns are spread out widely — high variability.
/// - **Small IQR**: returns are concentrated — low intraperiod volatility.
///
/// Uses `(close - prev_close) / prev_close` as the return measure.
/// Returns [`SignalValue::Unavailable`] until `period` returns have been collected.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period < 4`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ReturnIqr;
/// use fin_primitives::signals::Signal;
/// let iqr = ReturnIqr::new("iqr_20", 20).unwrap();
/// assert_eq!(iqr.period(), 20);
/// ```
pub struct ReturnIqr {
    name: String,
    period: usize,
    returns: VecDeque<Decimal>,
    prev_close: Option<Decimal>,
}

impl ReturnIqr {
    /// Constructs a new `ReturnIqr`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 4`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 4 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            returns: VecDeque::with_capacity(period),
            prev_close: None,
        })
    }
}

fn quantile(sorted: &[Decimal], q: f64) -> Decimal {
    let n = sorted.len();
    if n == 0 { return Decimal::ZERO; }
    let pos = q * (n - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    if lo == hi {
        return sorted[lo];
    }
    let frac = Decimal::try_from(pos - pos.floor()).unwrap_or(Decimal::ZERO);
    sorted[lo] + (sorted[hi] - sorted[lo]) * frac
}

impl Signal for ReturnIqr {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.returns.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            if !pc.is_zero() {
                let ret = (bar.close - pc)
                    .checked_div(pc)
                    .ok_or(FinError::ArithmeticOverflow)?;
                self.returns.push_back(ret);
                if self.returns.len() > self.period {
                    self.returns.pop_front();
                }
            }
        }

        self.prev_close = Some(bar.close);

        if self.returns.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let mut sorted: Vec<Decimal> = self.returns.iter().copied().collect();
        sorted.sort();

        let q25 = quantile(&sorted, 0.25);
        let q75 = quantile(&sorted, 0.75);
        let iqr = (q75 - q25).max(Decimal::ZERO);

        Ok(SignalValue::Scalar(iqr))
    }

    fn reset(&mut self) {
        self.returns.clear();
        self.prev_close = None;
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
    fn test_iqr_invalid_period() {
        assert!(ReturnIqr::new("iqr", 0).is_err());
        assert!(ReturnIqr::new("iqr", 3).is_err());
    }

    #[test]
    fn test_iqr_unavailable_during_warmup() {
        let mut s = ReturnIqr::new("iqr", 4).unwrap();
        for p in &["100","101","102","103"] {
            assert_eq!(s.update_bar(&bar(p)).unwrap(), SignalValue::Unavailable);
        }
        assert!(!s.is_ready());
    }

    #[test]
    fn test_iqr_non_negative() {
        let mut s = ReturnIqr::new("iqr", 4).unwrap();
        let prices = ["100","103","99","104","98","105","97","106"];
        for p in &prices {
            if let SignalValue::Scalar(v) = s.update_bar(&bar(p)).unwrap() {
                assert!(v >= dec!(0), "IQR must be non-negative: {v}");
            }
        }
    }

    #[test]
    fn test_iqr_volatile_greater_than_flat() {
        // Volatile series should have larger IQR than flat series
        let mut s_volatile = ReturnIqr::new("iqr", 4).unwrap();
        let mut s_flat = ReturnIqr::new("iqr", 4).unwrap();

        // Volatile: large swings
        let volatile_prices = ["100","110","90","112","88","115"];
        // Flat: tiny moves
        let flat_prices = ["100","100.1","100.2","100.1","100.2","100.1"];

        let mut last_vol = SignalValue::Unavailable;
        let mut last_flat = SignalValue::Unavailable;
        for p in &volatile_prices { last_vol = s_volatile.update_bar(&bar(p)).unwrap(); }
        for p in &flat_prices { last_flat = s_flat.update_bar(&bar(p)).unwrap(); }

        if let (SignalValue::Scalar(v), SignalValue::Scalar(f)) = (last_vol, last_flat) {
            assert!(v > f, "volatile IQR ({v}) should exceed flat IQR ({f})");
        } else {
            panic!("expected Scalar values");
        }
    }

    #[test]
    fn test_iqr_reset() {
        let mut s = ReturnIqr::new("iqr", 4).unwrap();
        for p in &["100","102","104","106","108"] { s.update_bar(&bar(p)).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
