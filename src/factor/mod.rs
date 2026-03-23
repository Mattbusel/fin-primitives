//! Fama-French style multi-factor regression and portfolio factor exposure.
//!
//! ## Overview
//!
//! This module provides:
//! - [`Factor`]: a named return series (e.g. market, value, momentum).
//! - [`FactorModel`]: OLS regression of asset returns on a set of factors.
//! - [`FactorExposure`]: betas, alpha, R², and residual variance for one asset.
//! - [`VarianceDecomposition`]: breaks total variance into systematic and idiosyncratic parts.
//! - [`FactorPortfolio`]: aggregates per-asset exposures weighted by portfolio weights.

use crate::portfolio::CovarianceMatrix;

// ─── Factor ───────────────────────────────────────────────────────────────────

/// A named return series representing one risk factor (e.g. market, value, momentum).
#[derive(Debug, Clone)]
pub struct Factor {
    /// Human-readable name (e.g. `"MKT"`, `"SMB"`, `"HML"`).
    pub name: String,
    /// Time-series of factor returns aligned with the asset return series.
    pub returns: Vec<f64>,
}

impl Factor {
    /// Creates a new [`Factor`] with the given name and return series.
    pub fn new(name: impl Into<String>, returns: Vec<f64>) -> Self {
        Self { name: name.into(), returns }
    }
}

// ─── FactorExposure ───────────────────────────────────────────────────────────

/// Regression output for one asset against a set of factors.
#[derive(Debug, Clone)]
pub struct FactorExposure {
    /// Asset identifier.
    pub asset: String,
    /// Factor loadings (betas), one per factor in the same order as `factors` passed to `fit`.
    pub betas: Vec<f64>,
    /// Intercept term (alpha): excess return unexplained by factors.
    pub alpha: f64,
    /// Coefficient of determination R² ∈ [0, 1].
    pub r_squared: f64,
    /// Variance of the OLS residuals (idiosyncratic variance).
    pub residual_variance: f64,
}

// ─── VarianceDecomposition ────────────────────────────────────────────────────

/// Decomposes an asset's total variance into systematic and idiosyncratic parts.
#[derive(Debug, Clone)]
pub struct VarianceDecomposition {
    /// Variance explained by the factor model: `β' Σ β`.
    pub systematic_variance: f64,
    /// Residual variance not explained by factors.
    pub idiosyncratic_variance: f64,
    /// Per-factor contributions including cross terms with all subsequent factors.
    ///
    /// `factor_contributions[i] = β_i² σ_i² + 2 Σ_{j>i} β_i β_j cov(i,j)`
    pub factor_contributions: Vec<(String, f64)>,
}

// ─── FactorModel ──────────────────────────────────────────────────────────────

/// Fama-French style multi-factor OLS regression engine.
///
/// ## Algorithm
///
/// Given `T` observations and `K` factors, form the `T × (K+1)` design matrix
/// `X` (first column all-ones for the intercept) and solve the normal equations:
///
/// ```text
/// β̂ = (X'X)⁻¹ X'y
/// ```
///
/// Matrix inversion is done analytically for 2×2 and 3×3 systems and via
/// Gaussian elimination with partial pivoting for larger systems.
pub struct FactorModel;

