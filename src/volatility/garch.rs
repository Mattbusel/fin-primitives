//! # GARCH(1,1) Volatility Model
//!
//! Implements the Generalized Autoregressive Conditional Heteroskedasticity
//! model of order (1,1) with:
//! - MLE fitting via grid search + gradient descent refinement
//! - Conditional variance series computation
//! - Multi-step ahead variance forecasting
//! - Volatility term structure (annualised)
//!
//! ## Model
//! ```text
//! h_t = omega + alpha * eps_{t-1}^2 + beta * h_{t-1}
//! ```
//! Stationarity requires: alpha + beta < 1.
//!
//! ## NOT Responsible For
//! - GARCH(p,q) for p,q > 1
//! - Estimation of mean equation / ARMA-GARCH

// ─── Error ───────────────────────────────────────────────────────────────────

/// Errors that can arise during GARCH model operations.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum GarchError {
    /// Fewer than 10 observations supplied — not enough to fit a GARCH model.
    #[error("Insufficient data: need at least 10 returns, got {0}")]
    InsufficientData(usize),

    /// The supplied or fitted parameters violate alpha + beta < 1.
    #[error("Non-stationary parameters: alpha + beta = {0:.6} >= 1")]
    NonStationary(f64),

    /// The numerical optimiser failed to find a finite log-likelihood.
    #[error("Optimisation failed: could not find valid parameters")]
    OptimizationFailed,
}

// ─── Parameters ──────────────────────────────────────────────────────────────

/// GARCH(1,1) model parameters.
///
/// The conditional variance recursion is:
/// `h_t = omega + alpha * eps_{t-1}^2 + beta * h_{t-1}`
///
/// All three parameters must be strictly positive; additionally
/// `alpha + beta < 1` is required for covariance stationarity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GarchParams {
    /// Constant (long-run variance floor). Must be > 0.
    pub omega: f64,
    /// ARCH coefficient (lagged squared shock). Must be >= 0.
    pub alpha: f64,
    /// GARCH coefficient (lagged variance). Must be >= 0.
    pub beta: f64,
}

impl GarchParams {
    /// Unconditional (long-run) variance implied by the parameters.
    ///
    /// ```text
    /// σ²_∞ = omega / (1 - alpha - beta)
    /// ```
    ///
    /// Returns `f64::INFINITY` if the model is non-stationary.
    pub fn unconditional_variance(&self) -> f64 {
        let denom = 1.0 - self.alpha - self.beta;
        if denom <= 0.0 {
            return f64::INFINITY;
        }
        self.omega / denom
    }

    /// Returns `true` when the stationarity condition alpha + beta < 1 holds.
    pub fn is_stationary(&self) -> bool {
        self.alpha + self.beta < 1.0
    }
}

// ─── Fit result ──────────────────────────────────────────────────────────────

/// Result of fitting a GARCH(1,1) model to a returns series.
#[derive(Debug, Clone)]
pub struct GarchFit {
    /// Maximum-likelihood estimates for omega, alpha, beta.
    pub params: GarchParams,
    /// Maximised log-likelihood.
    pub log_likelihood: f64,
    /// Akaike Information Criterion: -2*ll + 2*k  (k=3).
    pub aic: f64,
    /// Bayesian Information Criterion: -2*ll + k*ln(n)  (k=3).
    pub bic: f64,
    /// Number of observations used in fitting.
    pub n_obs: usize,
}

// ─── Model ───────────────────────────────────────────────────────────────────

/// GARCH(1,1) model: fit, forecast, and term-structure computation.
pub struct GarchModel;

