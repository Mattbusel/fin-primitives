//! Correlation matrix estimation and shrinkage.
//!
//! Provides:
//! - [`CorrelationMatrix`]: pairwise Pearson correlation from return series
//! - [`LedoitWolfShrinkage`]: oracle approximating shrinkage toward the identity
//! - [`DccGarch`]: simplified DCC-GARCH rolling and EWMA correlation utilities

/// Symmetric correlation matrix.
#[derive(Debug, Clone)]
pub struct CorrelationMatrix {
    /// Row-major correlation values; element `[i * n + j]` is `corr(i, j)`.
    pub matrix: Vec<Vec<f64>>,
    /// Dimension (number of assets).
    pub n: usize,
}

impl CorrelationMatrix {
    /// Compute pairwise Pearson correlation from a matrix of return series.
    ///
    /// `returns[i]` is the time-series of returns for asset `i`.
    /// All series must have the same length.
    pub fn from_returns(returns: &[Vec<f64>]) -> Self {
        let n = returns.len();
        if n == 0 {
            return Self { matrix: vec![], n: 0 };
        }

        let t = returns[0].len();

        // Compute means
        let means: Vec<f64> = returns
            .iter()
            .map(|r| r.iter().sum::<f64>() / t.max(1) as f64)
            .collect();

        // Compute standard deviations
        let stds: Vec<f64> = returns
            .iter()
            .zip(means.iter())
            .map(|(r, &m)| {
                let var = r.iter().map(|&x| (x - m).powi(2)).sum::<f64>() / t.max(1) as f64;
                var.sqrt()
            })
            .collect();

        let mut matrix = vec![vec![0.0_f64; n]; n];
        for i in 0..n {
            matrix[i][i] = 1.0;
            for j in (i + 1)..n {
                if stds[i] < 1e-12 || stds[j] < 1e-12 {
                    matrix[i][j] = 0.0;
                    matrix[j][i] = 0.0;
                    continue;
                }
                let cov: f64 = returns[i]
                    .iter()
                    .zip(returns[j].iter())
                    .map(|(&xi, &xj)| (xi - means[i]) * (xj - means[j]))
                    .sum::<f64>()
                    / t.max(1) as f64;
                let corr = cov / (stds[i] * stds[j]);
                matrix[i][j] = corr;
                matrix[j][i] = corr;
            }
        }

        Self { matrix, n }
    }

    /// Get the correlation between assets `i` and `j`.
    pub fn get(&self, i: usize, j: usize) -> f64 {
        self.matrix[i][j]
    }

    /// Test positive definiteness using Sylvester's criterion.
    ///
    /// Checks that all leading principal minors are strictly positive.
    /// Uses Gaussian elimination to compute determinants.
    pub fn is_positive_definite(&self) -> bool {
        let n = self.n;
        if n == 0 {
            return false;
        }
        // Check each leading minor
        for k in 1..=n {
            // Extract the k×k leading submatrix
            let det = leading_minor_det(&self.matrix, k);
            if det <= 0.0 {
                return false;
            }
        }
        true
    }

    /// Approximate the three largest eigenvalues via power iteration with deflation.
    ///
    /// Returns up to three eigenvalues (fewer if the matrix is smaller).
    pub fn eigenvalues_approx(&self) -> Vec<f64> {
        let n = self.n;
        let max_k = 3.min(n);
        let mut eigenvalues = Vec::with_capacity(max_k);
        // Work on a copy of the matrix for deflation
        let mut a = self.matrix.clone();

        for _ in 0..max_k {
            // Power iteration
            let mut v = vec![1.0_f64; n];
            let mut lambda = 0.0_f64;
            for _ in 0..200 {
                let av = mat_vec_mul(&a, &v);
                let norm = vec_norm(&av);
                if norm < 1e-12 {
                    break;
                }
                let new_v: Vec<f64> = av.iter().map(|&x| x / norm).collect();
                // Rayleigh quotient
                let av2 = mat_vec_mul(&a, &new_v);
                lambda = new_v.iter().zip(av2.iter()).map(|(&vi, &avi)| vi * avi).sum();
                v = new_v;
            }
            eigenvalues.push(lambda);
            // Deflation: A ← A - lambda * v * v^T
            for i in 0..n {
                for j in 0..n {
                    a[i][j] -= lambda * v[i] * v[j];
                }
            }
        }

        eigenvalues
    }

    /// Approximate condition number: ratio of largest to smallest (absolute) eigenvalue.
    pub fn condition_number(&self) -> f64 {
        let eigs = self.eigenvalues_approx();
        if eigs.is_empty() {
            return 1.0;
        }
        let max_eig = eigs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let min_eig = eigs.iter().cloned().fold(f64::INFINITY, f64::min);
        if min_eig.abs() < 1e-12 {
            return f64::INFINITY;
        }
        max_eig.abs() / min_eig.abs()
    }
}

