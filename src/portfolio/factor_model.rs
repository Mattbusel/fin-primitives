//! Factor model for portfolio analysis: OLS regression, Fama-French 3-factor,
//! APT, information ratio, factor contributions, and systematic/idiosyncratic decomposition.

/// The type of a factor used in multi-factor models.
#[derive(Debug, Clone, PartialEq)]
pub enum FactorType {
    /// Broad equity market return (e.g. MKT-RF).
    Market,
    /// Small-minus-big size factor.
    Size,
    /// High-minus-low value factor.
    Value,
    /// Price momentum factor.
    Momentum,
    /// Profitability / quality factor.
    Quality,
    /// Low-volatility / defensive factor.
    LowVolatility,
    /// User-defined factor with a descriptive label.
    Custom(String),
}

/// A named return series representing one systematic risk factor.
#[derive(Debug, Clone)]
pub struct Factor {
    /// Human-readable factor name.
    pub name: String,
    /// Time-series of factor excess returns (one per period).
    pub returns: Vec<f64>,
    /// Semantic classification of the factor.
    pub factor_type: FactorType,
}

/// Estimated sensitivity of an asset to a single factor.
#[derive(Debug, Clone)]
pub struct FactorExposure {
    /// Name of the factor this exposure refers to.
    pub factor_name: String,
    /// OLS beta (sensitivity) of the asset to this factor.
    pub beta: f64,
    /// t-statistic for the beta estimate.
    pub t_stat: f64,
    /// True when |t_stat| > 2 (conventional 5 % significance).
    pub is_significant: bool,
}

/// Full output of a factor-model regression.
#[derive(Debug, Clone)]
pub struct FactorModelResult {
    /// Jensen's alpha (intercept) — excess return not explained by factors.
    pub alpha: f64,
    /// t-statistic for the alpha estimate.
    pub alpha_t_stat: f64,
    /// Per-factor exposure estimates.
    pub exposures: Vec<FactorExposure>,
    /// Coefficient of determination (R²) of the OLS fit.
    pub r_squared: f64,
    /// OLS residuals (idiosyncratic returns).
    pub residuals: Vec<f64>,
    /// Annualised information ratio: alpha / tracking_error * sqrt(252).
    pub information_ratio: f64,
    /// Tracking error — standard deviation of residuals.
    pub tracking_error: f64,
}

/// Multi-factor risk model supporting OLS regression and auxiliary analytics.
#[derive(Debug, Clone, Default)]
pub struct FactorModel;

impl FactorModel {
    /// Solve an OLS system using Gaussian elimination on the augmented matrix of
    /// the normal equations (X^T X) β = X^T y.
    ///
    /// Returns `(coefficients, r_squared)`.  `x` rows are observations; columns
    /// are regressors (the intercept column must already be included by the caller).
    pub fn ols(y: &[f64], x: &[Vec<f64>]) -> (Vec<f64>, f64) {
        let n = y.len();
        if n == 0 || x.is_empty() {
            return (vec![], 0.0);
        }
        let k = x[0].len(); // number of regressors (including intercept)

        // Build X^T X (k×k) and X^T y (k×1).
        let mut xtx = vec![vec![0.0_f64; k]; k];
        let mut xty = vec![0.0_f64; k];
        for (i, row) in x.iter().enumerate() {
            let yi = y[i];
            for j in 0..k {
                xty[j] += row[j] * yi;
                for l in 0..k {
                    xtx[j][l] += row[j] * row[l];
                }
            }
        }

        // Augment [X^T X | X^T y] and solve via Gaussian elimination with partial pivoting.
        let mut aug: Vec<Vec<f64>> = (0..k)
            .map(|r| {
                let mut row = xtx[r].clone();
                row.push(xty[r]);
                row
            })
            .collect();

        for col in 0..k {
            // Partial pivot
            let mut max_row = col;
            let mut max_val = aug[col][col].abs();
            for row in (col + 1)..k {
                if aug[row][col].abs() > max_val {
                    max_val = aug[row][col].abs();
                    max_row = row;
                }
            }
            aug.swap(col, max_row);

            let pivot = aug[col][col];
            if pivot.abs() < 1e-14 {
                continue; // singular column — skip
            }
            for row in 0..k {
                if row == col {
                    continue;
                }
                let factor = aug[row][col] / pivot;
                for c in col..=k {
                    aug[row][c] -= factor * aug[col][c];
                }
            }
            let d = aug[col][col];
            for c in col..=k {
                aug[col][c] /= d;
            }
        }

        let coefficients: Vec<f64> = (0..k).map(|r| aug[r][k]).collect();

        // Compute R².
        let y_mean = y.iter().sum::<f64>() / n as f64;
        let ss_tot: f64 = y.iter().map(|&yi| (yi - y_mean).powi(2)).sum();
        let ss_res: f64 = x
            .iter()
            .zip(y.iter())
            .map(|(row, &yi)| {
                let y_hat: f64 = row.iter().zip(coefficients.iter()).map(|(&xi, &b)| xi * b).sum();
                (yi - y_hat).powi(2)
            })
            .sum();
        let r_squared = if ss_tot < 1e-14 { 0.0 } else { 1.0 - ss_res / ss_tot };

        (coefficients, r_squared)
    }

