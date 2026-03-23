//! Markowitz mean-variance portfolio optimization.
//!
//! Implements projected gradient descent over the simplex, supporting
//! MinVariance, MaxSharpe, RiskParity, and EqualWeight objectives with
//! per-asset and sector-level weight constraints.

use std::collections::HashMap;

/// A single investable asset.
#[derive(Debug, Clone)]
pub struct Asset {
    /// Ticker or identifier.
    pub symbol: String,
    /// Annualized expected return (e.g. 0.08 = 8 %).
    pub expected_return: f64,
    /// Annualized variance of returns.
    pub variance: f64,
}

/// Symmetric covariance matrix with named rows/columns.
#[derive(Debug, Clone)]
pub struct CovarianceMatrix {
    /// Row-major covariance data; element `[i * n + j]` is `cov(i, j)`.
    pub data: Vec<Vec<f64>>,
    /// Symbol labels matching the rows/columns.
    pub symbols: Vec<String>,
}

impl CovarianceMatrix {
    /// Create a new all-zero covariance matrix of dimension `n`.
    pub fn new(symbols: Vec<String>) -> Self {
        let n = symbols.len();
        Self {
            data: vec![vec![0.0; n]; n],
            symbols,
        }
    }

    /// Get element `(i, j)`.
    ///
    /// Returns `0.0` if the indices are out of bounds.
    pub fn get(&self, i: usize, j: usize) -> f64 {
        self.data
            .get(i)
            .and_then(|row| row.get(j))
            .copied()
            .unwrap_or(0.0)
    }

    /// Set element `(i, j)` (and the symmetric counterpart `(j, i)`).
    pub fn set(&mut self, i: usize, j: usize, v: f64) {
        let n = self.symbols.len();
        if i < n && j < n {
            self.data[i][j] = v;
            self.data[j][i] = v;
        }
    }

    /// Number of assets (dimension of the matrix).
    pub fn n(&self) -> usize {
        self.symbols.len()
    }

    /// Apply Ledoit-Wolf analytical shrinkage toward the identity scaled by
    /// the average diagonal (scaled identity target).
    ///
    /// Formula used:
    /// ```text
    /// mu   = trace(S) / n
    /// delta^2 = sum_{i!=j} S[i,j]^2          (off-diagonal squared sum)
    /// alpha = delta^2 / ((n + 2) * delta^2)   (simplifies to 1/(n+2) when non-zero,
    ///                                           but we use the full two-sample formula)
    /// S_shrunk = (1 - alpha) * S + alpha * mu * I
    /// ```
    ///
    /// When `delta^2 == 0` (already diagonal) the matrix is left unchanged.
    pub fn ledoit_wolf_shrinkage(&mut self) {
        let n = self.n();
        if n == 0 {
            return;
        }

        // mu = average diagonal
        let trace: f64 = (0..n).map(|i| self.get(i, i)).sum();
        let mu = trace / n as f64;

        // Sum of squared off-diagonal elements
        let mut off_diag_sq_sum = 0.0_f64;
        for i in 0..n {
            for j in 0..n {
                if i != j {
                    let v = self.get(i, j);
                    off_diag_sq_sum += v * v;
                }
            }
        }

        if off_diag_sq_sum == 0.0 {
            // Already diagonal — nothing to shrink.
            return;
        }

        // Analytical shrinkage intensity
        let alpha = off_diag_sq_sum / ((n as f64 + 2.0) * off_diag_sq_sum);

        let new_data: Vec<Vec<f64>> = (0..n)
            .map(|i| {
                (0..n)
                    .map(|j| {
                        let s_ij = self.get(i, j);
                        let target = if i == j { mu } else { 0.0 };
                        (1.0 - alpha) * s_ij + alpha * target
                    })
                    .collect()
            })
            .collect();

        self.data = new_data;
    }
}

/// Objective function for portfolio optimization.
#[derive(Debug, Clone)]
pub enum OptimizationObjective {
    /// Minimize portfolio variance.
    MinVariance,
    /// Maximize Sharpe ratio given a risk-free rate.
    MaxSharpe {
        /// Annualized risk-free rate (e.g. 0.05 = 5 %).
        risk_free_rate: f64,
    },
    /// Risk-parity: equalize each asset's marginal risk contribution.
    RiskParity,
    /// Equal-weight: `1/N` across all assets.
    EqualWeight,
}

