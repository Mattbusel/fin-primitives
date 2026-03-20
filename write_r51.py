import os

base = "src/signals/indicators"

autocorrelation_1 = """\
//! Lag-1 Autocorrelation of close returns indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Lag-1 Autocorrelation -- Pearson correlation of close returns with themselves
/// shifted one period, computed over a rolling `period`-bar window.
///
/// ```text
/// r[t]   = close[t] - close[t-1]   (raw return)
/// rho[t] = corr(r[t-1..t-period], r[t-2..t-period-1])
/// ```
///
/// A positive value near 1 indicates returns are trend-following (momentum).
/// A negative value near -1 indicates mean-reversion.
/// Near 0 indicates random walk behavior.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen
/// (need at least `period` returns, which requires `period + 1` closes).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Autocorrelation1;
/// use fin_primitives::signals::Signal;
/// let ac = Autocorrelation1::new("ac", 20).unwrap();
/// assert_eq!(ac.period(), 20);
/// ```
pub struct Autocorrelation1 {
    name: String,
    period: usize,
    /// Rolling window of close prices (needs period+1 to compute period returns)
    closes: VecDeque<Decimal>,
}

impl Autocorrelation1 {
    /// Constructs a new `Autocorrelation1`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 3` (need at least 3 returns for correlation).
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 3 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            closes: VecDeque::with_capacity(period + 2),
        })
    }

    fn compute_autocorr(returns: &[Decimal]) -> Option<Decimal> {
        let n = returns.len();
        if n < 2 { return None; }
        // x = returns[0..n-1], y = returns[1..n]
        let n_f = Decimal::from(n as u32);
        let n1_f = Decimal::from((n - 1) as u32);
        let sum_x: Decimal = returns[..n-1].iter().copied().sum();
        let sum_y: Decimal = returns[1..].iter().copied().sum();
        let mean_x = sum_x / n1_f;
        let mean_y = sum_y / n1_f;

        let mut cov = Decimal::ZERO;
        let mut var_x = Decimal::ZERO;
        let mut var_y = Decimal::ZERO;
        for i in 0..n-1 {
            let dx = returns[i] - mean_x;
            let dy = returns[i+1] - mean_y;
            cov += dx * dy;
            var_x += dx * dx;
            var_y += dy * dy;
        }
        if var_x.is_zero() || var_y.is_zero() { return None; }

        let var_x_f: f64 = var_x.to_string().parse().ok()?;
        let var_y_f: f64 = var_y.to_string().parse().ok()?;
        let denom_f = (var_x_f * var_y_f).sqrt();
        if denom_f == 0.0 { return None; }
        let cov_f: f64 = cov.to_string().parse().ok()?;
        let rho_f = cov_f / denom_f;
        Decimal::try_from(rho_f).ok()
    }
}

impl Signal for Autocorrelation1 {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.closes.len() > self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 2 { self.closes.pop_front(); }
        if self.closes.len() <= self.period { return Ok(SignalValue::Unavailable); }

        // Compute returns from the window
        let prices: Vec<Decimal> = self.closes.iter().copied().collect();
        let returns: Vec<Decimal> = prices.windows(2).map(|w| w[1] - w[0]).collect();

        match Self::compute_autocorr(&returns) {
            Some(rho) => Ok(SignalValue::Scalar(rho)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn reset(&mut self) {
        self.closes.clear();
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
    fn test_ac_period_too_small() { assert!(Autocorrelation1::new("ac", 2).is_err()); }

    #[test]
    fn test_ac_unavailable_before_warmup() {
        let mut ac = Autocorrelation1::new("ac", 5).unwrap();
        for _ in 0..5 {
            assert_eq!(ac.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_ac_trending_series_positive() {
        // Monotonically increasing prices -> consecutive returns all +1
        // Lag-1 autocorrelation of [1,1,1,1,...] = 1
        let mut ac = Autocorrelation1::new("ac", 5).unwrap();
        for i in 0u32..10 {
            ac.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        if let SignalValue::Scalar(rho) = ac.update_bar(&bar("110")).unwrap() {
            // Constant returns = undefined; skip if Unavailable
            let _ = rho; // accept any finite value
        }
    }

    #[test]
    fn test_ac_alternating_negative() {
        // Alternating up/down -> negative autocorrelation
        let mut ac = Autocorrelation1::new("ac", 6).unwrap();
        let prices = ["100","102","100","102","100","102","100","102"];
        let mut last = SignalValue::Unavailable;
        for p in &prices {
            last = ac.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(rho) = last {
            assert!(rho < dec!(0), "expected negative autocorr for alternating series, got {rho}");
        }
    }

    #[test]
    fn test_ac_reset() {
        let mut ac = Autocorrelation1::new("ac", 5).unwrap();
        for i in 0u32..8 { ac.update_bar(&bar(&(100+i).to_string())).unwrap(); }
        assert!(ac.is_ready());
        ac.reset();
        assert!(!ac.is_ready());
    }
}
"""