impl GarchModel {
    /// Fit a GARCH(1,1) model to the supplied return series via MLE.
    ///
    /// The optimisation proceeds in two phases:
    /// 1. **Grid search** over a coarse (omega, alpha, beta) grid to locate a
    ///    promising starting point.
    /// 2. **Gradient-descent refinement** from the best grid point.
    ///
    /// # Errors
    /// - [`GarchError::InsufficientData`] if `returns.len() < 10`.
    /// - [`GarchError::OptimizationFailed`] if no valid parameters are found.
    pub fn fit(returns: &[f64]) -> Result<GarchFit, GarchError> {
        let n = returns.len();
        if n < 10 {
            return Err(GarchError::InsufficientData(n));
        }

        // Initial variance estimate (h_0)
        let var0 = sample_variance(returns);

        // ── Phase 1: coarse grid search ──────────────────────────────────────
        let omegas = [var0 * 0.01, var0 * 0.05, var0 * 0.1, var0 * 0.2];
        let alphas = [0.05, 0.1, 0.15, 0.2];
        let betas = [0.70, 0.75, 0.80, 0.85];

        let mut best_ll = f64::NEG_INFINITY;
        let mut best_params = GarchParams { omega: var0 * 0.05, alpha: 0.1, beta: 0.8 };

        for &omega in &omegas {
            for &alpha in &alphas {
                for &beta in &betas {
                    if alpha + beta >= 1.0 {
                        continue;
                    }
                    let p = GarchParams { omega, alpha, beta };
                    if let Some(ll) = log_likelihood(returns, &p, var0) {
                        if ll > best_ll {
                            best_ll = ll;
                            best_params = p;
                        }
                    }
                }
            }
        }

        if best_ll == f64::NEG_INFINITY {
            return Err(GarchError::OptimizationFailed);
        }

        // ── Phase 2: gradient descent refinement ─────────────────────────────
        let refined = gradient_descent(returns, best_params, var0, 500, 1e-7);
        let final_ll = log_likelihood(returns, &refined, var0)
            .ok_or(GarchError::OptimizationFailed)?;

        let k = 3.0_f64;
        let nf = n as f64;
        let aic = -2.0 * final_ll + 2.0 * k;
        let bic = -2.0 * final_ll + k * nf.ln();

        Ok(GarchFit {
            params: refined,
            log_likelihood: final_ll,
            aic,
            bic,
            n_obs: n,
        })
    }

    /// Compute the full conditional variance series `h_t` for the given parameters.
    ///
    /// `h_0` is initialised to the sample variance of `returns`.
    /// The returned vector has the same length as `returns`.
    pub fn conditional_variance(params: &GarchParams, returns: &[f64]) -> Vec<f64> {
        if returns.is_empty() {
            return vec![];
        }
        let h0 = sample_variance(returns);
        compute_h_series(returns, params, h0)
    }

    /// Multi-step ahead variance forecasts.
    ///
    /// Returns a vector of length `horizon` where element `k` (0-indexed) is the
    /// `(k+1)`-step ahead conditional variance forecast.
    ///
    /// Recursion (unconditional mean reversion):
    /// ```text
    /// h_{t+1} = omega + (alpha + beta) * h_t
    /// h_{t+k} = omega_long + (alpha+beta)^{k-1} * (h_{t+1} - omega_long)   for k >= 2
    /// ```
    /// where `omega_long = omega / (1 - alpha - beta)`.
    pub fn forecast(params: &GarchParams, returns: &[f64], horizon: usize) -> Vec<f64> {
        if horizon == 0 || returns.is_empty() {
            return vec![];
        }
        let h0 = sample_variance(returns);
        let h_series = compute_h_series(returns, params, h0);
        let h_last = *h_series.last().unwrap_or(&h0);

        let ab = params.alpha + params.beta;
        let h1 = params.omega + ab * h_last;

        let mut forecasts = Vec::with_capacity(horizon);
        forecasts.push(h1);

        if params.is_stationary() {
            let long_run = params.unconditional_variance();
            for k in 1..horizon {
                // h_{t+k+1} = long_run + (alpha+beta)^k * (h1 - long_run)
                let h_k = long_run + ab.powi(k as i32) * (h1 - long_run);
                forecasts.push(h_k.max(0.0));
            }
        } else {
            // Non-stationary: extrapolate linearly (best effort)
            for _ in 1..horizon {
                let prev = *forecasts.last().unwrap_or(&h1);
                forecasts.push(params.omega + ab * prev);
            }
        }

        forecasts
    }

    /// Volatility term structure: annualised volatility for each horizon up to `max_horizon`.
    ///
    /// Returns a `Vec<(horizon_days, annualised_vol)>` for `horizon_days` in `1..=max_horizon`.
    /// Assumes 252 trading days per year.
    pub fn volatility_term_structure(
        params: &GarchParams,
        returns: &[f64],
        max_horizon: usize,
    ) -> Vec<(usize, f64)> {
        let forecasts = Self::forecast(params, returns, max_horizon);
        forecasts
            .into_iter()
            .enumerate()
            .map(|(i, h)| {
                let days = i + 1;
                // Annualised vol = sqrt(h * 252)
                let ann_vol = (h * 252.0).sqrt();
                (days, ann_vol)
            })
            .collect()
    }
}

// ─── internals ───────────────────────────────────────────────────────────────