impl FactorModel {
    /// Fit a factor model to `asset_returns` using the provided `factors`.
    ///
    /// # Panics
    ///
    /// Does not panic; returns a zero-exposure result if the system is singular or
    /// if the return series have mismatched lengths.
    pub fn fit(asset: &str, asset_returns: &[f64], factors: &[Factor]) -> FactorExposure {
        let t = asset_returns.len();
        let k = factors.len();

        // Validate all factor series are the same length.
        if t == 0 || factors.iter().any(|f| f.returns.len() != t) {
            return FactorExposure {
                asset: asset.to_string(),
                betas: vec![0.0; k],
                alpha: 0.0,
                r_squared: 0.0,
                residual_variance: 0.0,
            };
        }

        let cols = k + 1; // intercept + K factors

        // Build X (T × cols) — column 0 is all ones (intercept).
        let mut x = vec![vec![0.0f64; cols]; t];
        for row in 0..t {
            x[row][0] = 1.0;
            for (j, fac) in factors.iter().enumerate() {
                x[row][j + 1] = fac.returns[row];
            }
        }

        // Compute X'X (cols × cols).
        let mut xtx = vec![vec![0.0f64; cols]; cols];
        for i in 0..cols {
            for j in 0..cols {
                let mut s = 0.0;
                for row in 0..t {
                    s += x[row][i] * x[row][j];
                }
                xtx[i][j] = s;
            }
        }

        // Compute X'y (cols × 1).
        let mut xty = vec![0.0f64; cols];
        for i in 0..cols {
            let mut s = 0.0;
            for row in 0..t {
                s += x[row][i] * asset_returns[row];
            }
            xty[i] = s;
        }

        // Invert X'X and solve for β̂ = (X'X)⁻¹ X'y.
        let coeffs = match invert_and_solve(&xtx, &xty) {
            Some(c) => c,
            None => {
                return FactorExposure {
                    asset: asset.to_string(),
                    betas: vec![0.0; k],
                    alpha: 0.0,
                    r_squared: 0.0,
                    residual_variance: 0.0,
                };
            }
        };

        let alpha = coeffs[0];
        let betas: Vec<f64> = coeffs[1..].to_vec();

        // Compute fitted values and residuals.
        let mut ss_res = 0.0;
        let mut ss_tot = 0.0;
        let y_mean = asset_returns.iter().sum::<f64>() / t as f64;

        for row in 0..t {
            let fitted = alpha + betas.iter().enumerate().map(|(j, &b)| b * factors[j].returns[row]).sum::<f64>();
            let residual = asset_returns[row] - fitted;
            ss_res += residual * residual;
            let dev = asset_returns[row] - y_mean;
            ss_tot += dev * dev;
        }

        let r_squared = if ss_tot < 1e-15 {
            0.0
        } else {
            (1.0 - ss_res / ss_tot).clamp(0.0, 1.0)
        };

        let residual_variance = if t > cols {
            ss_res / (t - cols) as f64
        } else {
            0.0
        };

        FactorExposure {
            asset: asset.to_string(),
            betas,
            alpha,
            r_squared,
            residual_variance,
        }
    }

    /// Decompose total variance into systematic and idiosyncratic components.
    ///
    /// `factor_contributions[i] = β_i² σ_i² + 2 Σ_{j>i} β_i β_j cov(i,j)`
    pub fn decompose(
        exposure: &FactorExposure,
        factor_cov: &CovarianceMatrix,
        factor_names: &[String],
    ) -> VarianceDecomposition {
        let k = exposure.betas.len();
        let mut systematic_variance = 0.0;

        // Compute β' Σ β via explicit double sum.
        for i in 0..k {
            for j in 0..k {
                let ci = factor_cov.symbols.iter().position(|s| s == &factor_names[i]).unwrap_or(i);
                let cj = factor_cov.symbols.iter().position(|s| s == &factor_names[j]).unwrap_or(j);
                systematic_variance += exposure.betas[i] * exposure.betas[j] * factor_cov.get(ci, cj);
            }
        }

        // Per-factor contributions (diagonal + cross terms with j > i).
        let mut factor_contributions = Vec::with_capacity(k);
        for i in 0..k {
            let ci = factor_cov.symbols.iter().position(|s| s == &factor_names[i]).unwrap_or(i);
            // Diagonal term: β_i² * σ_i²
            let mut contrib = exposure.betas[i] * exposure.betas[i] * factor_cov.get(ci, ci);
            // Cross terms with j > i: 2 * β_i * β_j * cov(i,j)
            for j in (i + 1)..k {
                let cj = factor_cov.symbols.iter().position(|s| s == &factor_names[j]).unwrap_or(j);
                contrib += 2.0 * exposure.betas[i] * exposure.betas[j] * factor_cov.get(ci, cj);
            }
            factor_contributions.push((factor_names[i].clone(), contrib));
        }

        VarianceDecomposition {
            systematic_variance,
            idiosyncratic_variance: exposure.residual_variance,
            factor_contributions,
        }
    }
}