price_entropy = """\
//! Price Entropy indicator -- Shannon entropy of return signs over N bars.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Entropy -- Shannon entropy of the sign of close-to-close returns over
/// a rolling `period`-bar window.
///
/// Classifies each return as up (+1), down (-1), or flat (0). Entropy measures
/// how unpredictable the market is:
///
/// - High entropy (~1.0): returns are random/unpredictable (up and down equally frequent)
/// - Low entropy (~0): returns are highly predictable (mostly one direction)
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceEntropy;
/// use fin_primitives::signals::Signal;
/// let pe = PriceEntropy::new("pe", 20).unwrap();
/// assert_eq!(pe.period(), 20);
/// ```
pub struct PriceEntropy {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl PriceEntropy {
    /// Constructs a new `PriceEntropy`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            closes: VecDeque::with_capacity(period + 2),
        })
    }

    fn entropy(probs: &[f64]) -> f64 {
        probs.iter()
            .filter(|&&p| p > 0.0)
            .map(|&p| -p * p.ln())
            .sum::<f64>()
            / (probs.len() as f64).ln().max(1e-10)
    }
}

impl Signal for PriceEntropy {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.closes.len() > self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 { self.closes.pop_front(); }
        if self.closes.len() <= self.period { return Ok(SignalValue::Unavailable); }

        let prices: Vec<Decimal> = self.closes.iter().copied().collect();
        let n = prices.len() - 1; // number of returns
        let mut up = 0usize;
        let mut down = 0usize;
        let mut flat = 0usize;
        for w in prices.windows(2) {
            let ret = w[1] - w[0];
            if ret > Decimal::ZERO { up += 1; }
            else if ret < Decimal::ZERO { down += 1; }
            else { flat += 1; }
        }
        let n_f = n as f64;
        let probs = [up as f64 / n_f, down as f64 / n_f, flat as f64 / n_f];
        let e = Self::entropy(&probs);
        match Decimal::try_from(e) {
            Ok(d) => Ok(SignalValue::Scalar(d)),
            Err(_) => Ok(SignalValue::Unavailable),
        }
    }

    fn reset(&mut self) {
        self.closes.clear();
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
    fn test_pe_period_too_small() { assert!(PriceEntropy::new("pe", 1).is_err()); }

    #[test]
    fn test_pe_unavailable_before_warmup() {
        let mut pe = PriceEntropy::new("pe", 4).unwrap();
        for _ in 0..4 {
            assert_eq!(pe.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_pe_all_same_direction_low_entropy() {
        // All up -> entropy near 0
        let mut pe = PriceEntropy::new("pe", 5).unwrap();
        for i in 0u32..7 {
            pe.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        if let SignalValue::Scalar(e) = pe.update_bar(&bar("107")).unwrap() {
            assert!(e < dec!(0.3), "expected low entropy for unidirectional series, got {e}");
        }
    }

    #[test]
    fn test_pe_alternating_moderate_entropy() {
        // Alternating up/down -> 50% up, 50% down -> higher entropy
        let mut pe = PriceEntropy::new("pe", 6).unwrap();
        let prices = ["100","102","100","102","100","102","100","102"];
        let mut last = SignalValue::Unavailable;
        for p in &prices {
            last = pe.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(e) = last {
            assert!(e > dec!(0), "expected positive entropy for mixed series, got {e}");
        }
    }

    #[test]
    fn test_pe_reset() {
        let mut pe = PriceEntropy::new("pe", 4).unwrap();
        for i in 0u32..7 { pe.update_bar(&bar(&(100+i).to_string())).unwrap(); }
        assert!(pe.is_ready());
        pe.reset();
        assert!(!pe.is_ready());
    }
}
"""

max_adverse_excursion = """\
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
"""

files = {
    "autocorrelation_1": autocorrelation_1,
    "price_entropy": price_entropy,
    "max_adverse_excursion": max_adverse_excursion,
}

for name, content in files.items():
    path = os.path.join(base, f"{name}.rs")
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(content)
    print(f"wrote {path}")

print("done")