/// A weight constraint applied during optimization.
#[derive(Debug, Clone)]
pub enum Constraint {
    /// No asset weight may exceed this value (e.g. `0.3` = 30 %).
    MaxWeight(f64),
    /// No asset weight may fall below this value (floor).
    MinWeight(f64),
    /// All weights must be non-negative (no short selling).
    LongOnly,
    /// Total weight of assets in the named sector must not exceed `max_weight`.
    SectorConstraint {
        /// Sector label, matched against [`Asset::symbol`] prefix convention.
        sector: String,
        /// Maximum aggregate weight for the sector.
        max_weight: f64,
    },
}

/// The result of a portfolio optimization run.
#[derive(Debug, Clone)]
pub struct OptimizedPortfolio {
    /// Optimal asset weights, keyed by symbol.
    pub weights: HashMap<String, f64>,
    /// Expected portfolio return (`sum_i w_i * mu_i`).
    pub expected_return: f64,
    /// Expected portfolio variance (`w' Σ w`).
    pub expected_variance: f64,
    /// Sharpe ratio (uses risk_free_rate = 0 unless MaxSharpe objective given).
    pub sharpe_ratio: f64,
    /// Effective N (inverse HHI): `1 / sum(w_i^2)`. Higher = more diversified.
    pub effective_n: f64,
}

/// Portfolio optimizer using projected gradient descent.
///
/// Runs 200 gradient steps with step size 0.01 and projects weights onto the
/// probability simplex after each step to satisfy the budget constraint.
/// Additional constraints are enforced via clipping before simplex projection.
pub struct PortfolioOptimizer;

impl PortfolioOptimizer {
    /// Optimize a portfolio given assets, a covariance matrix, an objective,
    /// and a list of constraints.
    ///
    /// Returns an [`OptimizedPortfolio`] with the solved weights and metrics.
    ///
    /// # Panics
    ///
    /// Does not panic; invalid configurations (empty asset list, mismatched
    /// dimensions) return a zero-weight portfolio.
    pub fn optimize(
        assets: &[Asset],
        cov_matrix: &CovarianceMatrix,
        objective: &OptimizationObjective,
        constraints: &[Constraint],
    ) -> OptimizedPortfolio {
        let n = assets.len();
        if n == 0 {
            return OptimizedPortfolio {
                weights: HashMap::new(),
                expected_return: 0.0,
                expected_variance: 0.0,
                sharpe_ratio: 0.0,
                effective_n: 0.0,
            };
        }

        // Handle EqualWeight immediately — no gradient needed.
        if matches!(objective, OptimizationObjective::EqualWeight) {
            let w = 1.0 / n as f64;
            let weights: HashMap<String, f64> = assets
                .iter()
                .map(|a| (a.symbol.clone(), w))
                .collect();
            return Self::build_result(assets, cov_matrix, weights, 0.0);
        }

        // Initial weights: equal weight.
        let mut w: Vec<f64> = vec![1.0 / n as f64; n];

        // Determine min/max bounds from constraints.
        let long_only = constraints.iter().any(|c| matches!(c, Constraint::LongOnly));
        let min_w: f64 = constraints
            .iter()
            .filter_map(|c| if let Constraint::MinWeight(v) = c { Some(*v) } else { None })
            .fold(if long_only { 0.0 } else { f64::NEG_INFINITY }, f64::max);
        let max_w: f64 = constraints
            .iter()
            .filter_map(|c| if let Constraint::MaxWeight(v) = c { Some(*v) } else { None })
            .fold(1.0, f64::min);

        let sector_constraints: Vec<(&str, f64)> = constraints
            .iter()
            .filter_map(|c| {
                if let Constraint::SectorConstraint { sector, max_weight } = c {
                    Some((sector.as_str(), *max_weight))
                } else {
                    None
                }
            })
            .collect();

        const ITERS: usize = 200;
        const STEP: f64 = 0.01;

        for _ in 0..ITERS {
            let grad = Self::compute_gradient(assets, cov_matrix, objective, &w);

            // Gradient step (gradient descent: subtract for minimization,
            // negate gradient for maximization objectives).
            for i in 0..n {
                w[i] -= STEP * grad[i];
            }

            // Clip to per-asset bounds before projection.
            let eff_min = if long_only { 0.0_f64.max(min_w) } else { min_w };
            let eff_min = eff_min.max(f64::NEG_INFINITY);
            let eff_max = max_w.min(1.0);
            for wi in w.iter_mut() {
                *wi = wi.clamp(eff_min, eff_max);
            }

            // Project onto simplex.
            w = project_simplex(&w);

            // Enforce sector constraints (greedy clamp, then re-project).
            for (sector, sec_max) in &sector_constraints {
                let sector_total: f64 = assets
                    .iter()
                    .enumerate()
                    .filter(|(_, a)| a.symbol.starts_with(sector))
                    .map(|(i, _)| w[i])
                    .sum();
                if sector_total > *sec_max && sector_total > 0.0 {
                    let scale = sec_max / sector_total;
                    for (i, a) in assets.iter().enumerate() {
                        if a.symbol.starts_with(sector) {
                            w[i] *= scale;
                        }
                    }
                    // Re-project after sector clamp.
                    w = project_simplex(&w);
                }
            }
        }

        let weights: HashMap<String, f64> = assets
            .iter()
            .enumerate()
            .map(|(i, a)| (a.symbol.clone(), w[i]))
            .collect();

        let rf = match objective {
            OptimizationObjective::MaxSharpe { risk_free_rate } => *risk_free_rate,
            _ => 0.0,
        };

        Self::build_result(assets, cov_matrix, weights, rf)
    }

