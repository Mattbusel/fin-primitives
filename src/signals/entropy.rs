//! # Module: signals::entropy
//!
//! ## Responsibility
//! Entropy-based signal quality estimators that quantify the information content
//! and complexity of price change sequences.
//!
//! ## Estimators
//! - `ShannonEntropy` — rolling Shannon entropy of discretised price-change bins.
//! - `PermutationEntropy` — Bandt-Pompe permutation entropy; detects ordinal complexity.
//! - `ApproximateEntropy` — measures regularity/predictability of a time series.
//!
//! ## Interpretation
//! - **High entropy** → unpredictable, noise-dominated, hard to trade.
//! - **Low entropy** → structured, patterned, potential opportunity.
//!
//! ## NOT Responsible For
//! - Normalisation to [0, 1] (callers should divide by ln(bins) or ln(m!) as needed)
//! - Signal combination or trading decisions

use crate::error::FinError;
use std::collections::HashMap;

// ─── Shannon Entropy ──────────────────────────────────────────────────────────

/// Rolling Shannon entropy estimator over discretised price changes.
///
/// Maintains a fixed-length sliding window of returns, bins them into
/// `num_bins` equal-width buckets, and computes H = -Σ p·ln(p).
///
/// A value of `None` is returned until the window is full.
#[derive(Debug, Clone)]
pub struct ShannonEntropy {
    window: usize,
    num_bins: usize,
    buffer: Vec<f64>,
}

impl ShannonEntropy {
    /// Create a new Shannon entropy estimator.
    ///
    /// # Errors
    /// Returns `FinError::InvalidPeriod` if `window` or `num_bins` is 0.
    pub fn new(window: usize, num_bins: usize) -> Result<Self, FinError> {
        if window == 0 {
            return Err(FinError::InvalidPeriod(window));
        }
        if num_bins == 0 {
            return Err(FinError::InvalidInput("num_bins must be at least 1".to_owned()));
        }
        Ok(Self { window, num_bins, buffer: Vec::with_capacity(window + 1) })
    }

    /// Push a new price observation. Returns the rolling entropy once the
    /// window is full, or `None` while still warming up.
    pub fn update(&mut self, price: f64) -> Option<f64> {
        self.buffer.push(price);
        if self.buffer.len() > self.window + 1 {
            self.buffer.remove(0);
        }
        if self.buffer.len() < 2 {
            return None;
        }
        // Compute log-returns over the current window
        let returns: Vec<f64> = self
            .buffer
            .windows(2)
            .map(|w| {
                if w[0] != 0.0 { (w[1] / w[0]).ln() } else { 0.0 }
            })
            .collect();

        if returns.len() < self.window {
            return None;
        }

        let min = returns.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = returns.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let range = max - min;

        // All identical returns → entropy = 0
        if range < f64::EPSILON {
            return Some(0.0);
        }

        let bin_width = range / self.num_bins as f64;
        let mut counts = vec![0usize; self.num_bins];
        for &r in &returns {
            let idx = ((r - min) / bin_width).floor() as usize;
            let idx = idx.min(self.num_bins - 1);
            counts[idx] += 1;
        }

        let n = returns.len() as f64;
        let entropy = counts
            .iter()
            .filter(|&&c| c > 0)
            .map(|&c| {
                let p = c as f64 / n;
                -p * p.ln()
            })
            .sum();

        Some(entropy)
    }

    /// Returns the number of observations needed before values are emitted.
    pub fn warmup_period(&self) -> usize {
        self.window + 1
    }
}

// ─── Permutation Entropy ──────────────────────────────────────────────────────

/// Bandt-Pompe permutation entropy estimator.
///
/// For each consecutive window of length `order`, the ordinal pattern (rank
/// permutation) of the values is computed. The probability distribution of
/// patterns gives H_perm = -Σ p·ln(p), normalised by ln(order!).
///
/// Normalised output lies in [0, 1]:
/// - 0 → perfectly ordered (deterministic).
/// - 1 → maximum complexity (all patterns equally probable).
#[derive(Debug, Clone)]
pub struct PermutationEntropy {
    order: usize,
    window: usize,
    buffer: Vec<f64>,
}