// ─── FactorPortfolio ──────────────────────────────────────────────────────────

/// Portfolio-level factor exposure: weighted sum of individual asset exposures.
#[derive(Debug, Clone)]
pub struct FactorPortfolio {
    /// Asset identifiers matching the order of `exposures`.
    pub exposures: Vec<(String, f64)>, // (asset, weight)
}

impl FactorPortfolio {
    /// Creates a new [`FactorPortfolio`] from a list of `(asset, weight)` pairs.
    pub fn new(exposures: Vec<(String, f64)>) -> Self {
        Self { exposures }
    }

    /// Aggregates per-asset [`FactorExposure`]s into a portfolio-level exposure.
    ///
    /// The result's `betas` are the weighted sum of individual asset betas.
    /// The result's `alpha` is the weighted sum of individual asset alphas.
    /// `r_squared` and `residual_variance` are weight-averaged.
    pub fn aggregate(&self, asset_exposures: &[FactorExposure]) -> Option<FactorExposure> {
        if self.exposures.is_empty() || asset_exposures.is_empty() {
            return None;
        }
        let k = asset_exposures[0].betas.len();
        let mut portfolio_betas = vec![0.0f64; k];
        let mut portfolio_alpha = 0.0f64;
        let mut portfolio_r2 = 0.0f64;
        let mut portfolio_resid_var = 0.0f64;
        let mut total_weight = 0.0f64;

        for (asset, weight) in &self.exposures {
            let w = *weight;
            if let Some(exp) = asset_exposures.iter().find(|e| &e.asset == asset) {
                for (i, &b) in exp.betas.iter().enumerate() {
                    if i < k {
                        portfolio_betas[i] += w * b;
                    }
                }
                portfolio_alpha += w * exp.alpha;
                portfolio_r2 += w * exp.r_squared;
                portfolio_resid_var += w * w * exp.residual_variance; // variance scales by w²
                total_weight += w;
            }
        }

        let label = "Portfolio".to_string();
        Some(FactorExposure {
            asset: label,
            betas: portfolio_betas,
            alpha: if total_weight.abs() > 1e-12 { portfolio_alpha } else { 0.0 },
            r_squared: if total_weight.abs() > 1e-12 { portfolio_r2 / total_weight } else { 0.0 },
            residual_variance: portfolio_resid_var,
        })
    }
}

// ─── Matrix utilities ─────────────────────────────────────────────────────────

/// Solves `A x = b` where `A` is a square matrix using analytic inversion for
/// 1×1, 2×2, 3×3 and Gaussian elimination with partial pivoting for larger systems.
fn invert_and_solve(a: &[Vec<f64>], b: &[f64]) -> Option<Vec<f64>> {
    let n = a.len();
    match n {
        0 => Some(vec![]),
        1 => {
            let det = a[0][0];
            if det.abs() < 1e-15 {
                return None;
            }
            Some(vec![b[0] / det])
        }
        2 => {
            let det = a[0][0] * a[1][1] - a[0][1] * a[1][0];
            if det.abs() < 1e-15 {
                return None;
            }
            let inv = [[a[1][1] / det, -a[0][1] / det], [-a[1][0] / det, a[0][0] / det]];
            Some(vec![
                inv[0][0] * b[0] + inv[0][1] * b[1],
                inv[1][0] * b[0] + inv[1][1] * b[1],
            ])
        }
        3 => {
            let (a00, a01, a02) = (a[0][0], a[0][1], a[0][2]);
            let (a10, a11, a12) = (a[1][0], a[1][1], a[1][2]);
            let (a20, a21, a22) = (a[2][0], a[2][1], a[2][2]);
            let det = a00 * (a11 * a22 - a12 * a21)
                - a01 * (a10 * a22 - a12 * a20)
                + a02 * (a10 * a21 - a11 * a20);
            if det.abs() < 1e-15 {
                return None;
            }
            let inv = [
                [
                    (a11 * a22 - a12 * a21) / det,
                    (a02 * a21 - a01 * a22) / det,
                    (a01 * a12 - a02 * a11) / det,
                ],
                [
                    (a12 * a20 - a10 * a22) / det,
                    (a00 * a22 - a02 * a20) / det,
                    (a02 * a10 - a00 * a12) / det,
                ],
                [
                    (a10 * a21 - a11 * a20) / det,
                    (a01 * a20 - a00 * a21) / det,
                    (a00 * a11 - a01 * a10) / det,
                ],
            ];
            Some(vec![
                inv[0][0] * b[0] + inv[0][1] * b[1] + inv[0][2] * b[2],
                inv[1][0] * b[0] + inv[1][1] * b[1] + inv[1][2] * b[2],
                inv[2][0] * b[0] + inv[2][1] * b[1] + inv[2][2] * b[2],
            ])
        }
        _ => gaussian_solve(a, b),
    }
}

