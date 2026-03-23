//! Black-Litterman portfolio optimization model.
//!
//! Blends market equilibrium implied returns with investor views to produce
//! a posterior return distribution, then computes optimal portfolio weights.

// ─────────────────────────────────────────
//  View
// ─────────────────────────────────────────

/// An investor view on one or more assets.
#[derive(Debug, Clone)]
pub struct View {
    /// Indices of assets involved in this view.
    pub asset_indices: Vec<usize>,
    /// View portfolio weights (relative: e.g. [1, -1] for long/short).
    pub weights: Vec<f64>,
    /// Expected return for this view.
    pub expected_return: f64,
}

// ─────────────────────────────────────────
//  BlackLittermanInput
// ─────────────────────────────────────────

/// Input parameters for the Black-Litterman model.
#[derive(Debug, Clone)]
pub struct BlackLittermanInput {
    /// Number of assets.
    pub n_assets: usize,
    /// Market capitalization weights.
    pub market_weights: Vec<f64>,
    /// Covariance matrix (n_assets × n_assets).
    pub sigma: Vec<Vec<f64>>,
    /// Risk aversion parameter (lambda).
    pub risk_aversion: f64,
    /// Uncertainty scalar for the prior (typically 0.025–0.05).
    pub tau: f64,
    /// Investor views.
    pub views: Vec<View>,
    /// Confidence in each view (0–1; higher = more confident).
    pub view_confidences: Vec<f64>,
}

// ─────────────────────────────────────────
//  BlackLittermanResult
// ─────────────────────────────────────────

/// Output of the Black-Litterman model.
#[derive(Debug, Clone)]
pub struct BlackLittermanResult {
    /// Posterior expected returns.
    pub posterior_returns: Vec<f64>,
    /// Posterior covariance matrix.
    pub posterior_cov: Vec<Vec<f64>>,
    /// Optimal portfolio weights.
    pub optimal_weights: Vec<f64>,
}

// ─────────────────────────────────────────
//  Matrix helpers
// ─────────────────────────────────────────

/// Multiply matrices A (m×k) and B (k×n) → C (m×n).
#[must_use]
pub fn matrix_mul(a: &[Vec<f64>], b: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let m = a.len();
    if m == 0 || b.is_empty() {
        return vec![];
    }
    let k = b.len();
    let n = b[0].len();
    let mut c = vec![vec![0.0_f64; n]; m];
    for i in 0..m {
        for j in 0..n {
            for l in 0..k {
                c[i][j] += a[i][l] * b[l][j];
            }
        }
    }
    c
}

/// Transpose matrix A (m×n) → A^T (n×m).
#[must_use]
pub fn matrix_transpose(a: &[Vec<f64>]) -> Vec<Vec<f64>> {
    if a.is_empty() || a[0].is_empty() {
        return vec![];
    }
    let m = a.len();
    let n = a[0].len();
    let mut t = vec![vec![0.0_f64; m]; n];
    for i in 0..m {
        for j in 0..n {
            t[j][i] = a[i][j];
        }
    }
    t
}

/// Element-wise add matrices A and B (must be same dimension).
#[must_use]
pub fn matrix_add(a: &[Vec<f64>], b: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let m = a.len();
    if m == 0 {
        return vec![];
    }
    let n = a[0].len();
    let mut c = vec![vec![0.0_f64; n]; m];
    for i in 0..m {
        for j in 0..n {
            c[i][j] = a[i][j] + b[i][j];
        }
    }
    c
}

/// Invert a 2×2 matrix. Returns `None` if the determinant is (near) zero.
#[must_use]
pub fn matrix_inv_2x2(a: &[[f64; 2]]) -> Option<[[f64; 2]; 2]> {
    let det = a[0][0] * a[1][1] - a[0][1] * a[1][0];
    if det.abs() < 1e-15 {
        return None;
    }
    let inv_det = 1.0 / det;
    Some([
        [a[1][1] * inv_det, -a[0][1] * inv_det],
        [-a[1][0] * inv_det, a[0][0] * inv_det],
    ])
}

/// Invert an n×n diagonal matrix (diagonal elements only).
fn invert_diagonal(diag: &[f64]) -> Vec<Vec<f64>> {
    let n = diag.len();
    let mut result = vec![vec![0.0_f64; n]; n];
    for i in 0..n {
        if diag[i].abs() > 1e-15 {
            result[i][i] = 1.0 / diag[i];
        }
    }
    result
}

/// Scale all elements of a matrix by a scalar.
fn matrix_scale(a: &[Vec<f64>], scalar: f64) -> Vec<Vec<f64>> {
    a.iter()
        .map(|row| row.iter().map(|&x| x * scalar).collect())
        .collect()
}

/// Add two vectors element-wise.
fn vec_add(a: &[f64], b: &[f64]) -> Vec<f64> {
    a.iter().zip(b.iter()).map(|(&x, &y)| x + y).collect()
}