impl PermutationEntropy {
    /// Create a new permutation entropy estimator.
    ///
    /// - `order`: embedding dimension (typically 3–7).
    /// - `window`: number of consecutive order-grams to accumulate.
    ///
    /// # Errors
    /// Returns `FinError::InvalidPeriod` if `order < 2` or `window == 0`.
    pub fn new(order: usize, window: usize) -> Result<Self, FinError> {
        if order < 2 {
            return Err(FinError::InvalidInput(
                "Permutation entropy order must be at least 2".to_owned(),
            ));
        }
        if window == 0 {
            return Err(FinError::InvalidPeriod(window));
        }
        Ok(Self { order, window, buffer: Vec::with_capacity(window + order) })
    }

    /// Push a new price observation. Returns normalised permutation entropy [0,1]
    /// once `window + order - 1` observations have been seen.
    pub fn update(&mut self, price: f64) -> Option<f64> {
        self.buffer.push(price);
        let needed = self.window + self.order - 1;
        if self.buffer.len() > needed {
            self.buffer.remove(0);
        }
        if self.buffer.len() < needed {
            return None;
        }

        let mut pattern_counts: HashMap<Vec<usize>, usize> = HashMap::new();
        let total_patterns = self.buffer.len() - self.order + 1;

        for i in 0..total_patterns {
            let slice = &self.buffer[i..i + self.order];
            let mut indexed: Vec<(usize, f64)> =
                slice.iter().copied().enumerate().collect();
            indexed.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
            let pattern: Vec<usize> = indexed.iter().map(|(idx, _)| *idx).collect();
            *pattern_counts.entry(pattern).or_insert(0) += 1;
        }

        let n = total_patterns as f64;
        let raw_entropy: f64 = pattern_counts
            .values()
            .map(|&c| {
                let p = c as f64 / n;
                -p * p.ln()
            })
            .sum();

        // Normalise by ln(order!)
        let max_entropy = ln_factorial(self.order);
        if max_entropy < f64::EPSILON {
            return Some(0.0);
        }
        Some((raw_entropy / max_entropy).clamp(0.0, 1.0))
    }

    /// Returns the minimum number of observations before values are emitted.
    pub fn warmup_period(&self) -> usize {
        self.window + self.order - 1
    }
}

/// Compute ln(n!) efficiently.
fn ln_factorial(n: usize) -> f64 {
    (1..=n).map(|k| (k as f64).ln()).sum()
}

// ─── Approximate Entropy ──────────────────────────────────────────────────────

/// Approximate entropy (ApEn) — measures the regularity of a time series.
///
/// ApEn(m, r, N) counts the frequency of similar runs of length m vs m+1.
/// A lower ApEn indicates a more regular, predictable series.
///
/// This implementation buffers exactly `capacity` observations and recomputes
/// ApEn on each new observation once the buffer is full (O(N²) per call).
///
/// For production use with large N, prefer Sample Entropy (SampEn).
#[derive(Debug, Clone)]
pub struct ApproximateEntropy {
    /// Embedding dimension (run length), typically 1 or 2.
    m: usize,
    /// Tolerance (fraction of std dev of data), typically 0.1–0.25.
    r: f64,
    /// Fixed buffer capacity N.
    capacity: usize,
    buffer: Vec<f64>,
}

impl ApproximateEntropy {
    /// Create a new ApEn estimator.
    ///
    /// - `m`: template length (2 is standard).
    /// - `r`: matching tolerance (absolute, caller should scale by std dev if desired).
    /// - `capacity`: rolling window size N (must be ≥ m + 2).
    ///
    /// # Errors
    /// Returns `FinError::InvalidInput` if parameters are out of range.
    pub fn new(m: usize, r: f64, capacity: usize) -> Result<Self, FinError> {
        if m == 0 {
            return Err(FinError::InvalidInput("m must be at least 1".to_owned()));
        }
        if r <= 0.0 {
            return Err(FinError::InvalidInput("r must be positive".to_owned()));
        }
        if capacity < m + 2 {
            return Err(FinError::InvalidInput(format!(
                "capacity must be at least m+2 = {}",
                m + 2
            )));
        }
        Ok(Self { m, r, capacity, buffer: Vec::with_capacity(capacity) })
    }

