//! # Module: volatility
//!
//! ## Responsibility
//! Realised volatility estimators using OHLCV data.
//!
//! ## Estimators
//! | Estimator | Data needed | Efficiency gain vs Close-to-Close |
//! |-----------|-------------|-----------------------------------|
//! | `CloseToClose` | Close only | 1× (baseline) |
//! | `Parkinson` | High, Low | ~5× |
//! | `GarmanKlass` | Open, High, Low, Close | ~7-8× |
//! | `RogersSatchell` | Open, High, Low, Close | ~8× (drift-adjusted) |
//! | `YangZhang` | Open, High, Low, Close + previous close | ~15× (gap-robust) |
//!
//! All estimators operate in rolling-window mode: push bars one at a time,
//! receive `Some(annualised_vol)` once the window is full.
//!
//! ## NOT Responsible For
//! - Forward volatility (implied vol is in `options`)
//! - GARCH/ARCH models

use crate::error::FinError;

/// A single OHLCV bar input for volatility estimators.
#[derive(Debug, Clone, Copy)]
pub struct OhlcBar {
    /// Opening price of the bar.
    pub open: f64,
    /// Highest price during the bar.
    pub high: f64,
    /// Lowest price during the bar.
    pub low: f64,
    /// Closing price of the bar.
    pub close: f64,
}

// ─── helper: rolling window statistics ───────────────────────────────────────

fn sample_variance(values: &[f64]) -> f64 {
    let n = values.len() as f64;
    if n < 2.0 {
        return 0.0;
    }
    let mean = values.iter().sum::<f64>() / n;
    values.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0)
}

fn annualise(variance_per_bar: f64, bars_per_year: f64) -> f64 {
    (variance_per_bar * bars_per_year).sqrt()
}

// ─── Close-to-Close ───────────────────────────────────────────────────────────

/// Standard close-to-close realised volatility estimator.
///
/// Uses log-returns: r_t = ln(C_t / C_{t-1}).
/// σ_ann = std(r) × √(bars_per_year).
#[derive(Debug, Clone)]
pub struct CloseToClose {
    window: usize,
    bars_per_year: f64,
    closes: Vec<f64>,
}

impl CloseToClose {
    /// Create a new close-to-close estimator.
    ///
    /// - `window`: number of bars in the rolling window.
    /// - `bars_per_year`: annualisation factor (252 for daily, 252×6.5×60 for minute bars).
    ///
    /// # Errors
    /// `FinError::InvalidPeriod` if `window == 0`.
    pub fn new(window: usize, bars_per_year: f64) -> Result<Self, FinError> {
        if window == 0 {
            return Err(FinError::InvalidPeriod(window));
        }
        Ok(Self { window, bars_per_year, closes: Vec::with_capacity(window + 1) })
    }

    /// Push the latest close price. Returns annualised volatility once the window is full.
    pub fn update(&mut self, close: f64) -> Option<f64> {
        self.closes.push(close);
        if self.closes.len() > self.window + 1 {
            self.closes.remove(0);
        }
        if self.closes.len() <= self.window {
            return None;
        }
        let returns: Vec<f64> = self
            .closes
            .windows(2)
            .map(|w| (w[1] / w[0]).ln())
            .collect();
        let var = sample_variance(&returns);
        Some(annualise(var, self.bars_per_year))
    }

    /// Minimum observations before output is emitted.
    pub fn warmup_period(&self) -> usize {
        self.window + 1
    }
}

// ─── Parkinson ────────────────────────────────────────────────────────────────

/// Parkinson high-low realised volatility estimator.
///
/// Uses only the high and low prices; ~5× more efficient than close-to-close
/// but assumes no overnight gaps and zero drift.
///
/// σ²_park = [1/(4n·ln2)] × Σ (ln(H_i/L_i))²
#[derive(Debug, Clone)]
pub struct Parkinson {
    window: usize,
    bars_per_year: f64,
    hl_sq: Vec<f64>,
}

impl Parkinson {
    /// Create a new Parkinson estimator.
    ///
    /// # Errors
    /// `FinError::InvalidPeriod` if `window == 0`.
    pub fn new(window: usize, bars_per_year: f64) -> Result<Self, FinError> {
        if window == 0 {
            return Err(FinError::InvalidPeriod(window));
        }
        Ok(Self { window, bars_per_year, hl_sq: Vec::with_capacity(window) })
    }

    /// Push an OHLC bar. Returns annualised volatility once the window is full.
    pub fn update(&mut self, bar: OhlcBar) -> Option<f64> {
        if bar.high <= 0.0 || bar.low <= 0.0 || bar.high < bar.low {
            return None;
        }
        let hl = (bar.high / bar.low).ln();
        self.hl_sq.push(hl * hl);
        if self.hl_sq.len() > self.window {
            self.hl_sq.remove(0);
        }
        if self.hl_sq.len() < self.window {
            return None;
        }
        let n = self.window as f64;
        let var_per_bar = self.hl_sq.iter().sum::<f64>() / (4.0 * n * 2.0_f64.ln());
        Some(annualise(var_per_bar, self.bars_per_year))
    }

