//! # Module: cross_asset
//!
//! ## Responsibility
//! Rolling cross-asset correlation tracking and dimensionality reduction via
//! Principal Component Analysis (PCA) on the correlation matrix.
//!
//! ## Guarantees
//! - `CrossAssetCorrelation` returns `None` until `window` samples have been seen
//! - Correlation values are clamped to `[-1, 1]` to absorb floating-point rounding
//! - `PrincipalComponents` extracts up to 3 PCs using the power-iteration method;
//!   returns `None` when fewer than 2 assets are tracked or matrix not ready
//! - All arithmetic is in `f64` (sufficient for statistical computation)
//! - Zero panics; validation errors returned as `FinError`
//!
//! ## NOT Responsible For
//! - Causal inference or portfolio optimization
//! - Persistence

use crate::error::FinError;
use std::collections::VecDeque;

// ─────────────────────────────────────────
//  CrossAssetCorrelation
// ─────────────────────────────────────────

/// Tracks rolling Pearson correlations between N instruments.
///
/// Feed one return observation per bar per instrument via [`update`](CrossAssetCorrelation::update).
/// Once `window` samples have been seen, the full NxN [`CorrelationMatrix`] is available.
///
/// # Example
/// ```rust
/// use fin_primitives::cross_asset::CrossAssetCorrelation;
///
/// let mut cac = CrossAssetCorrelation::new(
///     vec!["SPY".into(), "QQQ".into(), "IWM".into()],
///     20,
/// ).unwrap();
///
/// for i in 0..20 {
///     let returns = vec![0.001 * i as f64, 0.0012 * i as f64, 0.0008 * i as f64];
///     cac.update(&returns).unwrap();
/// }
///
/// let matrix = cac.correlation_matrix();
/// assert!(matrix.is_some());
/// ```
#[derive(Debug)]
pub struct CrossAssetCorrelation {
    /// Instrument names in column order.
    names: Vec<String>,
    /// Number of instruments.
    n: usize,
    /// Rolling window size.
    window: usize,
    /// Circular buffer: each entry is one bar's vector of `n` return values.
    buf: VecDeque<Vec<f64>>,
}

impl CrossAssetCorrelation {
    /// Constructs a `CrossAssetCorrelation` tracker.
    ///
    /// # Parameters
    /// - `names`: instrument names (must have >= 2 elements, no duplicates).
    /// - `window`: rolling window in bars (must be >= 2).
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if fewer than 2 instruments are provided or names
    /// contain duplicates.
    /// Returns [`FinError::InvalidPeriod`] if `window < 2`.
    pub fn new(names: Vec<String>, window: usize) -> Result<Self, FinError> {
        if names.len() < 2 {
            return Err(FinError::InvalidInput(
                "CrossAssetCorrelation requires at least 2 instruments".to_owned(),
            ));
        }
        if window < 2 {
            return Err(FinError::InvalidPeriod(window));
        }
        // Check for duplicate names
        for (i, name) in names.iter().enumerate() {
            for (j, other) in names.iter().enumerate() {
                if i != j && name == other {
                    return Err(FinError::InvalidInput(format!(
                        "duplicate instrument name: '{name}'"
                    )));
                }
            }
        }
        let n = names.len();
        Ok(Self { names, n, window, buf: VecDeque::with_capacity(window) })
    }

    /// Returns the instrument names.
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// Returns the number of instruments tracked.
    pub fn n_instruments(&self) -> usize {
        self.n
    }

    /// Returns the rolling window size.
    pub fn window(&self) -> usize {
        self.window
    }

    /// Returns the number of samples currently buffered.
    pub fn sample_count(&self) -> usize {
        self.buf.len()
    }

    /// Returns `true` when enough samples have been collected.
    pub fn is_ready(&self) -> bool {
        self.buf.len() >= self.window
    }