/// Compute the determinant of the k×k leading principal submatrix via Gaussian elimination.
fn leading_minor_det(matrix: &[Vec<f64>], k: usize) -> f64 {
    // Copy the submatrix
    let mut a: Vec<Vec<f64>> = (0..k).map(|i| matrix[i][..k].to_vec()).collect();
    let mut det = 1.0_f64;
    for col in 0..k {
        // Find pivot
        let mut pivot_row = None;
        for row in col..k {
            if a[row][col].abs() > 1e-12 {
                pivot_row = Some(row);
                break;
            }
        }
        let pr = match pivot_row {
            Some(r) => r,
            None => return 0.0,
        };
        if pr != col {
            a.swap(col, pr);
            det = -det;
        }
        det *= a[col][col];
        let pivot = a[col][col];
        for row in (col + 1)..k {
            let factor = a[row][col] / pivot;
            for c in col..k {
                let sub = factor * a[col][c];
                a[row][c] -= sub;
            }
        }
    }
    det
}

/// Multiply matrix `a` by vector `v`.
fn mat_vec_mul(a: &[Vec<f64>], v: &[f64]) -> Vec<f64> {
    a.iter()
        .map(|row| row.iter().zip(v.iter()).map(|(&aij, &vj)| aij * vj).sum())
        .collect()
}

/// Euclidean norm of a vector.
fn vec_norm(v: &[f64]) -> f64 {
    v.iter().map(|&x| x * x).sum::<f64>().sqrt()
}

// ---------------------------------------------------------------------------
// Ledoit-Wolf shrinkage
// ---------------------------------------------------------------------------

/// Ledoit-Wolf analytical shrinkage toward the identity matrix.
pub struct LedoitWolfShrinkage;

impl LedoitWolfShrinkage {
    /// Shrink the sample correlation matrix toward the identity.
    ///
    /// Returns the shrunk `CorrelationMatrix` and the optimal shrinkage coefficient `alpha`.
    pub fn shrink(
        sample_corr: &CorrelationMatrix,
        returns: &[Vec<f64>],
    ) -> (CorrelationMatrix, f64) {
        let alpha = Self::optimal_alpha(returns);
        let target = Self::target_identity(sample_corr.n);
        let blended = Self::blend(sample_corr, &target, alpha);
        (blended, alpha)
    }

    /// Compute the Oracle Approximating Shrinkage (OAS) coefficient.
    ///
    /// Uses a simplified formula: `alpha = (1 - 2/p) * rho_bar / ((T - 1)(1 - rho_bar))`,
    /// clamped to `[0, 1]`.
    pub fn optimal_alpha(returns: &[Vec<f64>]) -> f64 {
        let p = returns.len();
        if p < 2 {
            return 0.0;
        }
        let t = returns[0].len();
        if t < 2 {
            return 0.0;
        }

        // Compute sample correlations to get average off-diagonal
        let sample = CorrelationMatrix::from_returns(returns);
        let mut rho_sum = 0.0_f64;
        let mut count = 0_usize;
        for i in 0..p {
            for j in (i + 1)..p {
                rho_sum += sample.get(i, j).abs();
                count += 1;
            }
        }
        let rho_bar = if count > 0 { rho_sum / count as f64 } else { 0.0 };

        let denom = (t as f64 - 1.0) * (1.0 - rho_bar);
        if denom.abs() < 1e-12 {
            return 0.5;
        }

        let alpha = ((1.0 - 2.0 / p as f64) * rho_bar) / denom;
        alpha.clamp(0.0, 1.0)
    }

    /// Construct an `n × n` identity correlation matrix.
    pub fn target_identity(n: usize) -> CorrelationMatrix {
        let mut matrix = vec![vec![0.0_f64; n]; n];
        for i in 0..n {
            matrix[i][i] = 1.0;
        }
        CorrelationMatrix { matrix, n }
    }

    /// Blend sample and target matrices: `(1-alpha) * sample + alpha * target`.
    pub fn blend(
        sample: &CorrelationMatrix,
        target: &CorrelationMatrix,
        alpha: f64,
    ) -> CorrelationMatrix {
        let n = sample.n;
        let alpha = alpha.clamp(0.0, 1.0);
        let mut matrix = vec![vec![0.0_f64; n]; n];
        for i in 0..n {
            for j in 0..n {
                matrix[i][j] =
                    (1.0 - alpha) * sample.matrix[i][j] + alpha * target.matrix[i][j];
            }
        }
        CorrelationMatrix { matrix, n }
    }
}

// ---------------------------------------------------------------------------
// DCC-GARCH utilities
// ---------------------------------------------------------------------------

/// Simplified DCC-GARCH correlation utilities.
pub struct DccGarch;

impl DccGarch {
    /// Compute rolling Pearson correlation between two series using a sliding window.
    ///
    /// Returns a `Vec<f64>` of length `series_a.len() - window + 1`.
    pub fn rolling_correlation(series_a: &[f64], series_b: &[f64], window: usize) -> Vec<f64> {
        let len = series_a.len().min(series_b.len());
        if window == 0 || len < window {
            return vec![];
        }
        let mut results = Vec::with_capacity(len - window + 1);
        for start in 0..=(len - window) {
            let a = &series_a[start..start + window];
            let b = &series_b[start..start + window];
            results.push(pearson_correlation(a, b));
        }
        results
    }

