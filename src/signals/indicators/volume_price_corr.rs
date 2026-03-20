//! Volume Price Correlation indicator -- Pearson correlation of volume with close returns.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::collections::VecDeque;

/// Volume Price Correlation -- rolling Pearson correlation between bar volume and
/// the bar's close-to-close return over `period` bars.
///
/// ```text
/// return[t] = close[t] - close[t-1]   (raw return)
/// rho[t]    = corr(volume[t-period+1..t], return[t-period+1..t])
/// ```
///
/// A positive correlation means high-volume bars tend to accompany rising prices
/// (accumulation). A negative correlation indicates high volume on falling prices
/// (distribution).
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen
/// or if variance is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumePriceCorr;
/// use fin_primitives::signals::Signal;
/// let vpc = VolumePriceCorr::new("vpc", 20).unwrap();
/// assert_eq!(vpc.period(), 20);
/// ```
pub struct VolumePriceCorr {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    vols: VecDeque<Decimal>,
    rets: VecDeque<Decimal>,
}

impl VolumePriceCorr {
    /// Constructs a new `VolumePriceCorr`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            vols: VecDeque::with_capacity(period),
            rets: VecDeque::with_capacity(period),
        })
    }

    fn pearson(xs: &VecDeque<Decimal>, ys: &VecDeque<Decimal>) -> Option<Decimal> {
        let n = xs.len();
        if n < 2 { return None; }
        let nf = Decimal::from(n as u32);
        let mean_x: Decimal = xs.iter().sum::<Decimal>() / nf;
        let mean_y: Decimal = ys.iter().sum::<Decimal>() / nf;
        let mut cov = Decimal::ZERO;
        let mut var_x = Decimal::ZERO;
        let mut var_y = Decimal::ZERO;
        for (x, y) in xs.iter().zip(ys.iter()) {
            let dx = x - mean_x;
            let dy = y - mean_y;
            cov += dx * dy;
            var_x += dx * dx;
            var_y += dy * dy;
        }
        if var_x.is_zero() || var_y.is_zero() { return None; }
        let var_x_f = var_x.to_f64()?;
        let var_y_f = var_y.to_f64()?;
        let denom = (var_x_f * var_y_f).sqrt();
        if denom == 0.0 { return None; }
        let cov_f = cov.to_f64()?;
        Decimal::try_from(cov_f / denom).ok()
    }
}

impl Signal for VolumePriceCorr {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.vols.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let ret = bar.close - pc;
            self.vols.push_back(bar.volume);
            self.rets.push_back(ret);
            if self.vols.len() > self.period {
                self.vols.pop_front();
                self.rets.pop_front();
            }
        }
        self.prev_close = Some(bar.close);
        if self.vols.len() < self.period { return Ok(SignalValue::Unavailable); }
        match Self::pearson(&self.vols, &self.rets) {
            Some(rho) => Ok(SignalValue::Scalar(rho)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.vols.clear();
        self.rets.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(c: &str, vol: &str) -> OhlcvBar {
        let p = Price::new(c.parse().unwrap()).unwrap();
        let v = Quantity::new(vol.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p, volume: v,
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vpc_period_too_small() { assert!(VolumePriceCorr::new("v", 1).is_err()); }

    #[test]
    fn test_vpc_unavailable_before_period() {
        let mut v = VolumePriceCorr::new("v", 5).unwrap();
        assert_eq!(v.update_bar(&bar("100", "1000")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_vpc_positive_correlation() {
        // Rising prices with increasing volume -> positive correlation
        let mut v = VolumePriceCorr::new("v", 4).unwrap();
        v.update_bar(&bar("100", "100")).unwrap();
        v.update_bar(&bar("102", "200")).unwrap(); // ret=+2, vol=200
        v.update_bar(&bar("105", "400")).unwrap(); // ret=+3, vol=400
        v.update_bar(&bar("109", "700")).unwrap(); // ret=+4, vol=700
        let r = v.update_bar(&bar("114", "1100")).unwrap(); // ret=+5, vol=1100
        if let SignalValue::Scalar(rho) = r {
            assert!(rho > dec!(0), "expected positive correlation, got {rho}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vpc_reset() {
        let mut v = VolumePriceCorr::new("v", 4).unwrap();
        for (c, vol) in [("100","100"), ("101","200"), ("102","300"), ("103","400"), ("104","500")] {
            v.update_bar(&bar(c, vol)).unwrap();
        }
        assert!(v.is_ready());
        v.reset();
        assert!(!v.is_ready());
    }
}
