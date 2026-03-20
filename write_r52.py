import os

base = "src/signals/indicators"

rolling_skewness = """\
//! Rolling Skewness indicator -- skewness of close-to-close returns over N bars.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling Skewness -- measures the asymmetry of the return distribution over a
/// rolling `period`-bar window.
///
/// Uses the sample skewness formula:
/// ```text
/// skew = (n / ((n-1)(n-2))) * sum((r_i - mean)^3) / stddev^3
/// ```
///
/// Positive skewness indicates a right tail (occasional large gains);
/// negative skewness indicates a left tail (occasional large losses).
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen
/// (needs `period` returns from `period + 1` closes) or when stddev is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RollingSkewness;
/// use fin_primitives::signals::Signal;
/// let rs = RollingSkewness::new("rs", 20).unwrap();
/// assert_eq!(rs.period(), 20);
/// ```
pub struct RollingSkewness {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl RollingSkewness {
    /// Constructs a new `RollingSkewness`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 3`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 3 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            closes: VecDeque::with_capacity(period + 2),
        })
    }
}

impl Signal for RollingSkewness {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.closes.len() > self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 { self.closes.pop_front(); }
        if self.closes.len() <= self.period { return Ok(SignalValue::Unavailable); }

        let prices: Vec<f64> = self.closes.iter()
            .map(|d| d.to_string().parse::<f64>().unwrap_or(f64::NAN))
            .collect();
        let returns: Vec<f64> = prices.windows(2).map(|w| w[1] - w[0]).collect();
        let n = returns.len() as f64;
        if n < 3.0 { return Ok(SignalValue::Unavailable); }

        let mean = returns.iter().sum::<f64>() / n;
        let variance = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (n - 1.0);
        if variance <= 0.0 { return Ok(SignalValue::Unavailable); }
        let stddev = variance.sqrt();

        let m3 = returns.iter().map(|r| (r - mean).powi(3)).sum::<f64>();
        let skew = (n / ((n - 1.0) * (n - 2.0))) * m3 / stddev.powi(3);
        match Decimal::try_from(skew) {
            Ok(d) => Ok(SignalValue::Scalar(d)),
            Err(_) => Ok(SignalValue::Unavailable),
        }
    }

    fn reset(&mut self) { self.closes.clear(); }
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
    fn test_rs_period_too_small() { assert!(RollingSkewness::new("rs", 2).is_err()); }

    #[test]
    fn test_rs_unavailable_before_warmup() {
        let mut rs = RollingSkewness::new("rs", 5).unwrap();
        for _ in 0..5 {
            assert_eq!(rs.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_rs_symmetric_near_zero() {
        let mut rs = RollingSkewness::new("rs", 5).unwrap();
        // returns: +1,-1,+1,-1,+1 -> approx 0 skew
        let prices = ["100","101","100","101","100","101"];
        let mut last = SignalValue::Unavailable;
        for p in &prices { last = rs.update_bar(&bar(p)).unwrap(); }
        if let SignalValue::Scalar(s) = last {
            assert!(s.abs() < dec!(1), "expected near-zero skew for symmetric, got {s}");
        }
    }

    #[test]
    fn test_rs_reset() {
        let mut rs = RollingSkewness::new("rs", 5).unwrap();
        for i in 0u32..8 { rs.update_bar(&bar(&(100+i).to_string())).unwrap(); }
        assert!(rs.is_ready());
        rs.reset();
        assert!(!rs.is_ready());
    }
}
"""

close_above_ema = """\
//! Close-Above-EMA ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Close-Above-EMA Ratio -- percentage of the last `window` bars where close > EMA(period).
///
/// Measures bullish EMA position consistency. High values (>70) indicate price has been
/// persistently above its moving average (uptrend). Low values (<30) indicate a downtrend.
///
/// # Parameters
/// - `ema_period`: EMA smoothing period
/// - `window`: rolling look-back window for the fraction calculation
///
/// Returns [`SignalValue::Unavailable`] until the EMA has warmed up (`ema_period` bars)
/// and the window is full (`window` bars).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseAboveEma;
/// use fin_primitives::signals::Signal;
/// let cae = CloseAboveEma::new("cae", 20, 10).unwrap();
/// assert_eq!(cae.period(), 20);
/// ```
pub struct CloseAboveEma {
    name: String,
    ema_period: usize,
    window_size: usize,
    k: Decimal,
    ema: Option<Decimal>,
    ema_bars: usize,
    results: VecDeque<u8>,
    count: usize,
}

impl CloseAboveEma {
    /// Constructs a new `CloseAboveEma`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if either period is 0.
    pub fn new(name: impl Into<String>, ema_period: usize, window: usize) -> Result<Self, FinError> {
        if ema_period == 0 { return Err(FinError::InvalidPeriod(ema_period)); }
        if window == 0 { return Err(FinError::InvalidPeriod(window)); }
        #[allow(clippy::cast_possible_truncation)]
        let k = Decimal::TWO / (Decimal::from(ema_period as u32) + Decimal::ONE);
        Ok(Self {
            name: name.into(),
            ema_period,
            window_size: window,
            k,
            ema: None,
            ema_bars: 0,
            results: VecDeque::with_capacity(window),
            count: 0,
        })
    }

    /// Returns the EMA period.
    pub fn ema_period(&self) -> usize { self.ema_period }
}

impl Signal for CloseAboveEma {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.ema_period }
    fn is_ready(&self) -> bool { self.results.len() >= self.window_size }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let close = bar.close;
        self.ema = Some(match self.ema {
            None => close,
            Some(prev) => self.k * close + (Decimal::ONE - self.k) * prev,
        });
        self.ema_bars += 1;

        if self.ema_bars <= self.ema_period { return Ok(SignalValue::Unavailable); }

        let above: u8 = if close > self.ema.unwrap_or(Decimal::ZERO) { 1 } else { 0 };
        self.results.push_back(above);
        self.count += above as usize;
        if self.results.len() > self.window_size {
            if let Some(old) = self.results.pop_front() { self.count -= old as usize; }
        }
        if self.results.len() < self.window_size { return Ok(SignalValue::Unavailable); }

        #[allow(clippy::cast_possible_truncation)]
        let ratio = Decimal::from(self.count as u32)
            / Decimal::from(self.window_size as u32)
            * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(ratio))
    }

    fn reset(&mut self) {
        self.ema = None;
        self.ema_bars = 0;
        self.results.clear();
        self.count = 0;
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
    fn test_cae_period_0_error() { assert!(CloseAboveEma::new("c", 0, 5).is_err()); }
    #[test]
    fn test_cae_window_0_error() { assert!(CloseAboveEma::new("c", 10, 0).is_err()); }

    #[test]
    fn test_cae_unavailable_before_warmup() {
        let mut c = CloseAboveEma::new("c", 3, 2).unwrap();
        for _ in 0..3 {
            assert_eq!(c.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_cae_all_above_is_100() {
        // constant price, EMA == price, close NOT > ema (equal), so 0%
        // use rising prices to be above
        let mut c = CloseAboveEma::new("c", 3, 3).unwrap();
        // warm up EMA
        for _ in 0..3 { c.update_bar(&bar("100")).unwrap(); }
        // now push bars well above 100 (EMA will be ~ 100)
        for _ in 0..3 { c.update_bar(&bar("200")).unwrap(); }
        if let SignalValue::Scalar(v) = c.update_bar(&bar("200")).unwrap() {
            assert_eq!(v, dec!(100));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_cae_reset() {
        let mut c = CloseAboveEma::new("c", 3, 2).unwrap();
        for _ in 0..10 { c.update_bar(&bar("100")).unwrap(); }
        assert!(c.is_ready());
        c.reset();
        assert!(!c.is_ready());
    }
}
"""

