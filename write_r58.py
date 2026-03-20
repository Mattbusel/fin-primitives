import os

base = "src/signals/indicators"

value_at_risk5 = """\
//! Value at Risk 5% indicator -- 5th-percentile rolling close return.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Value at Risk 5% (VaR95) -- the 5th-percentile close-to-close return over
/// the last `period` bars, expressed as a percentage.
///
/// Interpretation: with 95% confidence, the one-bar loss will not exceed
/// the absolute value of this number (a negative value represents a loss).
///
/// ```text
/// return[t]  = (close[t] - close[t-1]) / close[t-1] * 100
/// var5pct[t] = percentile_5(returns, period)
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ValueAtRisk5;
/// use fin_primitives::signals::Signal;
/// let var = ValueAtRisk5::new("var5", 20).unwrap();
/// assert_eq!(var.period(), 20);
/// ```
pub struct ValueAtRisk5 {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<Decimal>,
}

impl ValueAtRisk5 {
    /// Constructs a new `ValueAtRisk5`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            window: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for ValueAtRisk5 {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            if !pc.is_zero() {
                let ret = (bar.close - pc) / pc * Decimal::ONE_HUNDRED;
                self.window.push_back(ret);
                if self.window.len() > self.period {
                    self.window.pop_front();
                }
            }
        }
        self.prev_close = Some(bar.close);
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        let mut sorted: Vec<Decimal> = self.window.iter().copied().collect();
        sorted.sort();
        // 5th percentile index
        let idx = (self.period as f64 * 0.05) as usize;
        let idx = idx.min(sorted.len() - 1);
        Ok(SignalValue::Scalar(sorted[idx]))
    }

    fn reset(&mut self) {
        self.prev_close = None;
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
    fn test_var5_period_0_error() { assert!(ValueAtRisk5::new("v", 0).is_err()); }

    #[test]
    fn test_var5_unavailable_before_period() {
        let mut v = ValueAtRisk5::new("v", 5).unwrap();
        assert_eq!(v.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(v.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_var5_all_positive_returns() {
        // All returns +1%, worst 5th percentile is still +1%
        let mut v = ValueAtRisk5::new("v", 4).unwrap();
        v.update_bar(&bar("100")).unwrap();
        v.update_bar(&bar("101")).unwrap();
        v.update_bar(&bar("102")).unwrap();
        v.update_bar(&bar("103")).unwrap();
        let r = v.update_bar(&bar("104")).unwrap();
        if let SignalValue::Scalar(var) = r {
            // All returns are positive ~1%, VaR5 >= 0
            assert!(var > dec!(0), "All positive returns, VaR5 should be > 0, got {var}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_var5_includes_big_loss() {
        // One big loss should pull down the 5th percentile
        let mut v = ValueAtRisk5::new("v", 5).unwrap();
        v.update_bar(&bar("100")).unwrap();
        v.update_bar(&bar("50")).unwrap();  // -50% return
        v.update_bar(&bar("51")).unwrap();
        v.update_bar(&bar("52")).unwrap();
        v.update_bar(&bar("53")).unwrap();
        let r = v.update_bar(&bar("54")).unwrap();
        if let SignalValue::Scalar(var) = r {
            // Sorted returns have the -50% as the smallest
            assert!(var < dec!(0), "Should have negative VaR5, got {var}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_var5_reset() {
        let mut v = ValueAtRisk5::new("v", 3).unwrap();
        for p in ["100", "101", "102", "103"] { v.update_bar(&bar(p)).unwrap(); }
        assert!(v.is_ready());
        v.reset();
        assert!(!v.is_ready());
    }
}
"""

volume_price_corr = """\
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
"""