    /// Push a new observation. Returns `Some(ApEn)` once the buffer is full.
    pub fn update(&mut self, value: f64) -> Option<f64> {
        if self.buffer.len() == self.capacity {
            self.buffer.remove(0);
        }
        self.buffer.push(value);
        if self.buffer.len() < self.capacity {
            return None;
        }
        Some(self.compute())
    }

    /// Returns the number of observations needed before output is emitted.
    pub fn warmup_period(&self) -> usize {
        self.capacity
    }

    fn compute(&self) -> f64 {
        let phi_m = self.phi(self.m);
        let phi_m1 = self.phi(self.m + 1);
        phi_m - phi_m1
    }

    fn phi(&self, m: usize) -> f64 {
        let n = self.buffer.len();
        if n < m {
            return 0.0;
        }
        let count_sum: f64 = (0..=(n - m))
            .map(|i| {
                let matches = (0..=(n - m))
                    .filter(|&j| self.max_dist(i, j, m) <= self.r)
                    .count();
                (matches as f64 / (n - m + 1) as f64).ln()
            })
            .sum();
        count_sum / (n - m + 1) as f64
    }

    fn max_dist(&self, i: usize, j: usize, m: usize) -> f64 {
        (0..m)
            .map(|k| (self.buffer[i + k] - self.buffer[j + k]).abs())
            .fold(0.0_f64, f64::max)
    }
}

// ─── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shannon_warmup() {
        let mut se = ShannonEntropy::new(5, 8).unwrap();
        for _ in 0..5 {
            assert!(se.update(100.0).is_none());
        }
        // 6th price completes first 5-return window
        let v = se.update(101.0);
        assert!(v.is_some());
    }

    #[test]
    fn test_shannon_constant_series_zero_entropy() {
        let mut se = ShannonEntropy::new(10, 8).unwrap();
        let mut last = None;
        for _ in 0..20 {
            last = se.update(100.0);
        }
        // All returns = 0 → only one bin populated → entropy = 0
        assert_eq!(last.unwrap(), 0.0);
    }

    #[test]
    fn test_shannon_invalid_params() {
        assert!(ShannonEntropy::new(0, 8).is_err());
        assert!(ShannonEntropy::new(5, 0).is_err());
    }

    #[test]
    fn test_permutation_entropy_warmup() {
        let mut pe = PermutationEntropy::new(3, 10).unwrap();
        // needs 10 + 3 - 1 = 12 observations
        for i in 0..11 {
            assert!(pe.update(i as f64).is_none());
        }
        assert!(pe.update(12.0).is_some());
    }

    #[test]
    fn test_permutation_entropy_monotone_low() {
        let mut pe = PermutationEntropy::new(3, 20).unwrap();
        let mut last = None;
        for i in 0..50 {
            last = pe.update(i as f64); // perfectly monotone increasing
        }
        // Only one ordinal pattern — entropy should be 0
        assert!(last.unwrap() < 0.1);
    }

    #[test]
    fn test_approximate_entropy_warmup() {
        let mut ae = ApproximateEntropy::new(2, 0.1, 10).unwrap();
        for i in 0..9 {
            assert!(ae.update(i as f64).is_none());
        }
        assert!(ae.update(10.0).is_some());
    }

    #[test]
    fn test_approximate_entropy_invalid_params() {
        assert!(ApproximateEntropy::new(0, 0.1, 10).is_err());
        assert!(ApproximateEntropy::new(2, -0.1, 10).is_err());
        assert!(ApproximateEntropy::new(2, 0.1, 2).is_err()); // capacity < m+2
    }
}