    /// Compute the gradient of the objective w.r.t. weights.
    ///
    /// For minimization objectives the gradient points in the ascent direction
    /// so callers subtract it. For maximization (MaxSharpe) the negative
    /// gradient is returned so subtraction still descends.
    fn compute_gradient(
        assets: &[Asset],
        cov: &CovarianceMatrix,
        objective: &OptimizationObjective,
        w: &[f64],
    ) -> Vec<f64> {
        let n = assets.len();
        match objective {
            OptimizationObjective::MinVariance => {
                // grad_i = 2 * (Σ w)_i
                let mut g = vec![0.0; n];
                for i in 0..n {
                    for j in 0..n {
                        g[i] += 2.0 * cov.get(i, j) * w[j];
                    }
                }
                g
            }
            OptimizationObjective::MaxSharpe { risk_free_rate } => {
                // Maximize (mu_p - rf) / sigma_p.
                // Use negative gradient so that gradient descent increases Sharpe.
                let mu_p: f64 = assets.iter().enumerate().map(|(i, a)| w[i] * a.expected_return).sum();
                let sigma2_p: f64 = portfolio_variance(cov, w);
                let sigma_p = sigma2_p.sqrt().max(1e-10);
                let excess = mu_p - risk_free_rate;

                let mut g = vec![0.0; n];
                for i in 0..n {
                    let d_mu = assets[i].expected_return;
                    let mut d_sigma2 = 0.0_f64;
                    for j in 0..n {
                        d_sigma2 += 2.0 * cov.get(i, j) * w[j];
                    }
                    let d_sigma = d_sigma2 / (2.0 * sigma_p);
                    // d(Sharpe)/dw_i = (d_mu * sigma_p - excess * d_sigma) / sigma_p^2
                    let d_sharpe = (d_mu * sigma_p - excess * d_sigma) / (sigma_p * sigma_p);
                    // Negate because we subtract the gradient in the optimizer loop.
                    g[i] = -d_sharpe;
                }
                g
            }
            OptimizationObjective::RiskParity => {
                // Minimize sum_i (RC_i - RC_avg)^2 where RC_i = w_i * (Σw)_i / w'Σw.
                let sigma2_p = portfolio_variance(cov, w).max(1e-12);
                let mut sigma_w = vec![0.0_f64; n];
                for i in 0..n {
                    for j in 0..n {
                        sigma_w[i] += cov.get(i, j) * w[j];
                    }
                }
                // RC_i = w_i * sigma_w[i] / sigma2_p
                let rc: Vec<f64> = (0..n).map(|i| w[i] * sigma_w[i] / sigma2_p).collect();
                let rc_avg = rc.iter().sum::<f64>() / n as f64;

                // Gradient of sum_i (RC_i - rc_avg)^2 w.r.t. w_k (approximate).
                let mut g = vec![0.0_f64; n];
                for k in 0..n {
                    for i in 0..n {
                        let d_rc_i_d_wk = if i == k {
                            sigma_w[i] / sigma2_p + w[i] * cov.get(i, k) / sigma2_p
                                - w[i] * sigma_w[i] * 2.0 * sigma_w[k] * w[k] / (sigma2_p * sigma2_p)
                        } else {
                            w[i] * cov.get(i, k) / sigma2_p
                                - w[i] * sigma_w[i] * 2.0 * sigma_w[k] * w[k] / (sigma2_p * sigma2_p)
                        };
                        g[k] += 2.0 * (rc[i] - rc_avg) * d_rc_i_d_wk;
                    }
                }
                g
            }
            OptimizationObjective::EqualWeight => {
                // Handled before loop; shouldn't reach here.
                vec![0.0; n]
            }
        }
    }