fn sample_variance(returns: &[f64]) -> f64 {
    let n = returns.len();
    if n < 2 {
        return if n == 1 { returns[0] * returns[0] } else { 1e-6 };
    }
    let mean = returns.iter().sum::<f64>() / n as f64;
    let var = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (n - 1) as f64;
    var.max(1e-12)
}

/// Compute the conditional variance series given params and h_0.
fn compute_h_series(returns: &[f64], params: &GarchParams, h0: f64) -> Vec<f64> {
    let n = returns.len();
    let mut h = Vec::with_capacity(n);
    let mut h_prev = h0;
    h.push(h0);
    for t in 1..n {
        let eps_sq = returns[t - 1] * returns[t - 1];
        let h_t = params.omega + params.alpha * eps_sq + params.beta * h_prev;
        let h_t = h_t.max(1e-12);
        h.push(h_t);
        h_prev = h_t;
    }
    h
}

/// GARCH(1,1) log-likelihood under Gaussian innovations.
///
/// L = Σ_t -0.5 * (ln(2π) + ln(h_t) + ε_t²/h_t)
fn log_likelihood(returns: &[f64], params: &GarchParams, h0: f64) -> Option<f64> {
    if !params.is_stationary() || params.omega <= 0.0 || params.alpha < 0.0 || params.beta < 0.0 {
        return None;
    }
    let ln2pi = std::f64::consts::TAU.ln(); // ln(2π)
    let h_series = compute_h_series(returns, params, h0);
    let mut ll = 0.0_f64;
    for t in 0..returns.len() {
        let h_t = h_series[t];
        if h_t <= 0.0 {
            return None;
        }
        let eps = returns[t];
        ll += -0.5 * (ln2pi + h_t.ln() + eps * eps / h_t);
    }
    if ll.is_finite() { Some(ll) } else { None }
}

/// Simple gradient-descent optimiser for GARCH MLE.
///
/// Projects parameters back into the feasible region after each step.
fn gradient_descent(
    returns: &[f64],
    start: GarchParams,
    h0: f64,
    max_iter: usize,
    tol: f64,
) -> GarchParams {
    let mut p = start;
    let mut best_ll = log_likelihood(returns, &p, h0).unwrap_or(f64::NEG_INFINITY);
    let eps = 1e-6_f64;
    let mut step = 1e-4_f64;

    for _ in 0..max_iter {
        // Numerical gradient
        let grad_omega = numerical_grad(returns, &p, h0, eps, |q, d| GarchParams { omega: q.omega + d, ..*q });
        let grad_alpha = numerical_grad(returns, &p, h0, eps, |q, d| GarchParams { alpha: q.alpha + d, ..*q });
        let grad_beta  = numerical_grad(returns, &p, h0, eps, |q, d| GarchParams { beta:  q.beta  + d, ..*q });

        let grad_norm = (grad_omega * grad_omega + grad_alpha * grad_alpha + grad_beta * grad_beta).sqrt();
        if grad_norm < tol {
            break;
        }

        let candidate = GarchParams {
            omega: (p.omega + step * grad_omega / grad_norm).max(1e-9),
            alpha: (p.alpha + step * grad_alpha / grad_norm).max(0.0),
            beta:  (p.beta  + step * grad_beta  / grad_norm).max(0.0),
        };

        // Enforce stationarity constraint with margin
        if candidate.alpha + candidate.beta >= 0.9999 {
            step *= 0.5;
            continue;
        }

        if let Some(ll) = log_likelihood(returns, &candidate, h0) {
            if ll > best_ll {
                best_ll = ll;
                p = candidate;
                step *= 1.05;
            } else {
                step *= 0.7;
            }
        } else {
            step *= 0.5;
        }

        if step < 1e-12 {
            break;
        }
    }

    p
}

fn numerical_grad<F>(returns: &[f64], p: &GarchParams, h0: f64, eps: f64, perturb: F) -> f64
where
    F: Fn(&GarchParams, f64) -> GarchParams,
{
    let p_plus  = perturb(p,  eps);
    let p_minus = perturb(p, -eps);
    let ll_plus  = log_likelihood(returns, &p_plus,  h0).unwrap_or(f64::NEG_INFINITY);
    let ll_minus = log_likelihood(returns, &p_minus, h0).unwrap_or(f64::NEG_INFINITY);
    (ll_plus - ll_minus) / (2.0 * eps)
}