    /// Records one bar's returns for all instruments.
    ///
    /// `returns.len()` must equal `self.n_instruments()`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if the length is wrong or any value is non-finite.
    pub fn update(&mut self, returns: &[f64]) -> Result<(), FinError> {
        if returns.len() != self.n {
            return Err(FinError::InvalidInput(format!(
                "expected {} returns, got {}",
                self.n,
                returns.len()
            )));
        }
        for (i, r) in returns.iter().enumerate() {
            if !r.is_finite() {
                return Err(FinError::InvalidInput(format!(
                    "return[{i}] is not finite: {r}"
                )));
            }
        }
        self.buf.push_back(returns.to_vec());
        if self.buf.len() > self.window {
            self.buf.pop_front();
        }
        Ok(())
    }

    /// Returns the Pearson correlation between instruments `i` and `j`.
    ///
    /// Returns `None` when fewer than `window` samples have been seen, indices are
    /// out of bounds, or either instrument has zero variance.
    pub fn get(&self, i: usize, j: usize) -> Option<f64> {
        if !self.is_ready() || i >= self.n || j >= self.n {
            return None;
        }
        if i == j {
            return Some(1.0);
        }
        let n = self.buf.len() as f64;
        let mut sx = 0.0_f64;
        let mut sy = 0.0_f64;
        let mut sxy = 0.0_f64;
        let mut sx2 = 0.0_f64;
        let mut sy2 = 0.0_f64;
        for row in &self.buf {
            let x = row[i];
            let y = row[j];
            sx += x;
            sy += y;
            sxy += x * y;
            sx2 += x * x;
            sy2 += y * y;
        }
        let num = n * sxy - sx * sy;
        let den_sq = (n * sx2 - sx * sx) * (n * sy2 - sy * sy);
        if den_sq <= 0.0 {
            return None;
        }
        Some((num / den_sq.sqrt()).clamp(-1.0, 1.0))
    }

    /// Returns the index of an instrument by name, or `None` if not found.
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|n| n == name)
    }

    /// Returns the full NxN correlation matrix (row-major `Vec<f64>`).
    ///
    /// Element at `(i, j)` is at index `i * n + j`.
    /// Returns `None` until ready.
    pub fn correlation_matrix(&self) -> Option<CorrelationMatrix> {
        if !self.is_ready() {
            return None;
        }
        let mut mat = vec![0.0_f64; self.n * self.n];
        for i in 0..self.n {
            for j in 0..self.n {
                mat[i * self.n + j] = self.get(i, j).unwrap_or(0.0);
            }
        }
        Some(CorrelationMatrix {
            n: self.n,
            data: mat,
            names: self.names.clone(),
        })
    }

    /// Resets the tracker.
    pub fn reset(&mut self) {
        self.buf.clear();
    }
}

// ─────────────────────────────────────────
//  CorrelationMatrix
// ─────────────────────────────────────────

/// An NxN symmetric correlation matrix (row-major, `f64`).
///
/// Produced by [`CrossAssetCorrelation::correlation_matrix`].
#[derive(Debug, Clone)]
pub struct CorrelationMatrix {
    /// Dimension.
    n: usize,
    /// Row-major data, length `n * n`.
    data: Vec<f64>,
    /// Names of the instruments (columns = rows).
    names: Vec<String>,
}

impl CorrelationMatrix {
    /// Returns the correlation between instruments `i` and `j`, or `None` if out of bounds.
    pub fn get(&self, i: usize, j: usize) -> Option<f64> {
        if i >= self.n || j >= self.n {
            return None;
        }
        Some(self.data[i * self.n + j])
    }

    /// Returns the raw row-major data slice.
    pub fn data(&self) -> &[f64] {
        &self.data
    }

    /// Returns the matrix dimension N.
    pub fn n(&self) -> usize {
        self.n
    }