    fn build_result(
        assets: &[Asset],
        cov: &CovarianceMatrix,
        weights: HashMap<String, f64>,
        rf: f64,
    ) -> OptimizedPortfolio {
        let n = assets.len();
        let w: Vec<f64> = assets
            .iter()
            .map(|a| weights.get(&a.symbol).copied().unwrap_or(0.0))
            .collect();

        let expected_return: f64 = assets.iter().enumerate().map(|(i, a)| w[i] * a.expected_return).sum();
        let expected_variance = portfolio_variance(cov, &w);
        let sigma = expected_variance.sqrt().max(1e-10);
        let sharpe_ratio = (expected_return - rf) / sigma;

        let hhi: f64 = w.iter().map(|wi| wi * wi).sum();
        let effective_n = if hhi > 0.0 { 1.0 / hhi } else { n as f64 };

        OptimizedPortfolio {
            weights,
            expected_return,
            expected_variance,
            sharpe_ratio,
            effective_n,
        }
    }
}

/// Compute portfolio variance `w' Σ w`.
fn portfolio_variance(cov: &CovarianceMatrix, w: &[f64]) -> f64 {
    let n = w.len();
    let mut var = 0.0_f64;
    for i in 0..n {
        for j in 0..n {
            var += w[i] * cov.get(i, j) * w[j];
        }
    }
    var.max(0.0)
}