net_high_low_count = """\
//! Net High-Low Count indicator -- rolling higher-highs minus lower-lows count.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Net High-Low Count -- rolling count of higher-high bars minus lower-low bars.
///
/// Each bar is classified as:
/// - Higher high: `high[t] > high[t-1]` contributes +1
/// - Lower low:   `low[t]  < low[t-1]`  contributes -1
/// - A bar can be both (outside bar) contributing 0 net
///
/// ```text
/// hh[t] = 1 if high[t] > high[t-1], else 0
/// ll[t] = 1 if low[t]  < low[t-1],  else 0
/// net[t] = sum(hh - ll, period)
/// ```
///
/// Positive values indicate predominantly rising highs; negative values suggest
/// falling lows dominate.
///
/// Returns [`SignalValue::Unavailable`] until `period` comparisons have been made
/// (requires `period + 1` bars).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::NetHighLowCount;
/// use fin_primitives::signals::Signal;
/// let nhl = NetHighLowCount::new("nhl", 10).unwrap();
/// assert_eq!(nhl.period(), 10);
/// ```
pub struct NetHighLowCount {
    name: String,
    period: usize,
    prev_high: Option<Decimal>,
    prev_low: Option<Decimal>,
    window: VecDeque<i8>,
    sum: i32,
}

impl NetHighLowCount {
    /// Constructs a new `NetHighLowCount`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            prev_high: None,
            prev_low: None,
            window: VecDeque::with_capacity(period),
            sum: 0,
        })
    }
}

impl Signal for NetHighLowCount {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let (Some(ph), Some(pl)) = (self.prev_high, self.prev_low) {
            let hh: i8 = if bar.high > ph { 1 } else { 0 };
            let ll: i8 = if bar.low < pl { 1 } else { 0 };
            let net: i8 = hh - ll;
            self.window.push_back(net);
            self.sum += net as i32;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() { self.sum -= old as i32; }
            }
        }
        self.prev_high = Some(bar.high);
        self.prev_low = Some(bar.low);
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        Ok(SignalValue::Scalar(Decimal::from(self.sum)))
    }

    fn reset(&mut self) {
        self.prev_high = None;
        self.prev_low = None;
        self.window.clear();
        self.sum = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let mp = Price::new((hp.value() + lp.value()) / Decimal::TWO).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: mp, high: hp, low: lp, close: mp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_nhl_period_0_error() { assert!(NetHighLowCount::new("nhl", 0).is_err()); }

    #[test]
    fn test_nhl_unavailable_before_period() {
        let mut nhl = NetHighLowCount::new("nhl", 3).unwrap();
        assert_eq!(nhl.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_nhl_all_higher_highs() {
        // Rising highs, stable lows -> all higher highs, no lower lows -> net = +period
        let mut nhl = NetHighLowCount::new("nhl", 3).unwrap();
        nhl.update_bar(&bar("100", "90")).unwrap(); // prev
        nhl.update_bar(&bar("105", "90")).unwrap(); // hh=1, ll=0 -> net=+1
        nhl.update_bar(&bar("110", "90")).unwrap(); // hh=1, ll=0 -> net=+1
        let v = nhl.update_bar(&bar("115", "90")).unwrap(); // hh=1, ll=0 -> net=+1 -> sum=3
        assert_eq!(v, SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_nhl_all_lower_lows() {
        // Stable highs, falling lows -> all lower lows -> net = -period
        let mut nhl = NetHighLowCount::new("nhl", 3).unwrap();
        nhl.update_bar(&bar("110", "95")).unwrap(); // prev
        nhl.update_bar(&bar("110", "90")).unwrap(); // hh=0, ll=1 -> net=-1
        nhl.update_bar(&bar("110", "85")).unwrap(); // hh=0, ll=1 -> net=-1
        let v = nhl.update_bar(&bar("110", "80")).unwrap(); // hh=0, ll=1 -> net=-1 -> sum=-3
        assert_eq!(v, SignalValue::Scalar(dec!(-3)));
    }

    #[test]
    fn test_nhl_flat_is_zero() {
        // Identical bars -> no HH, no LL -> net = 0
        let mut nhl = NetHighLowCount::new("nhl", 3).unwrap();
        nhl.update_bar(&bar("110", "90")).unwrap();
        nhl.update_bar(&bar("110", "90")).unwrap();
        nhl.update_bar(&bar("110", "90")).unwrap();
        let v = nhl.update_bar(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_nhl_reset() {
        let mut nhl = NetHighLowCount::new("nhl", 2).unwrap();
        nhl.update_bar(&bar("110", "90")).unwrap();
        nhl.update_bar(&bar("115", "85")).unwrap();
        nhl.update_bar(&bar("120", "80")).unwrap();
        assert!(nhl.is_ready());
        nhl.reset();
        assert!(!nhl.is_ready());
    }
}
"""