    /// Compute t-statistics for each coefficient: β_i / SE_i where SE is derived from
    /// the OLS residual variance.
    pub fn t_statistics(
        coefficients: &[f64],
        x: &[Vec<f64>],
        y: &[f64],
        betas: &[f64],
    ) -> Vec<f64> {
        let n = y.len();
        let k = coefficients.len();
        if n <= k {
            return vec![0.0; k];
        }
        // Residual sum of squares.
        let rss: f64 = x
            .iter()
            .zip(y.iter())
            .map(|(row, &yi)| {
                let y_hat: f64 = row.iter().zip(betas.iter()).map(|(&xi, &b)| xi * b).sum();
                (yi - y_hat).powi(2)
            })
            .sum();
        let sigma2 = rss / (n - k) as f64;

        // (X^T X)^{-1} diagonal for SE.
        // Re-solve X^T X to get the diagonal of its inverse via the same augmented system.
        let kk = k;
        let mut xtx = vec![vec![0.0_f64; kk]; kk];
        for row in x.iter() {
            for j in 0..kk {
                for l in 0..kk {
                    xtx[j][l] += row[j] * row[l];
                }
            }
        }
        // Invert X^T X using Gauss-Jordan with identity augmentation.
        let mut aug: Vec<Vec<f64>> = (0..kk)
            .map(|r| {
                let mut row = xtx[r].clone();
                let mut id = vec![0.0_f64; kk];
                id[r] = 1.0;
                row.extend(id);
                row
            })
            .collect();

        for col in 0..kk {
            let mut max_row = col;
            let mut max_val = aug[col][col].abs();
            for row in (col + 1)..kk {
                if aug[row][col].abs() > max_val {
                    max_val = aug[row][col].abs();
                    max_row = row;
                }
            }
            aug.swap(col, max_row);
            let pivot = aug[col][col];
            if pivot.abs() < 1e-14 {
                continue;
            }
            for row in 0..kk {
                if row == col {
                    continue;
                }
                let f = aug[row][col] / pivot;
                for c in 0..(2 * kk) {
                    aug[row][c] -= f * aug[col][c];
                }
            }
            let d = aug[col][col];
            for c in 0..(2 * kk) {
                aug[col][c] /= d;
            }
        }

        // Diagonal of inverse is in columns kk..2kk.
        (0..k)
            .map(|i| {
                let var_i = sigma2 * aug[i][kk + i];
                if var_i <= 0.0 { 0.0 } else { coefficients[i] / var_i.sqrt() }
            })
            .collect()
    }