volume_surge = """\
//! Volume Surge indicator -- flags bars where volume exceeds a threshold multiple of its SMA.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume Surge -- returns 1 when the current bar's volume exceeds `threshold` times
/// the rolling N-period SMA of volume, and 0 otherwise.
///
/// ```text
/// avg_vol[t] = SMA(volume, period)
/// surge[t]   = 1 if volume[t] > threshold * avg_vol[t], else 0
/// ```
///
/// A surge (value=1) signals abnormally high volume relative to recent norms,
/// which often accompanies significant price moves or institutional activity.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated
/// or if average volume is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeSurge2;
/// use fin_primitives::signals::Signal;
/// use rust_decimal_macros::dec;
/// let vs = VolumeSurge2::new("vs", 20, dec!(2.0)).unwrap();
/// assert_eq!(vs.period(), 20);
/// ```
pub struct VolumeSurge2 {
    name: String,
    period: usize,
    threshold: Decimal,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl VolumeSurge2 {
    /// Constructs a new `VolumeSurge2`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0` or threshold <= 0.
    pub fn new(name: impl Into<String>, period: usize, threshold: Decimal) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        if threshold <= Decimal::ZERO { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            threshold,
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }

    /// Returns the surge threshold multiplier.
    pub fn threshold(&self) -> Decimal { self.threshold }
}

impl Signal for VolumeSurge2 {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.volume);
        self.sum += bar.volume;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() { self.sum -= old; }
        }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        #[allow(clippy::cast_possible_truncation)]
        let avg = self.sum / Decimal::from(self.period as u32);
        if avg.is_zero() { return Ok(SignalValue::Unavailable); }
        let surge = if bar.volume > self.threshold * avg { Decimal::ONE } else { Decimal::ZERO };
        Ok(SignalValue::Scalar(surge))
    }

    fn reset(&mut self) {
        self.window.clear();
        self.sum = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(vol: &str) -> OhlcvBar {
        let p = Price::new(dec!(100)).unwrap();
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
    fn test_vs_period_0_error() { assert!(VolumeSurge2::new("vs", 0, dec!(2)).is_err()); }
    #[test]
    fn test_vs_negative_threshold_error() { assert!(VolumeSurge2::new("vs", 5, dec!(-1)).is_err()); }

    #[test]
    fn test_vs_unavailable_before_period() {
        let mut vs = VolumeSurge2::new("vs", 3, dec!(2)).unwrap();
        assert_eq!(vs.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_vs_no_surge() {
        let mut vs = VolumeSurge2::new("vs", 3, dec!(2)).unwrap();
        for _ in 0..3 { vs.update_bar(&bar("100")).unwrap(); }
        // avg=100, threshold=2, volume=100 -> 100 > 200? No
        let v = vs.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vs_surge_detected() {
        let mut vs = VolumeSurge2::new("vs", 3, dec!(2)).unwrap();
        for _ in 0..3 { vs.update_bar(&bar("100")).unwrap(); }
        // avg=100, threshold=2, volume=250 -> 250 > 200? Yes
        let v = vs.update_bar(&bar("250")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_vs_reset() {
        let mut vs = VolumeSurge2::new("vs", 2, dec!(2)).unwrap();
        vs.update_bar(&bar("100")).unwrap();
        vs.update_bar(&bar("100")).unwrap();
        assert!(vs.is_ready());
        vs.reset();
        assert!(!vs.is_ready());
    }
}
"""

files = {
    "rolling_skewness": rolling_skewness,
    "close_above_ema": close_above_ema,
    "volume_surge2": volume_surge,
}

for name, content in files.items():
    path = os.path.join(base, f"{name}.rs")
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(content)
    print(f"wrote {path}")

print("done")