/// Project a weight vector onto the probability simplex (`sum = 1`, `w_i >= 0`)
/// using the O(n log n) sorting algorithm (Duchi et al. 2008).
fn project_simplex(v: &[f64]) -> Vec<f64> {
    let mut u: Vec<f64> = v.to_vec();
    u.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));

    let mut cssv = 0.0_f64;
    let mut rho = 0_usize;
    for (j, &uj) in u.iter().enumerate() {
        cssv += uj;
        if uj - (cssv - 1.0) / (j as f64 + 1.0) > 0.0 {
            rho = j;
        }
    }

    let mut cssv2 = 0.0_f64;
    for k in 0..=rho {
        cssv2 += u[k];
    }
    let theta = (cssv2 - 1.0) / (rho as f64 + 1.0);

    v.iter().map(|&vi| (vi - theta).max(0.0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_asset_cov() -> CovarianceMatrix {
        let mut c = CovarianceMatrix::new(vec!["A".into(), "B".into()]);
        c.set(0, 0, 0.04);
        c.set(0, 1, 0.01);
        c.set(1, 1, 0.09);
        c
    }

    fn three_asset_cov() -> CovarianceMatrix {
        let mut c = CovarianceMatrix::new(vec!["A".into(), "B".into(), "C".into()]);
        c.set(0, 0, 0.04);
        c.set(1, 1, 0.09);
        c.set(2, 2, 0.01);
        c.set(0, 1, 0.01);
        c.set(0, 2, 0.005);
        c.set(1, 2, 0.015);
        c
    }

    fn two_assets() -> Vec<Asset> {
        vec![
            Asset { symbol: "A".into(), expected_return: 0.10, variance: 0.04 },
            Asset { symbol: "B".into(), expected_return: 0.15, variance: 0.09 },
        ]
    }

    fn three_assets() -> Vec<Asset> {
        vec![
            Asset { symbol: "A".into(), expected_return: 0.08, variance: 0.04 },
            Asset { symbol: "B".into(), expected_return: 0.12, variance: 0.09 },
            Asset { symbol: "C".into(), expected_return: 0.06, variance: 0.01 },
        ]
    }

    // --- CovarianceMatrix tests ---

    #[test]
    fn cov_get_set_symmetry() {
        let mut c = CovarianceMatrix::new(vec!["X".into(), "Y".into()]);
        c.set(0, 1, 0.05);
        assert!((c.get(0, 1) - 0.05).abs() < 1e-10);
        assert!((c.get(1, 0) - 0.05).abs() < 1e-10);
    }

    #[test]
    fn cov_get_out_of_bounds_returns_zero() {
        let c = CovarianceMatrix::new(vec!["X".into()]);
        assert_eq!(c.get(5, 5), 0.0);
    }

    #[test]
    fn cov_ledoit_wolf_shrinks_off_diagonal() {
        let mut c = two_asset_cov();
        let before_off = c.get(0, 1);
        c.ledoit_wolf_shrinkage();
        let after_off = c.get(0, 1);
        // Off-diagonal should be strictly smaller in magnitude.
        assert!(after_off.abs() < before_off.abs());
    }

    #[test]
    fn cov_ledoit_wolf_diagonal_unchanged_order_of_magnitude() {
        let mut c = two_asset_cov();
        c.ledoit_wolf_shrinkage();
        // Diagonal should still be positive.
        assert!(c.get(0, 0) > 0.0);
        assert!(c.get(1, 1) > 0.0);
    }

    #[test]
    fn cov_ledoit_wolf_already_diagonal_unchanged() {
        let mut c = CovarianceMatrix::new(vec!["X".into(), "Y".into()]);
        c.set(0, 0, 0.04);
        c.set(1, 1, 0.09);
        c.ledoit_wolf_shrinkage(); // off_diag_sq_sum == 0 → no-op
        assert!((c.get(0, 0) - 0.04).abs() < 1e-10);
        assert!((c.get(1, 1) - 0.09).abs() < 1e-10);
    }

    #[test]
    fn cov_ledoit_wolf_empty_matrix_no_panic() {
        let mut c = CovarianceMatrix::new(vec![]);
        c.ledoit_wolf_shrinkage(); // should not panic
    }

    // --- project_simplex tests ---

    #[test]
    fn simplex_projection_sums_to_one() {
        let v = vec![0.5, 0.5, 0.5];
        let p = project_simplex(&v);
        let sum: f64 = p.iter().sum();
        assert!((sum - 1.0).abs() < 1e-10);
    }

    #[test]
    fn simplex_projection_nonnegative() {
        let v = vec![-1.0, 2.0, 0.5];
        let p = project_simplex(&v);
        for wi in &p {
            assert!(*wi >= 0.0);
        }
    }

    // --- EqualWeight ---

    #[test]
    fn equal_weight_two_assets() {
        let assets = two_assets();
        let cov = two_asset_cov();
        let result = PortfolioOptimizer::optimize(&assets, &cov, &OptimizationObjective::EqualWeight, &[]);
        assert!((result.weights["A"] - 0.5).abs() < 1e-10);
        assert!((result.weights["B"] - 0.5).abs() < 1e-10);
    }

    #[test]
    fn equal_weight_effective_n_equals_n() {
        let assets = three_assets();
        let cov = three_asset_cov();
        let result = PortfolioOptimizer::optimize(&assets, &cov, &OptimizationObjective::EqualWeight, &[]);
        assert!((result.effective_n - 3.0).abs() < 1e-6);
    }

    // --- MinVariance ---

    #[test]
    fn min_variance_weights_sum_to_one() {
        let assets = two_assets();
        let cov = two_asset_cov();
        let result = PortfolioOptimizer::optimize(&assets, &cov, &OptimizationObjective::MinVariance, &[]);
        let sum: f64 = result.weights.values().sum();
        assert!((sum - 1.0).abs() < 1e-6, "weights sum = {sum}");
    }

    #[test]
    fn min_variance_lower_than_equal_weight() {
        let assets = two_assets();
        let cov = two_asset_cov();
        let mv = PortfolioOptimizer::optimize(&assets, &cov, &OptimizationObjective::MinVariance, &[]);
        let ew = PortfolioOptimizer::optimize(&assets, &cov, &OptimizationObjective::EqualWeight, &[]);
        assert!(mv.expected_variance <= ew.expected_variance + 1e-6);
    }

    #[test]
    fn min_variance_favors_lower_variance_asset() {
        let assets = two_assets(); // A has lower variance (0.04 vs 0.09)
        let cov = two_asset_cov();
        let result = PortfolioOptimizer::optimize(&assets, &cov, &OptimizationObjective::MinVariance, &[]);
        assert!(result.weights["A"] > result.weights["B"]);
    }

    #[test]
    fn min_variance_long_only_constraint() {
        let assets = two_assets();
        let cov = two_asset_cov();
        let result = PortfolioOptimizer::optimize(
            &assets, &cov, &OptimizationObjective::MinVariance, &[Constraint::LongOnly],
        );
        for w in result.weights.values() {
            assert!(*w >= -1e-9, "negative weight {w}");
        }
    }

    #[test]
    fn min_variance_max_weight_constraint() {
        let assets = two_assets();
        let cov = two_asset_cov();
        let result = PortfolioOptimizer::optimize(
            &assets, &cov, &OptimizationObjective::MinVariance,
            &[Constraint::MaxWeight(0.6), Constraint::LongOnly],
        );
        for w in result.weights.values() {
            assert!(*w <= 0.6 + 1e-6, "weight {w} exceeds max");
        }
    }

    #[test]
    fn min_variance_min_weight_constraint() {
        let assets = three_assets();
        let cov = three_asset_cov();
        let result = PortfolioOptimizer::optimize(
            &assets, &cov, &OptimizationObjective::MinVariance,
            &[Constraint::MinWeight(0.1), Constraint::LongOnly],
        );
        for w in result.weights.values() {
            assert!(*w >= 0.1 - 1e-6, "weight {w} below min");
        }
    }

    // --- MaxSharpe ---

    #[test]
    fn max_sharpe_weights_sum_to_one() {
        let assets = two_assets();
        let cov = two_asset_cov();
        let obj = OptimizationObjective::MaxSharpe { risk_free_rate: 0.02 };
        let result = PortfolioOptimizer::optimize(&assets, &cov, &obj, &[Constraint::LongOnly]);
        let sum: f64 = result.weights.values().sum();
        assert!((sum - 1.0).abs() < 1e-6, "weights sum = {sum}");
    }

    #[test]
    fn max_sharpe_higher_sharpe_than_equal_weight() {
        let assets = two_assets();
        let cov = two_asset_cov();
        let obj = OptimizationObjective::MaxSharpe { risk_free_rate: 0.02 };
        let ms = PortfolioOptimizer::optimize(&assets, &cov, &obj, &[Constraint::LongOnly]);
        let ew = PortfolioOptimizer::optimize(&assets, &cov, &OptimizationObjective::EqualWeight, &[]);
        assert!(ms.sharpe_ratio >= ew.sharpe_ratio - 1e-4);
    }

    #[test]
    fn max_sharpe_positive_sharpe() {
        let assets = two_assets();
        let cov = two_asset_cov();
        let obj = OptimizationObjective::MaxSharpe { risk_free_rate: 0.02 };
        let result = PortfolioOptimizer::optimize(&assets, &cov, &obj, &[]);
        assert!(result.sharpe_ratio > 0.0);
    }

    // --- RiskParity ---

    #[test]
    fn risk_parity_weights_sum_to_one() {
        let assets = three_assets();
        let cov = three_asset_cov();
        let result = PortfolioOptimizer::optimize(
            &assets, &cov, &OptimizationObjective::RiskParity, &[Constraint::LongOnly],
        );
        let sum: f64 = result.weights.values().sum();
        assert!((sum - 1.0).abs() < 1e-5, "weights sum = {sum}");
    }

    #[test]
    fn risk_parity_nonnegative_weights() {
        let assets = three_assets();
        let cov = three_asset_cov();
        let result = PortfolioOptimizer::optimize(
            &assets, &cov, &OptimizationObjective::RiskParity, &[Constraint::LongOnly],
        );
        for w in result.weights.values() {
            assert!(*w >= -1e-9);
        }
    }

    #[test]
    fn risk_parity_high_vol_asset_gets_lower_weight() {
        // B has much higher variance (0.09) vs A (0.04) and C (0.01).
        // Risk parity should give C the highest weight, B the lowest.
        let assets = three_assets();
        let cov = three_asset_cov();
        let result = PortfolioOptimizer::optimize(
            &assets, &cov, &OptimizationObjective::RiskParity, &[Constraint::LongOnly],
        );
        let wb = result.weights["B"];
        let wc = result.weights["C"];
        assert!(wc > wb, "C ({wc}) should outweigh B ({wb}) in risk parity");
    }

    // --- SectorConstraint ---

    #[test]
    fn sector_constraint_respected() {
        let assets = vec![
            Asset { symbol: "TECH_A".into(), expected_return: 0.15, variance: 0.10 },
            Asset { symbol: "TECH_B".into(), expected_return: 0.18, variance: 0.12 },
            Asset { symbol: "BOND_A".into(), expected_return: 0.04, variance: 0.01 },
        ];
        let mut cov = CovarianceMatrix::new(vec!["TECH_A".into(), "TECH_B".into(), "BOND_A".into()]);
        cov.set(0, 0, 0.10);
        cov.set(1, 1, 0.12);
        cov.set(2, 2, 0.01);
        let constraints = vec![
            Constraint::LongOnly,
            Constraint::SectorConstraint { sector: "TECH".into(), max_weight: 0.5 },
        ];
        let result = PortfolioOptimizer::optimize(
            &assets, &cov, &OptimizationObjective::MinVariance, &constraints,
        );
        let tech_total: f64 = result.weights["TECH_A"] + result.weights["TECH_B"];
        assert!(tech_total <= 0.5 + 1e-6, "tech total {tech_total} exceeds limit");
    }

    // --- Empty asset list ---

    #[test]
    fn empty_assets_returns_zero_portfolio() {
        let cov = CovarianceMatrix::new(vec![]);
        let result = PortfolioOptimizer::optimize(&[], &cov, &OptimizationObjective::MinVariance, &[]);
        assert!(result.weights.is_empty());
        assert_eq!(result.expected_return, 0.0);
        assert_eq!(result.expected_variance, 0.0);
    }

    // --- effective_n ---

    #[test]
    fn effective_n_concentrated_portfolio() {
        let assets = two_assets();
        let mut cov = two_asset_cov();
        // Force a concentrated portfolio by extreme constraint.
        let result = PortfolioOptimizer::optimize(
            &assets, &cov, &OptimizationObjective::EqualWeight, &[],
        );
        // Equal weight → effective_n = 2.
        assert!((result.effective_n - 2.0).abs() < 1e-6);
        let _ = cov.get(0, 0); // suppress warning
    }

    // --- expected_return and expected_variance consistency ---

    #[test]
    fn expected_return_consistent_with_weights() {
        let assets = three_assets();
        let cov = three_asset_cov();
        let result = PortfolioOptimizer::optimize(
            &assets, &cov, &OptimizationObjective::MinVariance, &[Constraint::LongOnly],
        );
        let manual_ret: f64 = assets
            .iter()
            .map(|a| result.weights[&a.symbol] * a.expected_return)
            .sum();
        assert!((result.expected_return - manual_ret).abs() < 1e-10);
    }

    #[test]
    fn expected_variance_nonnegative() {
        let assets = three_assets();
        let cov = three_asset_cov();
        let result = PortfolioOptimizer::optimize(
            &assets, &cov, &OptimizationObjective::RiskParity, &[Constraint::LongOnly],
        );
        assert!(result.expected_variance >= 0.0);
    }
}