rolling_skew_returns = """\
//! Rolling Skewness of Returns indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::collections::VecDeque;

/// Rolling Skewness of Returns -- measures the asymmetry of the close-return distribution
/// over the last `period` bars.
///
/// Positive skew: right tail (large gains) is longer than left (mean > median).
/// Negative skew: left tail (large losses) dominates (mean < median).
///
/// Uses the sample skewness formula:
/// ```text
/// return[t] = close[t] - close[t-1]
/// skew      = (n / ((n-1)(n-2))) * sum((r - mean)^3 / std^3)
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen
/// (needs `period` returns) or if standard deviation is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RollingSkewReturns;
/// use fin_primitives::signals::Signal;
/// let rs = RollingSkewReturns::new("rs", 20).unwrap();
/// assert_eq!(rs.period(), 20);
/// ```
pub struct RollingSkewReturns {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<Decimal>,
}

impl RollingSkewReturns {
    /// Constructs a new `RollingSkewReturns`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 3`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 3 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            window: VecDeque::with_capacity(period),
        })
    }

    fn compute_skew(returns: &VecDeque<Decimal>) -> Option<Decimal> {
        let n = returns.len();
        if n < 3 { return None; }
        let nf = n as f64;
        let xs: Vec<f64> = returns.iter().filter_map(|r| r.to_f64()).collect();
        if xs.len() != n { return None; }
        let mean = xs.iter().sum::<f64>() / nf;
        let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (nf - 1.0);
        let std = var.sqrt();
        if std == 0.0 { return None; }
        let m3 = xs.iter().map(|x| ((x - mean) / std).powi(3)).sum::<f64>();
        let skew = (nf / ((nf - 1.0) * (nf - 2.0))) * m3;
        Decimal::try_from(skew).ok()
    }
}

impl Signal for RollingSkewReturns {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let ret = bar.close - pc;
            self.window.push_back(ret);
            if self.window.len() > self.period {
                self.window.pop_front();
            }
        }
        self.prev_close = Some(bar.close);
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        match Self::compute_skew(&self.window) {
            Some(s) => Ok(SignalValue::Scalar(s)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn reset(&mut self) {
        self.prev_close = None;
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
    fn test_rsr_period_too_small() { assert!(RollingSkewReturns::new("rs", 2).is_err()); }

    #[test]
    fn test_rsr_unavailable_before_period() {
        let mut rs = RollingSkewReturns::new("rs", 5).unwrap();
        assert_eq!(rs.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(rs.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_rsr_symmetric_returns_near_zero_skew() {
        // Symmetric returns: +1, -1, +1, -1 should give skew near 0
        let mut rs = RollingSkewReturns::new("rs", 4).unwrap();
        rs.update_bar(&bar("100")).unwrap();
        rs.update_bar(&bar("101")).unwrap(); // +1
        rs.update_bar(&bar("100")).unwrap(); // -1
        rs.update_bar(&bar("101")).unwrap(); // +1
        let r = rs.update_bar(&bar("100")).unwrap(); // -1
        if let SignalValue::Scalar(s) = r {
            // Symmetric distribution -> skew should be close to 0
            assert!(s.abs() < dec!(0.1), "symmetric returns, skew near 0, got {s}");
        }
    }

    #[test]
    fn test_rsr_constant_returns_unavailable() {
        // Constant returns: std=0 -> Unavailable
        let mut rs = RollingSkewReturns::new("rs", 3).unwrap();
        rs.update_bar(&bar("100")).unwrap();
        rs.update_bar(&bar("101")).unwrap(); // +1
        rs.update_bar(&bar("102")).unwrap(); // +1
        let r = rs.update_bar(&bar("103")).unwrap(); // +1 -> std=0
        assert_eq!(r, SignalValue::Unavailable);
    }

    #[test]
    fn test_rsr_reset() {
        let mut rs = RollingSkewReturns::new("rs", 4).unwrap();
        for p in ["100", "101", "100", "101", "100"] { rs.update_bar(&bar(p)).unwrap(); }
        assert!(rs.is_ready());
        rs.reset();
        assert!(!rs.is_ready());
    }
}
"""

files = {
    "value_at_risk5": value_at_risk5,
    "volume_price_corr": volume_price_corr,
    "net_high_low_count": net_high_low_count,
    "rolling_skew_returns": rolling_skew_returns,
}

for name, content in files.items():
    path = os.path.join(base, f"{name}.rs")
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(content)
    print(f"wrote {path}")

print("done")