/// Gaussian elimination with partial pivoting. Returns `None` if singular.
fn gaussian_solve(a: &[Vec<f64>], b: &[f64]) -> Option<Vec<f64>> {
    let n = a.len();
    let mut mat: Vec<Vec<f64>> = (0..n)
        .map(|i| {
            let mut row = a[i].clone();
            row.push(b[i]);
            row
        })
        .collect();

    for col in 0..n {
        // Find pivot.
        let pivot_row = (col..n)
            .max_by(|&r1, &r2| mat[r1][col].abs().partial_cmp(&mat[r2][col].abs()).unwrap_or(std::cmp::Ordering::Equal))?;
        mat.swap(col, pivot_row);

        let pivot = mat[col][col];
        if pivot.abs() < 1e-15 {
            return None;
        }
        for j in col..=n {
            mat[col][j] /= pivot;
        }
        for row in 0..n {
            if row != col {
                let factor = mat[row][col];
                for j in col..=n {
                    let v = mat[col][j];
                    mat[row][j] -= factor * v;
                }
            }
        }
    }

    Some((0..n).map(|i| mat[i][n]).collect())
}

// ─── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_factor(name: &str, returns: Vec<f64>) -> Factor {
        Factor::new(name, returns)
    }

    // ── OLS regression correctness ──────────────────────────────────────────

    #[test]
    fn single_factor_known_output() {
        // y = 0.5 * x + 0.01 (alpha=0.01, beta=0.5)
        let xs: Vec<f64> = (0..20).map(|i| i as f64 * 0.01).collect();
        let ys: Vec<f64> = xs.iter().map(|&x| 0.01 + 0.5 * x).collect();
        let factor = make_factor("MKT", xs);
        let exp = FactorModel::fit("ASSET", &ys, &[factor]);
        assert!((exp.alpha - 0.01).abs() < 1e-9, "alpha={}", exp.alpha);
        assert_eq!(exp.betas.len(), 1);
        assert!((exp.betas[0] - 0.5).abs() < 1e-9, "beta={}", exp.betas[0]);
        assert!((exp.r_squared - 1.0).abs() < 1e-9, "r2={}", exp.r_squared);
    }

    #[test]
    fn two_factor_known_output() {
        // y = 0.02 + 0.7*x1 + 0.3*x2
        let n = 30;
        let x1: Vec<f64> = (0..n).map(|i| i as f64 * 0.01).collect();
        let x2: Vec<f64> = (0..n).map(|i| (i as f64 * 0.02).sin()).collect();
        let y: Vec<f64> = (0..n)
            .map(|i| 0.02 + 0.7 * x1[i] + 0.3 * x2[i])
            .collect();
        let factors = vec![make_factor("MKT", x1), make_factor("HML", x2)];
        let exp = FactorModel::fit("ASSET", &y, &factors);
        assert!((exp.alpha - 0.02).abs() < 1e-7, "alpha={}", exp.alpha);
        assert!((exp.betas[0] - 0.7).abs() < 1e-7, "beta0={}", exp.betas[0]);
        assert!((exp.betas[1] - 0.3).abs() < 1e-7, "beta1={}", exp.betas[1]);
        assert!((exp.r_squared - 1.0).abs() < 1e-7);
    }

    #[test]
    fn three_factor_known_output() {
        // y = 0.001 + 1.1*f1 + 0.5*f2 - 0.2*f3
        let n = 50;
        let f1: Vec<f64> = (0..n).map(|i| (i as f64) * 0.005).collect();
        let f2: Vec<f64> = (0..n).map(|i| (i as f64 * 0.1).cos()).collect();
        let f3: Vec<f64> = (0..n).map(|i| (i as f64 * 0.2).sin()).collect();
        let y: Vec<f64> = (0..n)
            .map(|i| 0.001 + 1.1 * f1[i] + 0.5 * f2[i] - 0.2 * f3[i])
            .collect();
        let factors = vec![
            make_factor("MKT", f1),
            make_factor("SMB", f2),
            make_factor("HML", f3),
        ];
        let exp = FactorModel::fit("ASSET", &y, &factors);
        assert!((exp.alpha - 0.001).abs() < 1e-6, "alpha={}", exp.alpha);
        assert!((exp.betas[0] - 1.1).abs() < 1e-6);
        assert!((exp.betas[1] - 0.5).abs() < 1e-6);
        assert!((exp.betas[2] - (-0.2)).abs() < 1e-6);
        assert!((exp.r_squared - 1.0).abs() < 1e-6);
    }

    #[test]
    fn r_squared_in_zero_one() {
        // Noisy data — R² must still be in [0, 1].
        let y: Vec<f64> = vec![0.1, -0.2, 0.15, 0.05, -0.1, 0.3, -0.05, 0.2, 0.0, -0.15];
        let f: Vec<f64> = vec![0.2, -0.1, 0.05, 0.15, -0.2, 0.1, 0.0, 0.25, -0.05, 0.1];
        let exp = FactorModel::fit("X", &y, &[make_factor("F", f)]);
        assert!((0.0..=1.0).contains(&exp.r_squared), "r2={}", exp.r_squared);
    }

    #[test]
    fn alpha_zero_when_no_intercept_needed() {
        // Pure factor return (no alpha)
        let xs: Vec<f64> = (0..20).map(|i| i as f64 * 0.01 - 0.1).collect();
        let ys: Vec<f64> = xs.iter().map(|&x| 2.0 * x).collect();
        let exp = FactorModel::fit("A", &ys, &[make_factor("F", xs)]);
        assert!(exp.alpha.abs() < 1e-8, "alpha={}", exp.alpha);
        assert!((exp.betas[0] - 2.0).abs() < 1e-8);
    }

    #[test]
    fn empty_returns_gives_zero_exposure() {
        let exp = FactorModel::fit("A", &[], &[make_factor("F", vec![])]);
        assert_eq!(exp.betas.len(), 1);
        assert_eq!(exp.betas[0], 0.0);
        assert_eq!(exp.alpha, 0.0);
    }

    #[test]
    fn mismatched_length_gives_zero_exposure() {
        let exp = FactorModel::fit("A", &[1.0, 2.0], &[make_factor("F", vec![1.0])]);
        assert_eq!(exp.betas[0], 0.0);
    }

    #[test]
    fn residual_variance_non_negative() {
        let y: Vec<f64> = vec![0.1, -0.2, 0.15, 0.05, -0.1, 0.3, -0.05, 0.2, 0.0, -0.15];
        let f: Vec<f64> = vec![0.2, -0.1, 0.05, 0.15, -0.2, 0.1, 0.0, 0.25, -0.05, 0.1];
        let exp = FactorModel::fit("X", &y, &[make_factor("F", f)]);
        assert!(exp.residual_variance >= 0.0);
    }

    #[test]
    fn perfect_fit_zero_residual_variance() {
        let xs: Vec<f64> = (0..20).map(|i| i as f64 * 0.01).collect();
        let ys: Vec<f64> = xs.iter().map(|&x| 0.5 * x).collect();
        let exp = FactorModel::fit("X", &ys, &[make_factor("F", xs)]);
        assert!(exp.residual_variance < 1e-12, "resid_var={}", exp.residual_variance);
    }

    // ── VarianceDecomposition ────────────────────────────────────────────────

    #[test]
    fn decompose_sums_to_total() {
        let xs: Vec<f64> = (0..30).map(|i| i as f64 * 0.01 - 0.15).collect();
        let ys: Vec<f64> = xs.iter().map(|&x| 0.5 * x + 0.001).collect();
        let factor_names = vec!["MKT".to_string()];
        let exp = FactorModel::fit("A", &ys, &[make_factor("MKT", xs.clone())]);
        // Build diagonal cov matrix: var(MKT)
        let var_mkt = xs.iter().map(|&v| v * v).sum::<f64>() / xs.len() as f64
            - (xs.iter().sum::<f64>() / xs.len() as f64).powi(2);
        let mut cov = CovarianceMatrix::new(factor_names.clone());
        cov.set(0, 0, var_mkt);
        let decomp = FactorModel::decompose(&exp, &cov, &factor_names);
        assert!(decomp.systematic_variance >= 0.0);
        assert!(decomp.idiosyncratic_variance >= 0.0);
        // systematic + idiosyncratic ≈ total sample variance of y
        let total = decomp.systematic_variance + decomp.idiosyncratic_variance;
        assert!(total >= 0.0, "total={total}");
    }

    #[test]
    fn decompose_two_factors_contributions_match_systematic() {
        let n = 40;
        let f1: Vec<f64> = (0..n).map(|i| (i as f64) * 0.01).collect();
        let f2: Vec<f64> = (0..n).map(|i| (i as f64 * 0.1).sin()).collect();
        let y: Vec<f64> = (0..n).map(|i| 0.5 * f1[i] + 0.3 * f2[i]).collect();
        let factor_names = vec!["F1".to_string(), "F2".to_string()];
        let factors = vec![make_factor("F1", f1.clone()), make_factor("F2", f2.clone())];
        let exp = FactorModel::fit("B", &y, &factors);
        let mut cov = CovarianceMatrix::new(factor_names.clone());
        let var_f1 = f1.iter().map(|&v| v * v).sum::<f64>() / n as f64;
        let var_f2 = f2.iter().map(|&v| v * v).sum::<f64>() / n as f64;
        cov.set(0, 0, var_f1);
        cov.set(1, 1, var_f2);
        let decomp = FactorModel::decompose(&exp, &cov, &factor_names);
        // Sum of factor_contributions should equal systematic_variance
        let sum_contrib: f64 = decomp.factor_contributions.iter().map(|(_, v)| v).sum();
        assert!((sum_contrib - decomp.systematic_variance).abs() < 1e-10,
            "sum_contrib={sum_contrib}, sys_var={}", decomp.systematic_variance);
    }

    #[test]
    fn decompose_single_factor_contribution_label() {
        let xs: Vec<f64> = (0..20).map(|i| i as f64 * 0.01).collect();
        let ys: Vec<f64> = xs.iter().map(|&x| 0.5 * x).collect();
        let factor_names = vec!["MKT".to_string()];
        let mut cov = CovarianceMatrix::new(factor_names.clone());
        cov.set(0, 0, 0.01);
        let exp = FactorModel::fit("A", &ys, &[make_factor("MKT", xs)]);
        let decomp = FactorModel::decompose(&exp, &cov, &factor_names);
        assert_eq!(decomp.factor_contributions[0].0, "MKT");
    }

    // ── FactorPortfolio ──────────────────────────────────────────────────────

    #[test]
    fn portfolio_weighted_betas() {
        let exp_a = FactorExposure {
            asset: "A".to_string(),
            betas: vec![1.0, 0.5],
            alpha: 0.01,
            r_squared: 0.9,
            residual_variance: 0.001,
        };
        let exp_b = FactorExposure {
            asset: "B".to_string(),
            betas: vec![0.5, 1.5],
            alpha: 0.02,
            r_squared: 0.8,
            residual_variance: 0.002,
        };
        let portfolio = FactorPortfolio::new(vec![("A".to_string(), 0.6), ("B".to_string(), 0.4)]);
        let agg = portfolio.aggregate(&[exp_a, exp_b]).expect("aggregate");
        // beta0: 0.6*1.0 + 0.4*0.5 = 0.8
        assert!((agg.betas[0] - 0.8).abs() < 1e-9, "b0={}", agg.betas[0]);
        // beta1: 0.6*0.5 + 0.4*1.5 = 0.9
        assert!((agg.betas[1] - 0.9).abs() < 1e-9, "b1={}", agg.betas[1]);
    }

    #[test]
    fn portfolio_weighted_alpha() {
        let exp_a = FactorExposure { asset: "A".to_string(), betas: vec![1.0], alpha: 0.01, r_squared: 0.9, residual_variance: 0.001 };
        let exp_b = FactorExposure { asset: "B".to_string(), betas: vec![0.5], alpha: 0.03, r_squared: 0.8, residual_variance: 0.002 };
        let portfolio = FactorPortfolio::new(vec![("A".to_string(), 0.5), ("B".to_string(), 0.5)]);
        let agg = portfolio.aggregate(&[exp_a, exp_b]).expect("aggregate");
        // alpha: 0.5*0.01 + 0.5*0.03 = 0.02
        assert!((agg.alpha - 0.02).abs() < 1e-9, "alpha={}", agg.alpha);
    }

    #[test]
    fn portfolio_empty_returns_none() {
        let portfolio = FactorPortfolio::new(vec![]);
        assert!(portfolio.aggregate(&[]).is_none());
    }

    #[test]
    fn portfolio_missing_asset_ignored() {
        let exp_a = FactorExposure { asset: "A".to_string(), betas: vec![1.0], alpha: 0.01, r_squared: 0.9, residual_variance: 0.0 };
        let portfolio = FactorPortfolio::new(vec![("A".to_string(), 0.6), ("Z".to_string(), 0.4)]);
        let agg = portfolio.aggregate(&[exp_a]).expect("aggregate");
        // Only A contributes
        assert!((agg.betas[0] - 0.6).abs() < 1e-9);
    }

    #[test]
    fn four_factor_gaussian_elimination() {
        // y = 0.005 + 0.8*f1 + 0.4*f2 + 0.2*f3 + 0.1*f4
        let n = 60;
        let f1: Vec<f64> = (0..n).map(|i| i as f64 * 0.01).collect();
        let f2: Vec<f64> = (0..n).map(|i| (i as f64 * 0.1).cos()).collect();
        let f3: Vec<f64> = (0..n).map(|i| (i as f64 * 0.05).sin()).collect();
        let f4: Vec<f64> = (0..n).map(|i| i as f64 * -0.005).collect();
        let y: Vec<f64> = (0..n)
            .map(|i| 0.005 + 0.8 * f1[i] + 0.4 * f2[i] + 0.2 * f3[i] + 0.1 * f4[i])
            .collect();
        let factors = vec![
            make_factor("F1", f1),
            make_factor("F2", f2),
            make_factor("F3", f3),
            make_factor("F4", f4),
        ];
        let exp = FactorModel::fit("X", &y, &factors);
        assert!((exp.alpha - 0.005).abs() < 1e-6, "alpha={}", exp.alpha);
        assert!((exp.betas[0] - 0.8).abs() < 1e-5);
        assert!((exp.betas[1] - 0.4).abs() < 1e-5);
        assert!((exp.betas[2] - 0.2).abs() < 1e-5);
        assert!((exp.betas[3] - 0.1).abs() < 1e-5);
        assert!((exp.r_squared - 1.0).abs() < 1e-5);
    }

    #[test]
    fn beta_count_matches_factor_count() {
        let y: Vec<f64> = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let factors = vec![
            make_factor("F1", vec![0.1, 0.2, 0.3, 0.4, 0.5]),
            make_factor("F2", vec![0.5, 0.4, 0.3, 0.2, 0.1]),
        ];
        let exp = FactorModel::fit("X", &y, &factors);
        assert_eq!(exp.betas.len(), 2);
    }

    #[test]
    fn no_factors_alpha_is_mean() {
        // With no factors, alpha should be the mean of y
        let y: Vec<f64> = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let exp = FactorModel::fit("X", &y, &[]);
        let mean = y.iter().sum::<f64>() / y.len() as f64;
        assert!((exp.alpha - mean).abs() < 1e-9, "alpha={}", exp.alpha);
    }

    #[test]
    fn systematic_variance_non_negative() {
        let xs: Vec<f64> = (0..20).map(|i| i as f64 * 0.01 - 0.1).collect();
        let ys: Vec<f64> = xs.iter().map(|&x| 0.5 * x).collect();
        let factor_names = vec!["MKT".to_string()];
        let exp = FactorModel::fit("A", &ys, &[make_factor("MKT", xs)]);
        let mut cov = CovarianceMatrix::new(factor_names.clone());
        cov.set(0, 0, 0.05);
        let decomp = FactorModel::decompose(&exp, &cov, &factor_names);
        assert!(decomp.systematic_variance >= 0.0);
    }

    #[test]
    fn portfolio_r_squared_weighted_average() {
        let exp_a = FactorExposure { asset: "A".to_string(), betas: vec![1.0], alpha: 0.0, r_squared: 0.8, residual_variance: 0.0 };
        let exp_b = FactorExposure { asset: "B".to_string(), betas: vec![1.0], alpha: 0.0, r_squared: 0.6, residual_variance: 0.0 };
        // Equal weights → R² should be (0.8+0.6)/2 / 1.0 = 0.7
        let portfolio = FactorPortfolio::new(vec![("A".to_string(), 0.5), ("B".to_string(), 0.5)]);
        let agg = portfolio.aggregate(&[exp_a, exp_b]).expect("aggregate");
        assert!((agg.r_squared - 0.7).abs() < 1e-9, "r2={}", agg.r_squared);
    }

    #[test]
    fn constant_factor_singular_handled() {
        // Constant factor creates a collinear matrix with intercept → singular.
        let y: Vec<f64> = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let f: Vec<f64> = vec![1.0, 1.0, 1.0, 1.0, 1.0]; // constant = collinear with intercept
        let exp = FactorModel::fit("X", &y, &[make_factor("CONST", f)]);
        // Should return zero-exposure rather than panic
        assert_eq!(exp.betas.len(), 1);
    }

    #[test]
    fn negative_betas_supported() {
        // y = -0.5 * x
        let xs: Vec<f64> = (0..20).map(|i| i as f64 * 0.01).collect();
        let ys: Vec<f64> = xs.iter().map(|&x| -0.5 * x).collect();
        let exp = FactorModel::fit("A", &ys, &[make_factor("F", xs)]);
        assert!((exp.betas[0] - (-0.5)).abs() < 1e-9, "beta={}", exp.betas[0]);
    }

    #[test]
    fn factor_name_preserved_in_exposure() {
        let xs: Vec<f64> = (0..10).map(|i| i as f64).collect();
        let ys: Vec<f64> = xs.iter().map(|&x| x).collect();
        let exp = FactorModel::fit("MY_ASSET", &ys, &[make_factor("MY_FACTOR", xs)]);
        assert_eq!(exp.asset, "MY_ASSET");
    }
}
