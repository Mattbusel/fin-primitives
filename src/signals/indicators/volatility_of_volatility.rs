//! Volatility of Volatility — standard deviation of rolling ATR values.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volatility of Volatility — std dev of rolling ATR values over a meta-window.
///
/// Measures how stable (or unstable) volatility itself is:
/// - **High**: ATR is swinging widely — the volatility regime is unstable.
/// - **Low**: ATR is relatively stable — consistent volatility environment.
/// - **Near zero**: flat volatility — no regime change occurring.
///
/// Uses a rolling sum-of-TR for ATR (simple moving average, not Wilder smoothing).
/// Returns [`SignalValue::Unavailable`] until `2 * period` bars have been accumulated
/// (one `period` of warmup to compute ATR, another to compute its std dev).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period < 2`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolatilityOfVolatility;
/// use fin_primitives::signals::Signal;
/// let vov = VolatilityOfVolatility::new("vov_14", 14).unwrap();
/// assert_eq!(vov.period(), 14);
/// ```
pub struct VolatilityOfVolatility {
    name: String,
    period: usize,
    tr_window: VecDeque<Decimal>,
    tr_sum: Decimal,
    atr_window: VecDeque<Decimal>,
    atr_sum: Decimal,
    atr_sum_sq: Decimal,
    prev_close: Option<Decimal>,
}

impl VolatilityOfVolatility {
    /// Constructs a new `VolatilityOfVolatility`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            tr_window: VecDeque::with_capacity(period),
            tr_sum: Decimal::ZERO,
            atr_window: VecDeque::with_capacity(period),
            atr_sum: Decimal::ZERO,
            atr_sum_sq: Decimal::ZERO,
            prev_close: None,
        })
    }
}

impl Signal for VolatilityOfVolatility {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.atr_window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = bar.true_range(self.prev_close);
        self.prev_close = Some(bar.close);

        // Update TR rolling window
        self.tr_sum += tr;
        self.tr_window.push_back(tr);
        if self.tr_window.len() > self.period {
            let removed = self.tr_window.pop_front().unwrap();
            self.tr_sum -= removed;
        }

        // Once we have a full TR window, compute ATR
        if self.tr_window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let atr = self.tr_sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        // Update ATR rolling window
        self.atr_sum += atr;
        self.atr_sum_sq += atr * atr;
        self.atr_window.push_back(atr);
        if self.atr_window.len() > self.period {
            let removed = self.atr_window.pop_front().unwrap();
            self.atr_sum -= removed;
            self.atr_sum_sq -= removed * removed;
        }

        if self.atr_window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = Decimal::from(self.period as u32);
        let mean = self.atr_sum
            .checked_div(n)
            .ok_or(FinError::ArithmeticOverflow)?;
        let mean_sq = self.atr_sum_sq
            .checked_div(n)
            .ok_or(FinError::ArithmeticOverflow)?;
        let variance = (mean_sq - mean * mean).max(Decimal::ZERO);

        let std_f64 = variance.to_f64().unwrap_or(0.0).sqrt();
        let std = Decimal::try_from(std_f64).unwrap_or(Decimal::ZERO);

        Ok(SignalValue::Scalar(std))
    }

    fn reset(&mut self) {
        self.tr_window.clear();
        self.tr_sum = Decimal::ZERO;
        self.atr_window.clear();
        self.atr_sum = Decimal::ZERO;
        self.atr_sum_sq = Decimal::ZERO;
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

    fn bar(h: &str, l: &str, c: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vov_invalid_period() {
        assert!(VolatilityOfVolatility::new("vov", 0).is_err());
        assert!(VolatilityOfVolatility::new("vov", 1).is_err());
    }

    #[test]
    fn test_vov_unavailable_during_warmup() {
        let mut s = VolatilityOfVolatility::new("vov", 3).unwrap();
        // Need 2*period = 6 bars for ready
        for _ in 0..5 {
            assert_eq!(s.update_bar(&bar("110","90","100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!s.is_ready());
    }

    #[test]
    fn test_vov_constant_tr_gives_zero() {
        // Same bar every time → TR constant → ATR constant → std dev of ATR = 0
        let mut s = VolatilityOfVolatility::new("vov", 2).unwrap();
        for _ in 0..5 {
            s.update_bar(&bar("110","90","100")).unwrap();
        }
        if let SignalValue::Scalar(v) = s.update_bar(&bar("110","90","100")).unwrap() {
            assert!(v.abs() < dec!(0.001), "constant TR → VoV ≈ 0: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vov_varying_tr_gives_positive() {
        // Alternating wide and narrow bars → ATR changes → VoV > 0
        let mut s = VolatilityOfVolatility::new("vov", 2).unwrap();
        let bars = [("120","80","100"),("104","96","100"),("130","70","100"),("103","97","100"),
                    ("125","75","100")];
        let mut last = SignalValue::Unavailable;
        for &(h, l, c) in &bars { last = s.update_bar(&bar(h, l, c)).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(0), "varying TR → VoV > 0: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vov_reset() {
        let mut s = VolatilityOfVolatility::new("vov", 2).unwrap();
        for (h, l, c) in &[("110","90","100"),("115","85","100"),("108","92","100"),
                            ("112","88","100"),("109","91","100")] {
            s.update_bar(&bar(h, l, c)).unwrap();
        }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