/// Subtract two vectors element-wise.
fn vec_sub(a: &[f64], b: &[f64]) -> Vec<f64> {
    a.iter().zip(b.iter()).map(|(&x, &y)| x - y).collect()
}

/// Multiply matrix (m×n) by vector (n) → vector (m).
fn mat_vec_mul(mat: &[Vec<f64>], vec: &[f64]) -> Vec<f64> {
    mat.iter()
        .map(|row| row.iter().zip(vec.iter()).map(|(&a, &b)| a * b).sum())
        .collect()
}

// ─────────────────────────────────────────
//  BL core computations
// ─────────────────────────────────────────

/// Implied equilibrium returns: `Pi = lambda * Sigma * w`.
///
/// Returns a vector of n_assets equilibrium excess returns.
#[must_use]
pub fn implied_equilibrium_returns(input: &BlackLittermanInput) -> Vec<f64> {
    let sigma_w = mat_vec_mul(&input.sigma, &input.market_weights);
    sigma_w.iter().map(|&x| input.risk_aversion * x).collect()
}

/// Build the P matrix (n_views × n_assets) from investor views.
///
/// Each row encodes one view: assets at `view.asset_indices[i]` get weight `view.weights[i]`;
/// all other assets get weight 0.
#[must_use]
pub fn view_matrix_p(input: &BlackLittermanInput) -> Vec<Vec<f64>> {
    let n_views = input.views.len();
    let mut p = vec![vec![0.0_f64; input.n_assets]; n_views];
    for (row, view) in input.views.iter().enumerate() {
        for (&idx, &w) in view.asset_indices.iter().zip(view.weights.iter()) {
            if idx < input.n_assets {
                p[row][idx] = w;
            }
        }
    }
    p
}

/// Build the Omega matrix (n_views × n_views diagonal) representing view uncertainty.
///
/// `Omega_ii = tau * (P * Sigma * P^T)_ii * (1 / confidence_i)`.
#[must_use]
pub fn omega_matrix(input: &BlackLittermanInput) -> Vec<Vec<f64>> {
    let n_views = input.views.len();
    if n_views == 0 {
        return vec![];
    }
    let p = view_matrix_p(input);
    let p_sigma = matrix_mul(&p, &input.sigma);
    let pt = matrix_transpose(&p);
    let p_sigma_pt = matrix_mul(&p_sigma, &pt);

    let mut omega = vec![vec![0.0_f64; n_views]; n_views];
    for i in 0..n_views {
        let conf = input.view_confidences.get(i).copied().unwrap_or(1.0).max(1e-9);
        omega[i][i] = input.tau * p_sigma_pt[i][i] / conf;
    }
    omega
}

/// Compute BL posterior returns.
///
/// Formula: `posterior = Pi + tau * Sigma * P^T * (tau * P * Sigma * P^T + Omega)^{-1} * (q - P * Pi)`
///
/// For the general case with up to 2 views, the (tau*P*Sigma*P^T + Omega) matrix is at most 2×2
/// and can be inverted explicitly. For n_views == 1, a scalar inversion is used.
/// For n_views > 2 or unsupported cases, returns the prior equilibrium returns.
#[must_use]
pub fn bl_posterior_returns(input: &BlackLittermanInput) -> Vec<f64> {
    let pi = implied_equilibrium_returns(input);
    let n_views = input.views.len();

    if n_views == 0 {
        return pi;
    }

    let p = view_matrix_p(input);
    let pt = matrix_transpose(&p);
    let omega = omega_matrix(input);

    // q vector: expected returns of views
    let q: Vec<f64> = input.views.iter().map(|v| v.expected_return).collect();

    // tau * Sigma * P^T  (n_assets × n_views)
    let tau_sigma = matrix_scale(&input.sigma, input.tau);
    let tau_sigma_pt = matrix_mul(&tau_sigma, &pt);

    // tau * P * Sigma * P^T  (n_views × n_views)
    let p_sigma = matrix_mul(&p, &input.sigma);
    let p_sigma_pt = matrix_mul(&p_sigma, &pt);
    let tau_p_sigma_pt = matrix_scale(&p_sigma_pt, input.tau);

    // M = tau * P * Sigma * P^T + Omega  (n_views × n_views)
    let m = matrix_add(&tau_p_sigma_pt, &omega);

    // q - P * Pi
    let p_pi = mat_vec_mul(&p, &pi);
    let q_minus_p_pi = vec_sub(&q, &p_pi);

    // Invert M depending on size
    let correction: Vec<f64> = match n_views {
        1 => {
            let m_scalar = m[0][0];
            if m_scalar.abs() < 1e-15 {
                return pi;
            }
            let factor = q_minus_p_pi[0] / m_scalar;
            // tau_sigma_pt is (n_assets × 1); correction = factor * tau_sigma_pt[:, 0]
            tau_sigma_pt.iter().map(|row| row[0] * factor).collect()
        }
        2 => {
            let m2 = [[m[0][0], m[0][1]], [m[1][0], m[1][1]]];
            match matrix_inv_2x2(&m2) {
                None => return pi,
                Some(m_inv) => {
                    // m_inv * (q - P*Pi) → 2-element vector
                    let v0 = m_inv[0][0] * q_minus_p_pi[0] + m_inv[0][1] * q_minus_p_pi[1];
                    let v1 = m_inv[1][0] * q_minus_p_pi[0] + m_inv[1][1] * q_minus_p_pi[1];
                    // tau_sigma_pt (n_assets × 2) * [v0, v1]
                    tau_sigma_pt
                        .iter()
                        .map(|row| row[0] * v0 + row[1] * v1)
                        .collect()
                }
            }
        }
        _ => {
            // For n_views > 2: use diagonal approximation (treat M as diagonal)
            let diag: Vec<f64> = (0..n_views).map(|i| m[i][i]).collect();
            let m_inv = invert_diagonal(&diag);
            let m_inv_q = mat_vec_mul(&m_inv, &q_minus_p_pi);
            mat_vec_mul(&tau_sigma_pt, &m_inv_q)
        }
    };

    vec_add(&pi, &correction)
}