    /// Compute exponentially weighted moving average correlation.
    ///
    /// Uses decay factor `lambda` (e.g. 0.94 for RiskMetrics).
    pub fn ewma_correlation(series_a: &[f64], series_b: &[f64], lambda: f64) -> f64 {
        let len = series_a.len().min(series_b.len());
        if len == 0 {
            return 0.0;
        }

        // EWMA means
        let mut mean_a = 0.0_f64;
        let mut mean_b = 0.0_f64;
        let mut weight_sum = 0.0_f64;

        let mut w = 1.0_f64;
        for k in (0..len).rev() {
            mean_a += w * series_a[k];
            mean_b += w * series_b[k];
            weight_sum += w;
            w *= lambda;
        }
        mean_a /= weight_sum;
        mean_b /= weight_sum;

        // EWMA covariance and variances
        let mut cov = 0.0_f64;
        let mut var_a = 0.0_f64;
        let mut var_b = 0.0_f64;
        w = 1.0_f64;
        let mut ws = 0.0_f64;
        for k in (0..len).rev() {
            let da = series_a[k] - mean_a;
            let db = series_b[k] - mean_b;
            cov += w * da * db;
            var_a += w * da * da;
            var_b += w * db * db;
            ws += w;
            w *= lambda;
        }
        if ws > 0.0 {
            cov /= ws;
            var_a /= ws;
            var_b /= ws;
        }

        let denom = (var_a * var_b).sqrt();
        if denom < 1e-12 {
            0.0
        } else {
            (cov / denom).clamp(-1.0, 1.0)
        }
    }

    /// Simplified DCC correlation update equation.
    ///
    /// `q_t = (1 - a - b) * rho_bar + a * epsilon_{t-1}^2 + b * q_{t-1}`
    /// where `rho_bar` is a long-run target (approximated here as `prev_corr`).
    pub fn dcc_update(prev_corr: f64, a: f64, b: f64, epsilon_t: f64) -> f64 {
        let rho_bar = prev_corr;
        let q_t = (1.0 - a - b) * rho_bar + a * epsilon_t.powi(2) + b * prev_corr;
        q_t.clamp(-1.0, 1.0)
    }
}

/// Compute Pearson correlation between two slices of equal length.
fn pearson_correlation(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len();
    if n == 0 {
        return 0.0;
    }
    let mean_a = a.iter().sum::<f64>() / n as f64;
    let mean_b = b.iter().sum::<f64>() / n as f64;
    let mut cov = 0.0_f64;
    let mut var_a = 0.0_f64;
    let mut var_b = 0.0_f64;
    for (&ai, &bi) in a.iter().zip(b.iter()) {
        let da = ai - mean_a;
        let db = bi - mean_b;
        cov += da * db;
        var_a += da * da;
        var_b += db * db;
    }
    let denom = (var_a * var_b).sqrt();
    if denom < 1e-12 {
        0.0
    } else {
        (cov / denom).clamp(-1.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_returns() -> Vec<Vec<f64>> {
        vec![
            vec![0.01, -0.02, 0.03, 0.01, -0.01],
            vec![0.02, -0.01, 0.02, 0.00, -0.02],
        ]
    }

    #[test]
    fn correlation_diagonal_is_one() {
        let cm = CorrelationMatrix::from_returns(&sample_returns());
        assert!((cm.get(0, 0) - 1.0).abs() < 1e-9);
        assert!((cm.get(1, 1) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn correlation_is_symmetric() {
        let cm = CorrelationMatrix::from_returns(&sample_returns());
        assert!((cm.get(0, 1) - cm.get(1, 0)).abs() < 1e-12);
    }

    #[test]
    fn identity_is_positive_definite() {
        let id = LedoitWolfShrinkage::target_identity(3);
        assert!(id.is_positive_definite());
    }

    #[test]
    fn shrink_alpha_in_range() {
        let returns = sample_returns();
        let alpha = LedoitWolfShrinkage::optimal_alpha(&returns);
        assert!(alpha >= 0.0 && alpha <= 1.0);
    }

    #[test]
    fn rolling_correlation_length() {
        let a = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let b = vec![5.0, 4.0, 3.0, 2.0, 1.0];
        let rc = DccGarch::rolling_correlation(&a, &b, 3);
        assert_eq!(rc.len(), 3);
    }

    #[test]
    fn ewma_correlation_range() {
        let a = vec![0.01, -0.02, 0.03, 0.01];
        let b = vec![0.02, -0.01, 0.02, 0.00];
        let c = DccGarch::ewma_correlation(&a, &b, 0.94);
        assert!(c >= -1.0 && c <= 1.0);
    }
}