    /// Fit the factor model: regress `asset_returns` onto the provided `factors`.
    ///
    /// The design matrix includes an intercept column (index 0).
    pub fn fit(&self, asset_returns: &[f64], factors: &[Factor]) -> FactorModelResult {
        let n = asset_returns.len();
        if n == 0 || factors.is_empty() {
            return FactorModelResult {
                alpha: 0.0,
                alpha_t_stat: 0.0,
                exposures: vec![],
                r_squared: 0.0,
                residuals: vec![],
                information_ratio: 0.0,
                tracking_error: 0.0,
            };
        }

        // Build design matrix: [1, f1, f2, …].
        let x: Vec<Vec<f64>> = (0..n)
            .map(|i| {
                let mut row = vec![1.0_f64];
                for fac in factors.iter() {
                    row.push(*fac.returns.get(i).unwrap_or(&0.0));
                }
                row
            })
            .collect();

        let (coeffs, r_squared) = Self::ols(asset_returns, &x);
        let t_stats = Self::t_statistics(&coeffs, &x, asset_returns, &coeffs);

        let alpha = *coeffs.first().unwrap_or(&0.0);
        let alpha_t_stat = *t_stats.first().unwrap_or(&0.0);

        let exposures: Vec<FactorExposure> = factors
            .iter()
            .enumerate()
            .map(|(idx, fac)| {
                let beta = *coeffs.get(idx + 1).unwrap_or(&0.0);
                let t_stat = *t_stats.get(idx + 1).unwrap_or(&0.0);
                FactorExposure {
                    factor_name: fac.name.clone(),
                    beta,
                    t_stat,
                    is_significant: t_stat.abs() > 2.0,
                }
            })
            .collect();

        // Residuals.
        let residuals: Vec<f64> = x
            .iter()
            .zip(asset_returns.iter())
            .map(|(row, &yi)| {
                let y_hat: f64 = row.iter().zip(coeffs.iter()).map(|(&xi, &b)| xi * b).sum();
                yi - y_hat
            })
            .collect();

        let tracking_error = Self::std_dev(&residuals);
        let information_ratio = Self::information_ratio(alpha, &residuals);

        FactorModelResult {
            alpha,
            alpha_t_stat,
            exposures,
            r_squared,
            residuals,
            information_ratio,
            tracking_error,
        }
    }

    /// Fama-French three-factor model: regress excess asset returns onto MKT-RF, SMB, HML.
    ///
    /// `rf_rate` is the per-period risk-free rate subtracted from `asset_returns`.
    pub fn fama_french_3(
        asset_returns: &[f64],
        mkt_rf: &[f64],
        smb: &[f64],
        hml: &[f64],
        rf_rate: f64,
    ) -> FactorModelResult {
        let n = asset_returns.len();
        let excess: Vec<f64> = asset_returns.iter().map(|&r| r - rf_rate).collect();

        let mkt_factor = Factor {
            name: "MKT-RF".to_string(),
            returns: mkt_rf[..n.min(mkt_rf.len())].to_vec(),
            factor_type: FactorType::Market,
        };
        let smb_factor = Factor {
            name: "SMB".to_string(),
            returns: smb[..n.min(smb.len())].to_vec(),
            factor_type: FactorType::Size,
        };
        let hml_factor = Factor {
            name: "HML".to_string(),
            returns: hml[..n.min(hml.len())].to_vec(),
            factor_type: FactorType::Value,
        };

        let model = FactorModel;
        model.fit(&excess, &[mkt_factor, smb_factor, hml_factor])
    }

    /// Annualised information ratio: alpha / tracking_error * sqrt(252).
    ///
    /// Returns 0.0 when tracking_error is (near) zero.
    pub fn information_ratio(alpha: f64, residuals: &[f64]) -> f64 {
        let te = Self::std_dev(residuals);
        if te < 1e-14 { 0.0 } else { alpha / te * 252.0_f64.sqrt() }
    }

    /// Per-period factor contribution: beta_i * factor_return_i for each period.
    pub fn factor_contribution(
        exposures: &[FactorExposure],
        factor_returns: &[f64],
    ) -> Vec<f64> {
        exposures
            .iter()
            .zip(factor_returns.iter())
            .map(|(exp, &fr)| exp.beta * fr)
            .collect()
    }

    /// Total systematic return for a single period: sum of beta_i * factor_return_i.
    pub fn systematic_return(
        exposures: &[FactorExposure],
        period_factor_returns: &[f64],
    ) -> f64 {
        exposures
            .iter()
            .zip(period_factor_returns.iter())
            .map(|(exp, &fr)| exp.beta * fr)
            .sum()
    }