    /// Minimum bars before output is emitted.
    pub fn warmup_period(&self) -> usize {
        self.window
    }
}

// ─── Garman-Klass ─────────────────────────────────────────────────────────────

/// Garman-Klass OHLC realised volatility estimator.
///
/// Uses open, high, low, and close; ~7-8× more efficient than close-to-close.
///
/// σ²_gk = (1/n) × Σ [ 0.5·(ln H/L)² − (2ln2−1)·(ln C/O)² ]
#[derive(Debug, Clone)]
pub struct GarmanKlass {
    window: usize,
    bars_per_year: f64,
    terms: Vec<f64>,
}

impl GarmanKlass {
    /// Create a new Garman-Klass estimator.
    ///
    /// # Errors
    /// `FinError::InvalidPeriod` if `window == 0`.
    pub fn new(window: usize, bars_per_year: f64) -> Result<Self, FinError> {
        if window == 0 {
            return Err(FinError::InvalidPeriod(window));
        }
        Ok(Self { window, bars_per_year, terms: Vec::with_capacity(window) })
    }

    /// Push an OHLC bar. Returns annualised volatility once the window is full.
    pub fn update(&mut self, bar: OhlcBar) -> Option<f64> {
        if bar.open <= 0.0 || bar.high <= 0.0 || bar.low <= 0.0 || bar.close <= 0.0 {
            return None;
        }
        let hl = (bar.high / bar.low).ln();
        let co = (bar.close / bar.open).ln();
        let term = 0.5 * hl * hl - (2.0 * 2.0_f64.ln() - 1.0) * co * co;
        self.terms.push(term);
        if self.terms.len() > self.window {
            self.terms.remove(0);
        }
        if self.terms.len() < self.window {
            return None;
        }
        let var_per_bar = self.terms.iter().sum::<f64>() / self.window as f64;
        let var_per_bar = var_per_bar.max(0.0);
        Some(annualise(var_per_bar, self.bars_per_year))
    }

    /// Minimum bars before output is emitted.
    pub fn warmup_period(&self) -> usize {
        self.window
    }
}

// ─── Rogers-Satchell ──────────────────────────────────────────────────────────

/// Rogers-Satchell OHLC realised volatility estimator.
///
/// Handles non-zero drift unlike Parkinson or Garman-Klass.
///
/// σ²_rs = (1/n) × Σ [ ln(H/C)·ln(H/O) + ln(L/C)·ln(L/O) ]
#[derive(Debug, Clone)]
pub struct RogersSatchell {
    window: usize,
    bars_per_year: f64,
    terms: Vec<f64>,
}

impl RogersSatchell {
    /// Create a new Rogers-Satchell estimator.
    ///
    /// # Errors
    /// `FinError::InvalidPeriod` if `window == 0`.
    pub fn new(window: usize, bars_per_year: f64) -> Result<Self, FinError> {
        if window == 0 {
            return Err(FinError::InvalidPeriod(window));
        }
        Ok(Self { window, bars_per_year, terms: Vec::with_capacity(window) })
    }

    /// Push an OHLC bar. Returns annualised volatility once the window is full.
    pub fn update(&mut self, bar: OhlcBar) -> Option<f64> {
        if bar.open <= 0.0 || bar.high <= 0.0 || bar.low <= 0.0 || bar.close <= 0.0 {
            return None;
        }
        let hc = (bar.high / bar.close).ln();
        let ho = (bar.high / bar.open).ln();
        let lc = (bar.low / bar.close).ln();
        let lo = (bar.low / bar.open).ln();
        let term = hc * ho + lc * lo;
        self.terms.push(term);
        if self.terms.len() > self.window {
            self.terms.remove(0);
        }
        if self.terms.len() < self.window {
            return None;
        }
        let var_per_bar = (self.terms.iter().sum::<f64>() / self.window as f64).max(0.0);
        Some(annualise(var_per_bar, self.bars_per_year))
    }

    /// Minimum bars before output is emitted.
    pub fn warmup_period(&self) -> usize {
        self.window
    }
}

// ─── Yang-Zhang ───────────────────────────────────────────────────────────────

/// Yang-Zhang OHLC realised volatility estimator.
///
/// Handles overnight gaps (open ≠ previous close) and drift.
/// ~15× more efficient than close-to-close.
///
/// σ²_yz = σ²_overnight + k·σ²_open + (1-k)·σ²_rs
/// where k = 0.34 / (1.34 + (n+1)/(n-1)).
#[derive(Debug, Clone)]
pub struct YangZhang {
    window: usize,
    bars_per_year: f64,
    /// (prev_close, open, high, low, close) tuples
    bars: Vec<OhlcBar>,
    prev_close: Option<f64>,
}

impl YangZhang {
    /// Create a new Yang-Zhang estimator.
    ///
    /// # Errors
    /// `FinError::InvalidPeriod` if `window < 2`.
    pub fn new(window: usize, bars_per_year: f64) -> Result<Self, FinError> {
        if window < 2 {
            return Err(FinError::InvalidInput(
                "YangZhang window must be at least 2".to_owned(),
            ));
        }
        Ok(Self { window, bars_per_year, bars: Vec::with_capacity(window), prev_close: None })
    }