    /// Returns the instrument names.
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// Returns the most correlated instrument indices for instrument `i`,
    /// sorted descending by absolute correlation.
    pub fn most_correlated_with(&self, i: usize) -> Vec<(usize, f64)> {
        if i >= self.n {
            return vec![];
        }
        let mut pairs: Vec<(usize, f64)> = (0..self.n)
            .filter(|&j| j != i)
            .filter_map(|j| self.get(i, j).map(|r| (j, r)))
            .collect();
        pairs.sort_by(|a, b| {
            b.1.abs()
                .partial_cmp(&a.1.abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        pairs
    }
}

// ─────────────────────────────────────────
//  PrincipalComponents
// ─────────────────────────────────────────

/// Principal Component Analysis on a correlation matrix.
///
/// Extracts the first `k` (up to 3) principal components via the power-iteration
/// method (deflation). Useful for dimensionality reduction and regime identification.
///
/// # Example
/// ```rust
/// use fin_primitives::cross_asset::{CrossAssetCorrelation, PrincipalComponents};
///
/// let mut cac = CrossAssetCorrelation::new(
///     vec!["A".into(), "B".into(), "C".into()],
///     10,
/// ).unwrap();
/// for i in 0..10 {
///     cac.update(&[i as f64 * 0.01, i as f64 * 0.012, i as f64 * 0.009]).unwrap();
/// }
/// if let Some(mat) = cac.correlation_matrix() {
///     let pca = PrincipalComponents::from_matrix(&mat, 2).unwrap();
///     assert!(pca.explained_variance_ratio()[0] >= 0.0);
/// }
/// ```
#[derive(Debug, Clone)]
pub struct PrincipalComponents {
    /// Number of PCs extracted.
    k: usize,
    /// Principal component vectors, one per PC, length n each.
    components: Vec<Vec<f64>>,
    /// Eigenvalues (variance explained by each PC).
    eigenvalues: Vec<f64>,
    /// Total variance (sum of all diagonal entries of the correlation matrix = n).
    total_variance: f64,
}

impl PrincipalComponents {
    /// Extracts up to `k` principal components from a [`CorrelationMatrix`].
    ///
    /// Uses power iteration with deflation. `k` is clamped to `min(k, n - 1, 3)`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if `n < 2` or `k == 0`.
    pub fn from_matrix(matrix: &CorrelationMatrix, k: usize) -> Result<Self, FinError> {
        let n = matrix.n();
        if n < 2 {
            return Err(FinError::InvalidInput(
                "PCA requires at least 2 instruments".to_owned(),
            ));
        }
        if k == 0 {
            return Err(FinError::InvalidInput("k must be at least 1".to_owned()));
        }
        let k_actual = k.min(n - 1).min(3);
        let total_variance = n as f64; // trace of correlation matrix = n

        // Work on a mutable copy (for deflation)
        let mut work: Vec<f64> = matrix.data().to_vec();

        let mut components = Vec::with_capacity(k_actual);
        let mut eigenvalues = Vec::with_capacity(k_actual);

        for _ in 0..k_actual {
            let (eigval, eigvec) = power_iterate(&work, n, 200, 1e-8);
            if eigval <= 0.0 {
                break;
            }
            // Deflate: A = A - λ * v * v^T
            for i in 0..n {
                for j in 0..n {
                    work[i * n + j] -= eigval * eigvec[i] * eigvec[j];
                }
            }
            eigenvalues.push(eigval);
            components.push(eigvec);
        }

        Ok(Self { k: components.len(), components, eigenvalues, total_variance })
    }

    /// Returns the number of PCs extracted.
    pub fn k(&self) -> usize {
        self.k
    }

    /// Returns the PC vectors (each of length N instruments).
    pub fn components(&self) -> &[Vec<f64>] {
        &self.components
    }

    /// Returns the eigenvalues (variance explained by each PC).
    pub fn eigenvalues(&self) -> &[f64] {
        &self.eigenvalues
    }

    /// Returns the proportion of variance explained by each PC (`eigenvalue / total_variance`).
    pub fn explained_variance_ratio(&self) -> Vec<f64> {
        if self.total_variance <= 0.0 {
            return vec![0.0; self.k];
        }
        self.eigenvalues.iter().map(|e| e / self.total_variance).collect()
    }

    /// Projects a returns vector (length N) onto the first `m` PCs.
    ///
    /// Returns a `Vec<f64>` of length `min(m, self.k)`.
    pub fn project(&self, returns: &[f64], m: usize) -> Vec<f64> {
        let take = m.min(self.k);
        self.components[..take]
            .iter()
            .map(|pc| pc.iter().zip(returns.iter()).map(|(a, b)| a * b).sum())
            .collect()
    }
}

/// Power-iteration algorithm to find the dominant eigenvector and eigenvalue.
///
/// Returns `(eigenvalue, eigenvector)`. The eigenvector is L2-normalized.
/// If the matrix is zero or degenerate, returns `(0.0, vec![0; n])`.
fn power_iterate(matrix: &[f64], n: usize, max_iter: usize, tol: f64) -> (f64, Vec<f64>) {
    // Initialize with ones (uniform start)
    let mut v: Vec<f64> = vec![1.0 / (n as f64).sqrt(); n];

    for _ in 0..max_iter {
        // w = A * v
        let mut w = vec![0.0_f64; n];
        for i in 0..n {
            for j in 0..n {
                w[i] += matrix[i * n + j] * v[j];
            }
        }
        // Compute eigenvalue estimate (Rayleigh quotient numerator)
        let norm: f64 = w.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm == 0.0 {
            return (0.0, vec![0.0; n]);
        }
        let new_v: Vec<f64> = w.iter().map(|x| x / norm).collect();
        // Check convergence
        let diff: f64 = v.iter().zip(&new_v).map(|(a, b)| (a - b).powi(2)).sum::<f64>().sqrt();
        v = new_v;
        if diff < tol {
            break;
        }
    }

    // Eigenvalue = v^T A v
    let mut eigenvalue = 0.0_f64;
    for i in 0..n {
        let mut av_i = 0.0_f64;
        for j in 0..n {
            av_i += matrix[i * n + j] * v[j];
        }
        eigenvalue += v[i] * av_i;
    }

    (eigenvalue, v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cac(n: usize, window: usize) -> CrossAssetCorrelation {
        let names: Vec<String> = (0..n).map(|i| format!("asset{i}")).collect();
        CrossAssetCorrelation::new(names, window).unwrap()
    }

    // ── CrossAssetCorrelation ─────────────────────────────────────────────

    #[test]
    fn test_too_few_assets_fails() {
        assert!(CrossAssetCorrelation::new(vec!["A".into()], 10).is_err());
    }

    #[test]
    fn test_duplicate_name_fails() {
        assert!(CrossAssetCorrelation::new(
            vec!["A".into(), "A".into()],
            10
        )
        .is_err());
    }

    #[test]
    fn test_window_1_fails() {
        assert!(CrossAssetCorrelation::new(vec!["A".into(), "B".into()], 1).is_err());
    }

    #[test]
    fn test_not_ready_before_window() {
        let mut cac = make_cac(2, 5);
        cac.update(&[0.01, 0.02]).unwrap();
        assert!(!cac.is_ready());
        assert!(cac.correlation_matrix().is_none());
    }

    #[test]
    fn test_perfect_correlation_detected() {
        let mut cac = make_cac(2, 5);
        for i in 1..=5 {
            // asset0 and asset1 are identical → r = 1.0
            cac.update(&[i as f64 * 0.01, i as f64 * 0.01]).unwrap();
        }
        let r = cac.get(0, 1).unwrap();
        assert!((r - 1.0).abs() < 1e-9, "r={r}");
    }

    #[test]
    fn test_self_correlation_is_one() {
        let mut cac = make_cac(2, 5);
        for i in 1..=5 {
            cac.update(&[i as f64 * 0.01, i as f64 * 0.02]).unwrap();
        }
        let r = cac.get(0, 0).unwrap();
        assert_eq!(r, 1.0);
    }

    #[test]
    fn test_correlation_matrix_shape() {
        let mut cac = make_cac(3, 4);
        for i in 1..=4 {
            cac.update(&[i as f64, i as f64 * 2.0, i as f64 * 0.5]).unwrap();
        }
        let mat = cac.correlation_matrix().unwrap();
        assert_eq!(mat.data().len(), 9);
        assert_eq!(mat.n(), 3);
        // Diagonal should be 1.0
        assert!((mat.get(0, 0).unwrap() - 1.0).abs() < 1e-9);
        assert!((mat.get(1, 1).unwrap() - 1.0).abs() < 1e-9);
        assert!((mat.get(2, 2).unwrap() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_wrong_length_update_fails() {
        let mut cac = make_cac(3, 5);
        assert!(cac.update(&[0.01, 0.02]).is_err());
    }

    #[test]
    fn test_nan_update_fails() {
        let mut cac = make_cac(2, 5);
        assert!(cac.update(&[f64::NAN, 0.01]).is_err());
    }

    #[test]
    fn test_index_of() {
        let cac = CrossAssetCorrelation::new(
            vec!["SPY".into(), "QQQ".into()],
            5,
        )
        .unwrap();
        assert_eq!(cac.index_of("SPY"), Some(0));
        assert_eq!(cac.index_of("QQQ"), Some(1));
        assert_eq!(cac.index_of("MISSING"), None);
    }

    #[test]
    fn test_reset_clears_buffer() {
        let mut cac = make_cac(2, 3);
        for i in 0..3 {
            cac.update(&[i as f64, i as f64 * 2.0]).unwrap();
        }
        assert!(cac.is_ready());
        cac.reset();
        assert!(!cac.is_ready());
        assert_eq!(cac.sample_count(), 0);
    }

    #[test]
    fn test_most_correlated_with_sorted() {
        let mut cac = make_cac(3, 5);
        for i in 1..=5 {
            let v = i as f64;
            cac.update(&[v, v * 2.0, -v]).unwrap();
        }
        let mat = cac.correlation_matrix().unwrap();
        let corrs = mat.most_correlated_with(0);
        assert_eq!(corrs.len(), 2);
        assert!(corrs[0].1.abs() >= corrs[1].1.abs());
    }

    // ── PrincipalComponents ───────────────────────────────────────────────

    #[test]
    fn test_pca_explained_variance_sums_to_at_most_one() {
        let mut cac = make_cac(3, 10);
        for i in 1..=10 {
            let v = i as f64;
            cac.update(&[v, v * 1.1, v * 0.9]).unwrap();
        }
        let mat = cac.correlation_matrix().unwrap();
        let pca = PrincipalComponents::from_matrix(&mat, 3).unwrap();
        let total: f64 = pca.explained_variance_ratio().iter().sum();
        assert!(total <= 1.0 + 1e-9, "total explained variance ratio={total}");
        assert!(total >= 0.0);
    }

    #[test]
    fn test_pca_k_zero_fails() {
        let mut cac = make_cac(2, 5);
        for i in 1..=5 {
            cac.update(&[i as f64, i as f64 * 2.0]).unwrap();
        }
        let mat = cac.correlation_matrix().unwrap();
        assert!(PrincipalComponents::from_matrix(&mat, 0).is_err());
    }

    #[test]
    fn test_pca_project_length() {
        let mut cac = make_cac(3, 10);
        for i in 1..=10 {
            let v = i as f64;
            cac.update(&[v, v * 1.5, -v]).unwrap();
        }
        let mat = cac.correlation_matrix().unwrap();
        let pca = PrincipalComponents::from_matrix(&mat, 2).unwrap();
        let proj = pca.project(&[0.01, 0.02, -0.01], 2);
        assert!(proj.len() <= 2);
    }

    #[test]
    fn test_pca_first_eigenvalue_largest() {
        let mut cac = make_cac(3, 15);
        for i in 1..=15 {
            let v = i as f64;
            cac.update(&[v, v * 1.2, v * 0.8]).unwrap();
        }
        let mat = cac.correlation_matrix().unwrap();
        let pca = PrincipalComponents::from_matrix(&mat, 3).unwrap();
        let evs = pca.eigenvalues();
        if evs.len() >= 2 {
            assert!(evs[0] >= evs[1], "first eigenvalue should be largest");
        }
    }
}
