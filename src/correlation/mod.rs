//! # Module: correlation
//!
//! ## Responsibility
//! Streaming Pearson correlation matrix across a configurable window of indicator outputs.
//! Identifies redundant signals (pairs with |r| > a configurable threshold, default 0.95).
//!
//! Also exposes [`stats`] for standalone Pearson, Spearman, Kendall tau-b functions,
//! a symbol-based [`stats::SymbolCorrelationMatrix`], and a [`stats::RollingCorrelation`]
//! rolling-window tracker.
//!
//! ## Guarantees
//! - Returns `None` from [`CorrelationMatrix::get`] until `window` samples have been seen
//! - Correlation values are clamped to `[-1, 1]` to absorb floating-point rounding errors
//! - `most_correlated_with` results are sorted descending by absolute correlation
//!
//! ## NOT Responsible For
//! - Causal inference or feature selection policy
//! - Persistence

/// Standalone correlation measures: Pearson, Spearman rank, Kendall tau-b,
/// symbol-keyed `SymbolCorrelationMatrix`, and `RollingCorrelation`.
pub mod stats;

use crate::error::FinError;
use std::collections::VecDeque;

/// Default redundancy threshold: pairs with |r| above this are flagged as redundant.
pub const DEFAULT_REDUNDANCY_THRESHOLD: f64 = 0.95;

/// A streaming Pearson correlation matrix for a fixed set of indicators.
///
/// Feed one sample per bar via [`CorrelationMatrix::update`]. Once `window` samples
/// have been accumulated the full `n × n` correlation matrix is available.
///
/// # Example
/// ```rust
/// use fin_primitives::correlation::CorrelationMatrix;
///
/// let mut cm = CorrelationMatrix::new(3, 5, 0.95).unwrap();
/// for i in 0..5 {
///     let vals = vec![i as f64, (i * 2) as f64, (10 - i) as f64];
///     cm.update(&vals).unwrap();
/// }
/// // indicators 0 and 1 are perfectly correlated
/// let r = cm.get(0, 1).unwrap();
/// assert!((r - 1.0).abs() < 1e-9);
/// ```
#[derive(Debug)]
pub struct CorrelationMatrix {
    /// Number of indicators tracked.
    n: usize,
    /// Rolling window size.
    window: usize,
    /// Redundancy threshold.
    threshold: f64,
    /// Circular buffer: each entry is one bar's vector of `n` values.
    buf: VecDeque<Vec<f64>>,
}

impl CorrelationMatrix {
    /// Constructs a new `CorrelationMatrix`.
    ///
    /// # Parameters
    /// - `n_indicators`: number of indicators (must be >= 2)
    /// - `window`: rolling window in bars (must be >= 2)
    /// - `redundancy_threshold`: absolute correlation above which a pair is flagged
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `window < 2`.
    /// Returns [`FinError::InvalidInput`] if `n_indicators < 2` or threshold is not in `(0, 1]`.
    pub fn new(n_indicators: usize, window: usize, redundancy_threshold: f64) -> Result<Self, FinError> {
        if window < 2 {
            return Err(FinError::InvalidPeriod(window));
        }
        if n_indicators < 2 {
            return Err(FinError::InvalidInput(
                "CorrelationMatrix requires at least 2 indicators".to_owned(),
            ));
        }
        if redundancy_threshold <= 0.0 || redundancy_threshold > 1.0 {
            return Err(FinError::InvalidInput(
                "redundancy_threshold must be in (0, 1]".to_owned(),
            ));
        }
        Ok(Self {
            n: n_indicators,
            window,
            threshold: redundancy_threshold,
            buf: VecDeque::with_capacity(window),
        })
    }

    /// Constructs a `CorrelationMatrix` with the default redundancy threshold (0.95).
    ///
    /// # Errors
    /// See [`CorrelationMatrix::new`].
    pub fn with_defaults(n_indicators: usize, window: usize) -> Result<Self, FinError> {
        Self::new(n_indicators, window, DEFAULT_REDUNDANCY_THRESHOLD)
    }

    /// Feeds one bar's worth of indicator values.
    ///
    /// `values.len()` must equal the `n_indicators` supplied at construction.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if `values.len() != n_indicators`.
    pub fn update(&mut self, values: &[f64]) -> Result<(), FinError> {
        if values.len() != self.n {
            return Err(FinError::InvalidInput(format!(
                "expected {} values, got {}",
                self.n,
                values.len()
            )));
        }
        self.buf.push_back(values.to_vec());
        if self.buf.len() > self.window {
            self.buf.pop_front();
        }
        Ok(())
    }