    /// Push an OHLC bar. Returns annualised volatility once the window is full.
    pub fn update(&mut self, bar: OhlcBar) -> Option<f64> {
        if bar.open <= 0.0 || bar.high <= 0.0 || bar.low <= 0.0 || bar.close <= 0.0 {
            return None;
        }
        self.prev_close = Some(bar.close);
        self.bars.push(bar);
        if self.bars.len() > self.window {
            self.bars.remove(0);
        }
        if self.bars.len() < self.window {
            return None;
        }

        let n = self.window as f64;

        // Build sequences: need prev close for each bar except the first
        // We compute overnight (open/prev_close), open (close/open) and RS terms
        let mut overnight_sq = Vec::with_capacity(self.window - 1);
        let mut open_sq = Vec::with_capacity(self.window);
        let mut rs_terms = Vec::with_capacity(self.window);

        for i in 0..self.bars.len() {
            let b = &self.bars[i];
            // Rogers-Satchell per bar
            let hc = (b.high / b.close).ln();
            let ho = (b.high / b.open).ln();
            let lc = (b.low / b.close).ln();
            let lo = (b.low / b.open).ln();
            rs_terms.push(hc * ho + lc * lo);

            // Open-to-close
            open_sq.push((b.close / b.open).ln());

            // Overnight gap (requires previous bar)
            if i > 0 {
                let prev = &self.bars[i - 1];
                overnight_sq.push((b.open / prev.close).ln());
            }
        }

        let mean_overnight = overnight_sq.iter().sum::<f64>() / overnight_sq.len() as f64;
        let var_overnight = overnight_sq
            .iter()
            .map(|x| (x - mean_overnight).powi(2))
            .sum::<f64>()
            / (overnight_sq.len() as f64 - 1.0).max(1.0);

        let mean_open = open_sq.iter().sum::<f64>() / n;
        let var_open = open_sq
            .iter()
            .map(|x| (x - mean_open).powi(2))
            .sum::<f64>()
            / (n - 1.0).max(1.0);

        let var_rs = (rs_terms.iter().sum::<f64>() / n).max(0.0);

        let k = 0.34 / (1.34 + (n + 1.0) / (n - 1.0));
        let var_yz = (var_overnight + k * var_open + (1.0 - k) * var_rs).max(0.0);

        Some(annualise(var_yz, self.bars_per_year))
    }

    /// Minimum bars before output is emitted.
    pub fn warmup_period(&self) -> usize {
        self.window
    }
}

// ─── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn rising_bar(base: f64) -> OhlcBar {
        OhlcBar { open: base, high: base * 1.01, low: base * 0.99, close: base * 1.005 }
    }

    #[test]
    fn test_close_to_close_warmup() {
        let mut ctc = CloseToClose::new(5, 252.0).unwrap();
        for i in 0..5 {
            assert!(ctc.update(100.0 + i as f64).is_none());
        }
        assert!(ctc.update(106.0).is_some());
    }

    #[test]
    fn test_close_to_close_positive_vol() {
        let mut ctc = CloseToClose::new(10, 252.0).unwrap();
        let mut vol = None;
        for i in 0..20 {
            vol = ctc.update(100.0 + (i as f64).sin() * 2.0);
        }
        assert!(vol.unwrap() > 0.0);
    }

    #[test]
    fn test_parkinson_warmup_and_positive() {
        let mut pk = Parkinson::new(5, 252.0).unwrap();
        for i in 0..4 {
            assert!(pk.update(rising_bar(100.0 + i as f64)).is_none());
        }
        let v = pk.update(rising_bar(105.0));
        assert!(v.is_some());
        assert!(v.unwrap() > 0.0);
    }

    #[test]
    fn test_garman_klass_positive() {
        let mut gk = GarmanKlass::new(10, 252.0).unwrap();
        let mut vol = None;
        for i in 0..10 {
            vol = gk.update(rising_bar(100.0 + i as f64));
        }
        assert!(vol.is_some());
    }

    #[test]
    fn test_rogers_satchell_positive() {
        let mut rs = RogersSatchell::new(10, 252.0).unwrap();
        let mut vol = None;
        for i in 0..10 {
            vol = rs.update(rising_bar(100.0 + i as f64));
        }
        assert!(vol.is_some());
    }

    #[test]
    fn test_yang_zhang_warmup() {
        let mut yz = YangZhang::new(5, 252.0).unwrap();
        for i in 0..4 {
            assert!(yz.update(rising_bar(100.0 + i as f64)).is_none());
        }
        assert!(yz.update(rising_bar(105.0)).is_some());
    }

    #[test]
    fn test_yang_zhang_invalid_window() {
        assert!(YangZhang::new(1, 252.0).is_err());
    }

    #[test]
    fn test_invalid_period() {
        assert!(CloseToClose::new(0, 252.0).is_err());
        assert!(Parkinson::new(0, 252.0).is_err());
        assert!(GarmanKlass::new(0, 252.0).is_err());
        assert!(RogersSatchell::new(0, 252.0).is_err());
    }
}