    /// Idiosyncratic (alpha) return for a single period.
    pub fn idiosyncratic_return(asset_return: f64, systematic: f64) -> f64 {
        asset_return - systematic
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    fn std_dev(v: &[f64]) -> f64 {
        let n = v.len();
        if n < 2 {
            return 0.0;
        }
        let mean = v.iter().sum::<f64>() / n as f64;
        let var = v.iter().map(|&x| (x - mean).powi(2)).sum::<f64>() / (n - 1) as f64;
        var.sqrt()
    }
}

// ---------------------------------------------------------------------------
// Arbitrage Pricing Theory model
// ---------------------------------------------------------------------------

/// Arbitrage Pricing Theory (APT) factor model.
///
/// Fits an asset's returns against an arbitrary set of macro factors and
/// computes the expected risk premium from pre-specified factor risk premia.
#[derive(Debug, Clone, Default)]
pub struct AptModel;

impl AptModel {
    /// Fit the APT model: regress `asset_returns` onto `macro_factors`.
    ///
    /// Each element of `macro_factors` is a full time-series for one factor.
    pub fn fit(asset_returns: &[f64], macro_factors: &[Vec<f64>]) -> FactorModelResult {
        let factors: Vec<Factor> = macro_factors
            .iter()
            .enumerate()
            .map(|(i, series)| Factor {
                name: format!("MacroFactor{}", i + 1),
                returns: series.clone(),
                factor_type: FactorType::Custom(format!("macro_{}", i + 1)),
            })
            .collect();
        let model = FactorModel;
        model.fit(asset_returns, &factors)
    }

    /// Expected risk premium: sum of beta_i * factor_risk_premium_i.
    pub fn risk_premium(exposures: &[FactorExposure], factor_risk_premia: &[f64]) -> f64 {
        exposures
            .iter()
            .zip(factor_risk_premia.iter())
            .map(|(exp, &rp)| exp.beta * rp)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ols_simple() {
        // y = 2 + 3x
        let x: Vec<Vec<f64>> = (0..10).map(|i| vec![1.0, i as f64]).collect();
        let y: Vec<f64> = (0..10).map(|i| 2.0 + 3.0 * i as f64).collect();
        let (coeffs, r2) = FactorModel::ols(&y, &x);
        assert!((coeffs[0] - 2.0).abs() < 1e-8, "intercept");
        assert!((coeffs[1] - 3.0).abs() < 1e-8, "slope");
        assert!((r2 - 1.0).abs() < 1e-8, "R2");
    }

    #[test]
    fn test_fama_french_3() {
        let n = 50;
        let mkt: Vec<f64> = (0..n).map(|i| 0.01 * (i as f64).sin()).collect();
        let smb: Vec<f64> = (0..n).map(|i| 0.005 * (i as f64).cos()).collect();
        let hml: Vec<f64> = vec![0.002; n];
        let asset: Vec<f64> = mkt.iter().zip(smb.iter()).map(|(&m, &s)| m + 0.5 * s + 0.001).collect();
        let result = FactorModel::fama_french_3(&asset, &mkt, &smb, &hml, 0.0);
        assert!(result.r_squared >= 0.0 && result.r_squared <= 1.0 + 1e-9);
        assert_eq!(result.exposures.len(), 3);
    }

    #[test]
    fn test_information_ratio() {
        let residuals = vec![0.01, -0.01, 0.02, -0.02, 0.01];
        let ir = FactorModel::information_ratio(0.001, &residuals);
        assert!(ir.is_finite());
    }

    #[test]
    fn test_apt_risk_premium() {
        let exposures = vec![
            FactorExposure { factor_name: "f1".into(), beta: 1.2, t_stat: 3.0, is_significant: true },
            FactorExposure { factor_name: "f2".into(), beta: 0.5, t_stat: 1.5, is_significant: false },
        ];
        let premia = vec![0.04, 0.02];
        let rp = AptModel::risk_premium(&exposures, &premia);
        assert!((rp - (1.2 * 0.04 + 0.5 * 0.02)).abs() < 1e-10);
    }
}