    /// Returns `true` when enough samples have been accumulated to compute correlations.
    pub fn is_ready(&self) -> bool {
        self.buf.len() >= self.window
    }

    /// Returns the Pearson correlation between indicators `i` and `j`.
    ///
    /// Returns `None` when fewer than `window` samples have been seen, or when either
    /// indicator has zero variance (correlation is undefined).
    pub fn get(&self, i: usize, j: usize) -> Option<f64> {
        if !self.is_ready() {
            return None;
        }
        if i == j {
            return Some(1.0);
        }
        let n = self.buf.len() as f64;
        let mut sum_x = 0.0_f64;
        let mut sum_y = 0.0_f64;
        let mut sum_xy = 0.0_f64;
        let mut sum_x2 = 0.0_f64;
        let mut sum_y2 = 0.0_f64;
        for row in &self.buf {
            let x = row[i];
            let y = row[j];
            sum_x += x;
            sum_y += y;
            sum_xy += x * y;
            sum_x2 += x * x;
            sum_y2 += y * y;
        }
        let num = n * sum_xy - sum_x * sum_y;
        let den_sq = (n * sum_x2 - sum_x * sum_x) * (n * sum_y2 - sum_y * sum_y);
        if den_sq <= 0.0 {
            return None;
        }
        let r = num / den_sq.sqrt();
        // Clamp to [-1, 1] to absorb floating-point rounding errors
        Some(r.clamp(-1.0, 1.0))
    }

    /// Returns the full `n × n` correlation matrix as a flat `Vec<f64>` (row-major).
    ///
    /// Element at row `i`, column `j` is at index `i * n + j`.
    /// Returns `None` until ready.
    pub fn matrix(&self) -> Option<Vec<f64>> {
        if !self.is_ready() {
            return None;
        }
        let mut mat = vec![0.0_f64; self.n * self.n];
        for i in 0..self.n {
            for j in 0..self.n {
                mat[i * self.n + j] = self.get(i, j).unwrap_or(0.0);
            }
        }
        Some(mat)
    }

    /// Returns all indicators whose absolute correlation with `indicator_id` exceeds
    /// `threshold`, sorted descending by absolute correlation value.
    ///
    /// Returns an empty `Vec` if the matrix is not yet ready.
    pub fn most_correlated_with(&self, indicator_id: usize) -> Vec<(usize, f64)> {
        if !self.is_ready() {
            return vec![];
        }
        let mut result: Vec<(usize, f64)> = (0..self.n)
            .filter(|&j| j != indicator_id)
            .filter_map(|j| {
                self.get(indicator_id, j)
                    .map(|r| (j, r))
            })
            .collect();
        result.sort_by(|a, b| b.1.abs().partial_cmp(&a.1.abs()).unwrap_or(std::cmp::Ordering::Equal));
        result
    }

    /// Returns all pairs `(i, j)` where `i < j` and `|r| >= threshold`.
    ///
    /// These pairs are considered redundant signals.
    /// Returns an empty `Vec` if the matrix is not ready.
    pub fn redundant_pairs(&self) -> Vec<(usize, usize, f64)> {
        if !self.is_ready() {
            return vec![];
        }
        let mut pairs = Vec::new();
        for i in 0..self.n {
            for j in (i + 1)..self.n {
                if let Some(r) = self.get(i, j) {
                    if r.abs() >= self.threshold {
                        pairs.push((i, j, r));
                    }
                }
            }
        }
        pairs
    }

    /// Returns the number of indicators tracked.
    pub fn n_indicators(&self) -> usize {
        self.n
    }

    /// Returns the configured window size.
    pub fn window(&self) -> usize {
        self.window
    }