/// Compute BL posterior covariance.
///
/// Full formula: `(Sigma^{-1} + P^T * Omega^{-1} * P)^{-1}`.
/// For simplicity, approximate as `(1 + tau) * Sigma` when inversion is non-trivial,
/// or compute exactly for diagonal cases.
#[must_use]
pub fn bl_posterior_covariance(input: &BlackLittermanInput) -> Vec<Vec<f64>> {
    let n = input.n_assets;
    let n_views = input.views.len();
    let p = view_matrix_p(input);
    let pt = matrix_transpose(&p);
    let omega = omega_matrix(input);

    if n_views == 0 {
        // No views: posterior cov = (1 + tau) * Sigma
        return matrix_scale(&input.sigma, 1.0 + input.tau);
    }

    // Omega^{-1} (diagonal)
    let omega_diag: Vec<f64> = (0..n_views).map(|i| omega[i][i]).collect();
    let omega_inv = invert_diagonal(&omega_diag);

    // P^T * Omega^{-1} * P  (n × n)
    let omega_inv_p = matrix_mul(&omega_inv, &p);
    let pt_omega_inv_p = matrix_mul(&pt, &omega_inv_p);

    // Sigma^{-1}: approximate as diagonal 1/sigma_ii for simplicity
    let mut sigma_inv = vec![vec![0.0_f64; n]; n];
    for i in 0..n {
        if input.sigma[i][i].abs() > 1e-15 {
            sigma_inv[i][i] = 1.0 / input.sigma[i][i];
        }
    }

    // A = Sigma^{-1} + P^T * Omega^{-1} * P
    let a = matrix_add(&sigma_inv, &pt_omega_inv_p);

    // Invert A: for diagonal approximation, only use diagonal
    let a_diag: Vec<f64> = (0..n).map(|i| a[i][i]).collect();
    let a_inv = invert_diagonal(&a_diag);

    a_inv
}

/// Compute optimal portfolio weights from posterior returns and covariance.
///
/// `w = (lambda * Sigma)^{-1} * mu` then normalized to sum to 1.
/// For n > 2, falls back to equal weight.
#[must_use]
pub fn optimal_weights(posterior_returns: &[f64], sigma: &[Vec<f64>], lambda: f64) -> Vec<f64> {
    let n = posterior_returns.len();
    if n == 0 {
        return vec![];
    }
    if lambda == 0.0 {
        return vec![1.0 / n as f64; n];
    }

    // Use diagonal approximation for Sigma^{-1}
    let mut weights = vec![0.0_f64; n];
    for i in 0..n {
        let sigma_ii = sigma.get(i).and_then(|row| row.get(i)).copied().unwrap_or(1.0);
        if sigma_ii.abs() > 1e-15 {
            weights[i] = posterior_returns[i] / (lambda * sigma_ii);
        }
    }

    // Normalize so weights sum to 1 (long-only: clip negatives to 0)
    let sum_pos: f64 = weights.iter().map(|&w| w.max(0.0)).sum();
    if sum_pos > 1e-15 {
        weights.iter_mut().for_each(|w| {
            *w = w.max(0.0) / sum_pos;
        });
    } else {
        // All non-positive: equal weight
        weights = vec![1.0 / n as f64; n];
    }

    weights
}

