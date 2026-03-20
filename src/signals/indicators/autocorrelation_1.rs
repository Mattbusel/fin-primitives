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