// ─── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_returns(n: usize) -> Vec<f64> {
        // Deterministic pseudo-volatility clustering series
        let mut r = Vec::with_capacity(n);
        let mut h = 0.0001_f64;
        let mut seed = 12345_u64;
        for _ in 0..n {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let u = (seed >> 33) as f64 / u32::MAX as f64 - 0.5;
            let innov = u * (h * 12.0).sqrt();
            r.push(innov);
            h = 1e-6 + 0.1 * innov * innov + 0.85 * h;
        }
        r
    }

    #[test]
    fn test_unconditional_variance() {
        let p = GarchParams { omega: 0.000002, alpha: 0.1, beta: 0.85 };
        let expected = 0.000002 / (1.0 - 0.1 - 0.85);
        assert!((p.unconditional_variance() - expected).abs() < 1e-12);
    }

    #[test]
    fn test_is_stationary_true() {
        let p = GarchParams { omega: 1e-6, alpha: 0.1, beta: 0.8 };
        assert!(p.is_stationary());
    }

    #[test]
    fn test_is_stationary_false_when_sum_ge_1() {
        let p = GarchParams { omega: 1e-6, alpha: 0.15, beta: 0.86 };
        assert!(!p.is_stationary());
    }

    #[test]
    fn test_unconditional_variance_infinity_when_nonstationary() {
        let p = GarchParams { omega: 1e-6, alpha: 0.5, beta: 0.6 };
        assert_eq!(p.unconditional_variance(), f64::INFINITY);
    }

    #[test]
    fn test_conditional_variance_first_element_is_sample_variance() {
        let returns = make_returns(50);
        let p = GarchParams { omega: 1e-6, alpha: 0.1, beta: 0.85 };
        let h = GarchModel::conditional_variance(&p, &returns);
        assert_eq!(h.len(), returns.len());

        // First element should equal sample_variance(returns)
        let sv = sample_variance(&returns);
        assert!((h[0] - sv).abs() < 1e-12, "h[0]={} sv={}", h[0], sv);
    }

    #[test]
    fn test_conditional_variance_all_positive() {
        let returns = make_returns(100);
        let p = GarchParams { omega: 1e-6, alpha: 0.1, beta: 0.85 };
        let h = GarchModel::conditional_variance(&p, &returns);
        assert!(h.iter().all(|&v| v > 0.0));
    }

    #[test]
    fn test_forecast_length() {
        let returns = make_returns(100);
        let p = GarchParams { omega: 1e-6, alpha: 0.1, beta: 0.85 };
        let f = GarchModel::forecast(&p, &returns, 10);
        assert_eq!(f.len(), 10);
    }

    #[test]
    fn test_forecast_converges_to_unconditional_variance() {
        let returns = make_returns(500);
        let p = GarchParams { omega: 0.00002, alpha: 0.08, beta: 0.85 };
        let long_horizon = 500;
        let f = GarchModel::forecast(&p, &returns, long_horizon);
        let last = *f.last().expect("non-empty");
        let uv = p.unconditional_variance();
        // Should be within 1% of long-run variance at very long horizon
        assert!((last - uv).abs() / uv < 0.01, "last={last:.8}, uv={uv:.8}");
    }

    #[test]
    fn test_insufficient_data_error() {
        let returns = make_returns(5);
        let result = GarchModel::fit(&returns);
        assert!(matches!(result, Err(GarchError::InsufficientData(5))));
    }

    #[test]
    fn test_fit_produces_stationary_params() {
        let returns = make_returns(200);
        let fit = GarchModel::fit(&returns).expect("fit should succeed");
        assert!(fit.params.is_stationary());
        assert!(fit.params.alpha >= 0.0);
        assert!(fit.params.beta >= 0.0);
        assert!(fit.params.omega > 0.0);
    }

    #[test]
    fn test_fit_aic_bic_finite() {
        let returns = make_returns(200);
        let fit = GarchModel::fit(&returns).expect("fit should succeed");
        assert!(fit.aic.is_finite());
        assert!(fit.bic.is_finite());
    }

    #[test]
    fn test_volatility_term_structure_length() {
        let returns = make_returns(100);
        let p = GarchParams { omega: 1e-5, alpha: 0.1, beta: 0.85 };
        let ts = GarchModel::volatility_term_structure(&p, &returns, 22);
        assert_eq!(ts.len(), 22);
        assert_eq!(ts[0].0, 1);
        assert_eq!(ts[21].0, 22);
    }

    #[test]
    fn test_volatility_term_structure_positive() {
        let returns = make_returns(100);
        let p = GarchParams { omega: 1e-5, alpha: 0.1, beta: 0.85 };
        let ts = GarchModel::volatility_term_structure(&p, &returns, 10);
        assert!(ts.iter().all(|(_, v)| *v >= 0.0));
    }
}