/// Run the full Black-Litterman model and return posterior returns, covariance, and optimal weights.
#[must_use]
pub fn run(input: &BlackLittermanInput) -> BlackLittermanResult {
    let posterior_returns = bl_posterior_returns(input);
    let posterior_cov = bl_posterior_covariance(input);
    let weights = optimal_weights(&posterior_returns, &input.sigma, input.risk_aversion);
    BlackLittermanResult {
        posterior_returns,
        posterior_cov,
        optimal_weights: weights,
    }
}

// ─────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn two_asset_input() -> BlackLittermanInput {
        BlackLittermanInput {
            n_assets: 2,
            market_weights: vec![0.6, 0.4],
            sigma: vec![vec![0.04, 0.01], vec![0.01, 0.09]],
            risk_aversion: 2.5,
            tau: 0.05,
            views: vec![],
            view_confidences: vec![],
        }
    }

    #[test]
    fn implied_returns_proportional_to_weights() {
        // Pi = lambda * Sigma * w
        // For equal-variance diagonal Sigma, Pi should be proportional to w
        let input = BlackLittermanInput {
            n_assets: 2,
            market_weights: vec![0.5, 0.5],
            sigma: vec![vec![0.04, 0.0], vec![0.0, 0.04]],
            risk_aversion: 2.0,
            tau: 0.05,
            views: vec![],
            view_confidences: vec![],
        };
        let pi = implied_equilibrium_returns(&input);
        // Pi = 2.0 * [[0.04,0],[0,0.04]] * [0.5, 0.5] = [0.04, 0.04]
        assert!((pi[0] - 0.04).abs() < 1e-10);
        assert!((pi[1] - 0.04).abs() < 1e-10);
        // Both assets have same weight → same implied return → proportional to w ✓
    }

    #[test]
    fn no_views_posterior_equals_prior() {
        let input = two_asset_input();
        let pi = implied_equilibrium_returns(&input);
        let posterior = bl_posterior_returns(&input);
        for i in 0..2 {
            assert!(
                (posterior[i] - pi[i]).abs() < 1e-10,
                "without views, posterior should equal prior"
            );
        }
    }

    #[test]
    fn single_view_shifts_posterior_toward_view() {
        let mut input = two_asset_input();
        let pi = implied_equilibrium_returns(&input);
        // View: asset 0 will return 0.20 (much higher than equilibrium)
        input.views = vec![View {
            asset_indices: vec![0],
            weights: vec![1.0],
            expected_return: 0.20,
        }];
        input.view_confidences = vec![0.9];

        let posterior = bl_posterior_returns(&input);
        // Posterior for asset 0 should be between pi[0] and 0.20
        assert!(
            posterior[0] > pi[0],
            "posterior should shift toward view for asset 0"
        );
    }

    #[test]
    fn optimal_weights_sum_to_one() {
        let input = two_asset_input();
        let result = run(&input);
        let sum: f64 = result.optimal_weights.iter().sum();
        assert!((sum - 1.0).abs() < 1e-9, "weights should sum to 1, got {sum}");
    }

    #[test]
    fn weights_non_negative() {
        let input = two_asset_input();
        let result = run(&input);
        for &w in &result.optimal_weights {
            assert!(w >= 0.0, "weights should be non-negative");
        }
    }

    #[test]
    fn matrix_mul_identity() {
        let a = vec![vec![1.0, 0.0], vec![0.0, 1.0]];
        let b = vec![vec![3.0, 4.0], vec![5.0, 6.0]];
        let c = matrix_mul(&a, &b);
        assert!((c[0][0] - 3.0).abs() < 1e-10);
        assert!((c[1][1] - 6.0).abs() < 1e-10);
    }

    #[test]
    fn matrix_inv_2x2_basic() {
        // [[2, 0],[0, 4]] → [[0.5, 0],[0, 0.25]]
        let a = [[2.0_f64, 0.0], [0.0, 4.0]];
        let inv = matrix_inv_2x2(&a).unwrap();
        assert!((inv[0][0] - 0.5).abs() < 1e-10);
        assert!((inv[1][1] - 0.25).abs() < 1e-10);
    }

    #[test]
    fn matrix_inv_2x2_singular_returns_none() {
        let a = [[1.0_f64, 2.0], [2.0, 4.0]];
        assert!(matrix_inv_2x2(&a).is_none());
    }

    #[test]
    fn two_views_posterior_finite() {
        let mut input = two_asset_input();
        input.views = vec![
            View { asset_indices: vec![0], weights: vec![1.0], expected_return: 0.10 },
            View { asset_indices: vec![1], weights: vec![1.0], expected_return: 0.15 },
        ];
        input.view_confidences = vec![0.8, 0.7];
        let result = run(&input);
        for &r in &result.posterior_returns {
            assert!(r.is_finite(), "all posterior returns must be finite");
        }
        let sum: f64 = result.optimal_weights.iter().sum();
        assert!((sum - 1.0).abs() < 1e-9);
    }
}