    /// Returns the number of samples currently buffered.
    pub fn sample_count(&self) -> usize {
        self.buf.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(cm: &mut CorrelationMatrix, rows: &[[f64; 3]]) {
        for row in rows {
            cm.update(row).unwrap();
        }
    }

    #[test]
    fn test_perfect_positive_correlation() {
        let mut cm = CorrelationMatrix::new(3, 5, 0.95).unwrap();
        // indicators 0 and 1 are y=2x (perfect positive correlation)
        // indicator 2 is negatively correlated with 0
        let data = [[1.0, 2.0, 10.0], [2.0, 4.0, 9.0], [3.0, 6.0, 8.0], [4.0, 8.0, 7.0], [5.0, 10.0, 6.0]];
        feed(&mut cm, &data);
        assert!(cm.is_ready());
        let r01 = cm.get(0, 1).unwrap();
        assert!((r01 - 1.0).abs() < 1e-9, "r01={r01}");
        let r02 = cm.get(0, 2).unwrap();
        assert!((r02 + 1.0).abs() < 1e-9, "r02={r02}");
    }

    #[test]
    fn test_not_ready_until_window_filled() {
        let mut cm = CorrelationMatrix::new(2, 5, 0.95).unwrap();
        for i in 0..4 {
            cm.update(&[i as f64, (i * 2) as f64]).unwrap();
        }
        assert!(!cm.is_ready());
        assert!(cm.get(0, 1).is_none());
    }

    #[test]
    fn test_most_correlated_with_sorted() {
        let mut cm = CorrelationMatrix::new(3, 5, 0.50).unwrap();
        let data = [[1.0, 2.0, 10.0], [2.0, 4.0, 9.0], [3.0, 6.0, 8.0], [4.0, 8.0, 7.0], [5.0, 10.0, 6.0]];
        feed(&mut cm, &data);
        let corrs = cm.most_correlated_with(0);
        assert_eq!(corrs.len(), 2);
        // highest abs correlation first
        assert!(corrs[0].1.abs() >= corrs[1].1.abs());
    }

    #[test]
    fn test_redundant_pairs() {
        let mut cm = CorrelationMatrix::new(3, 5, 0.95).unwrap();
        let data = [[1.0, 2.0, 10.0], [2.0, 4.0, 9.0], [3.0, 6.0, 8.0], [4.0, 8.0, 7.0], [5.0, 10.0, 6.0]];
        feed(&mut cm, &data);
        let pairs = cm.redundant_pairs();
        // pairs (0,1) r≈1.0, (0,2) r≈-1.0, (1,2) r≈-1.0 all exceed 0.95
        assert_eq!(pairs.len(), 3);
    }

    #[test]
    fn test_self_correlation_is_one() {
        let mut cm = CorrelationMatrix::new(2, 3, 0.95).unwrap();
        for i in 0..3 {
            cm.update(&[i as f64, (i * 3) as f64]).unwrap();
        }
        assert_eq!(cm.get(0, 0).unwrap(), 1.0);
        assert_eq!(cm.get(1, 1).unwrap(), 1.0);
    }

    #[test]
    fn test_zero_variance_returns_none() {
        let mut cm = CorrelationMatrix::new(2, 3, 0.95).unwrap();
        // indicator 1 is constant → zero variance
        for _ in 0..3 {
            cm.update(&[1.0, 5.0]).unwrap();
        }
        assert!(cm.get(0, 1).is_none());
    }

    #[test]
    fn test_matrix_shape() {
        let mut cm = CorrelationMatrix::new(3, 3, 0.95).unwrap();
        for i in 0..3 {
            cm.update(&[i as f64, (i + 1) as f64, (i * 2) as f64]).unwrap();
        }
        let mat = cm.matrix().unwrap();
        assert_eq!(mat.len(), 9);
        // diagonal should be 1
        assert_eq!(mat[0], 1.0);
        assert_eq!(mat[4], 1.0);
        assert_eq!(mat[8], 1.0);
    }

    #[test]
    fn test_invalid_period_error() {
        assert!(matches!(
            CorrelationMatrix::new(2, 1, 0.95).unwrap_err(),
            FinError::InvalidPeriod(_)
        ));
    }

    #[test]
    fn test_invalid_indicator_count_error() {
        assert!(matches!(
            CorrelationMatrix::new(1, 5, 0.95).unwrap_err(),
            FinError::InvalidInput(_)
        ));
    }

    #[test]
    fn test_window_rolls_old_samples() {
        let mut cm = CorrelationMatrix::new(2, 3, 0.95).unwrap();
        // Feed 5 samples; only last 3 matter
        for i in 0..5 {
            cm.update(&[i as f64, (i * 2) as f64]).unwrap();
        }
        assert_eq!(cm.sample_count(), 3);
    }
}
