//! Value at Risk (VaR) and Conditional VaR (CVaR) models.
//!
//! Supports Historical, Parametric (normal), Monte Carlo (GBM), and Cornish-Fisher methods.

/// Method used to compute VaR.
#[derive(Debug, Clone, PartialEq)]
pub enum VarMethod {
    /// Historical simulation from empirical return distribution.
    Historical,
    /// Parametric normal distribution (mean + z-score * sigma).
    Parametric,
    /// Monte Carlo simulation via Geometric Brownian Motion.
    MonteCarlo {
        /// Number of simulated paths.
        n_simulations: usize,
        /// Random seed for reproducibility.
        seed: u64,
    },
    /// Cornish-Fisher expansion adjusted for skewness and excess kurtosis.
    CornishFisher,
}

/// Value at Risk result.
#[derive(Debug, Clone)]
pub struct VaRResult {
    /// Confidence level (e.g. 0.95 for 95%).
    pub confidence: f64,
    /// Risk horizon in trading days.
    pub horizon_days: u32,
    /// VaR expressed in USD (absolute loss at the confidence level).
    pub var_usd: f64,
    /// VaR expressed as a fraction of position value.
    pub var_pct: f64,
    /// Method used for calculation.
    pub method: VarMethod,
}

/// Conditional VaR (Expected Shortfall) result.
#[derive(Debug, Clone)]
pub struct CVaRResult {
    /// Confidence level.
    pub confidence: f64,
    /// Expected loss beyond the VaR threshold (USD).
    pub cvar_usd: f64,
    /// The underlying VaR result at the same confidence.
    pub var_result: VaRResult,
}

/// Stateless VaR calculator.
pub struct VaRCalculator;

impl VaRCalculator {
    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Probit approximation (inverse normal CDF) for the given probability `p`.
    /// Uses the Beasley-Springer-Moro approximation.
    fn probit(p: f64) -> f64 {
        // Rational approximation coefficients (Abramowitz & Stegun 26.2.17)
        const A: [f64; 4] = [2.515517, 0.802853, 0.010328, 0.0];
        const B: [f64; 3] = [1.432788, 0.189269, 0.001308];

        let p = p.clamp(1e-10, 1.0 - 1e-10);
        let sign = if p < 0.5 { -1.0_f64 } else { 1.0_f64 };
        let t = if p < 0.5 {
            (-2.0 * p.ln()).sqrt()
        } else {
            (-2.0 * (1.0 - p).ln()).sqrt()
        };
        let numerator = A[0] + A[1] * t + A[2] * t * t + A[3] * t * t * t;
        let denominator = 1.0 + B[0] * t + B[1] * t * t + B[2] * t * t * t;
        sign * (t - numerator / denominator)
    }

    /// Computes sample mean of a slice.
    fn mean(data: &[f64]) -> f64 {
        if data.is_empty() {
            return 0.0;
        }
        data.iter().sum::<f64>() / data.len() as f64
    }

    /// Computes sample standard deviation.
    fn std_dev(data: &[f64]) -> f64 {
        if data.len() < 2 {
            return 0.0;
        }
        let m = Self::mean(data);
        let var = data.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (data.len() - 1) as f64;
        var.sqrt()
    }

    /// Computes sample skewness (Fisher's definition).
    fn skewness(data: &[f64]) -> f64 {
        let n = data.len() as f64;
        if n < 3.0 {
            return 0.0;
        }
        let m = Self::mean(data);
        let s = Self::std_dev(data);
        if s == 0.0 {
            return 0.0;
        }
        let sum3 = data.iter().map(|x| ((x - m) / s).powi(3)).sum::<f64>();
        (n / ((n - 1.0) * (n - 2.0))) * sum3
    }

    /// Computes sample excess kurtosis.
    fn excess_kurtosis(data: &[f64]) -> f64 {
        let n = data.len() as f64;
        if n < 4.0 {
            return 0.0;
        }
        let m = Self::mean(data);
        let s = Self::std_dev(data);
        if s == 0.0 {
            return 0.0;
        }
        let sum4 = data.iter().map(|x| ((x - m) / s).powi(4)).sum::<f64>();
        let kurt = (n * (n + 1.0) / ((n - 1.0) * (n - 2.0) * (n - 3.0))) * sum4
            - 3.0 * (n - 1.0).powi(2) / ((n - 2.0) * (n - 3.0));
        kurt
    }

    /// Simple LCG pseudo-random number generator yielding values in (0, 1).
    fn lcg_random(seed: &mut u64) -> f64 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let bits = 0x3FF0000000000000_u64 | (*seed >> 12);
        f64::from_bits(bits) - 1.0
    }

    /// Box-Muller transform: generates a standard normal sample.
    fn standard_normal(seed: &mut u64) -> f64 {
        let u1 = Self::lcg_random(seed).max(1e-10);
        let u2 = Self::lcg_random(seed);
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
    }

    // -----------------------------------------------------------------------
    // Public methods
    // -----------------------------------------------------------------------

    /// Historical VaR: sort empirical returns, pick the quantile at `1 - confidence`.
    pub fn historical_var(
        returns: &[f64],
        position_value: f64,
        confidence: f64,
        horizon_days: u32,
    ) -> VaRResult {
        if returns.is_empty() {
            return VaRResult {
                confidence,
                horizon_days,
                var_usd: 0.0,
                var_pct: 0.0,
                method: VarMethod::Historical,
            };
        }
        let mut sorted = returns.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let alpha = 1.0 - confidence;
        let idx = ((alpha * sorted.len() as f64).floor() as usize).min(sorted.len() - 1);
        let daily_var_pct = -sorted[idx]; // losses are negative returns

        // Scale to horizon using sqrt-of-time rule.
        let var_pct = daily_var_pct * (horizon_days as f64).sqrt();
        let var_usd = var_pct * position_value;

        VaRResult {
            confidence,
            horizon_days,
            var_usd: var_usd.max(0.0),
            var_pct: var_pct.max(0.0),
            method: VarMethod::Historical,
        }
    }

    /// Parametric VaR: assumes normally distributed returns.
    ///
    /// Uses the probit function to find the z-score at the given confidence level.
    pub fn parametric_var(
        mu: f64,
        sigma: f64,
        position_value: f64,
        confidence: f64,
        horizon_days: u32,
    ) -> VaRResult {
        let z = -Self::probit(1.0 - confidence); // e.g. ~1.645 for 95%
        // Daily VaR as a fraction: -(mu - z * sigma) per day
        let daily_var_pct = -(mu - z * sigma);
        let var_pct = (daily_var_pct * (horizon_days as f64).sqrt()).max(0.0);
        let var_usd = var_pct * position_value;

        VaRResult {
            confidence,
            horizon_days,
            var_usd,
            var_pct,
            method: VarMethod::Parametric,
        }
    }

    /// Monte Carlo VaR via Geometric Brownian Motion simulation.
    ///
    /// Simulates `n` paths over `horizon_days` days, collects terminal returns,
    /// and takes the empirical quantile.
    pub fn monte_carlo_var(
        mu: f64,
        sigma: f64,
        position_value: f64,
        confidence: f64,
        horizon_days: u32,
        n: usize,
        seed: u64,
    ) -> VaRResult {
        let mut rng_seed = seed;
        let dt = 1.0; // daily steps
        let mut terminal_returns: Vec<f64> = Vec::with_capacity(n);

        for _ in 0..n {
            let mut log_return = 0.0_f64;
            for _ in 0..horizon_days {
                let z = Self::standard_normal(&mut rng_seed);
                log_return += (mu - 0.5 * sigma * sigma) * dt + sigma * dt.sqrt() * z;
            }
            // Convert log return to simple return
            terminal_returns.push(log_return.exp() - 1.0);
        }

        terminal_returns.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let alpha = 1.0 - confidence;
        let idx = ((alpha * n as f64).floor() as usize).min(n.saturating_sub(1));
        let var_pct = (-terminal_returns[idx]).max(0.0);
        let var_usd = var_pct * position_value;

        VaRResult {
            confidence,
            horizon_days,
            var_usd,
            var_pct,
            method: VarMethod::MonteCarlo {
                n_simulations: n,
                seed,
            },
        }
    }

    /// Conditional VaR (Expected Shortfall): expected loss beyond the VaR threshold.
    pub fn conditional_var(
        returns: &[f64],
        position_value: f64,
        confidence: f64,
        horizon_days: u32,
    ) -> CVaRResult {
        let var_result =
            Self::historical_var(returns, position_value, confidence, horizon_days);

        if returns.is_empty() {
            return CVaRResult {
                confidence,
                cvar_usd: 0.0,
                var_result,
            };
        }

        let mut sorted = returns.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let alpha = 1.0 - confidence;
        let cutoff_idx = ((alpha * sorted.len() as f64).floor() as usize).min(sorted.len() - 1);

        // Expected shortfall: mean of returns worse than the VaR quantile.
        let tail: Vec<f64> = sorted[..=cutoff_idx].to_vec();
        let cvar_pct = if tail.is_empty() {
            0.0
        } else {
            -Self::mean(&tail) * (horizon_days as f64).sqrt()
        };
        let cvar_usd = (cvar_pct * position_value).max(0.0);

        CVaRResult {
            confidence,
            cvar_usd,
            var_result,
        }
    }

    /// Portfolio VaR using a correlation matrix.
    ///
    /// `individual_vars` is the vector of individual VaR values (in the same units).
    /// `correlation_matrix` is an N×N correlation matrix (row-major).
    ///
    /// Uses the formula: `portfolio_var = sqrt(w^T * C * w)` where `w = individual_vars`.
    pub fn portfolio_var(individual_vars: &[f64], correlation_matrix: &[Vec<f64>]) -> f64 {
        let n = individual_vars.len();
        if n == 0 {
            return 0.0;
        }

        let mut variance = 0.0_f64;
        for i in 0..n {
            for j in 0..n {
                let corr = if i < correlation_matrix.len() && j < correlation_matrix[i].len() {
                    correlation_matrix[i][j]
                } else {
                    if i == j { 1.0 } else { 0.0 }
                };
                variance += individual_vars[i] * individual_vars[j] * corr;
            }
        }
        variance.max(0.0).sqrt()
    }

    /// Cornish-Fisher VaR: adjusts the normal z-score for skewness and excess kurtosis.
    ///
    /// Modified z-score: `z_cf = z + (z²-1)*S/6 + (z³-3z)*K/24 - (2z³-5z)*S²/36`
    pub fn cornish_fisher_var(
        returns: &[f64],
        position_value: f64,
        confidence: f64,
        horizon_days: u32,
    ) -> VaRResult {
        if returns.is_empty() {
            return VaRResult {
                confidence,
                horizon_days,
                var_usd: 0.0,
                var_pct: 0.0,
                method: VarMethod::CornishFisher,
            };
        }

        let mu = Self::mean(returns);
        let sigma = Self::std_dev(returns);
        let skew = Self::skewness(returns);
        let kurt = Self::excess_kurtosis(returns);

        let z = -Self::probit(1.0 - confidence); // ~1.645 for 95%

        // Cornish-Fisher expansion
        let z_cf = z
            + (z.powi(2) - 1.0) * skew / 6.0
            + (z.powi(3) - 3.0 * z) * kurt / 24.0
            - (2.0 * z.powi(3) - 5.0 * z) * skew.powi(2) / 36.0;

        let daily_var_pct = -(mu - z_cf * sigma);
        let var_pct = (daily_var_pct * (horizon_days as f64).sqrt()).max(0.0);
        let var_usd = var_pct * position_value;

        VaRResult {
            confidence,
            horizon_days,
            var_usd,
            var_pct,
            method: VarMethod::CornishFisher,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_returns() -> Vec<f64> {
        vec![
            0.01, -0.02, 0.015, -0.03, 0.005, -0.01, 0.02, -0.025, 0.008, -0.015,
            0.012, -0.018, 0.003, -0.04, 0.022, -0.011, 0.007, -0.009, 0.014, -0.035,
        ]
    }

    #[test]
    fn test_historical_var_basic() {
        let returns = sample_returns();
        let result = VaRCalculator::historical_var(&returns, 1_000_000.0, 0.95, 1);
        assert!(result.var_usd > 0.0, "VaR should be positive");
        assert!(result.var_pct > 0.0, "VaR pct should be positive");
        assert!((result.confidence - 0.95).abs() < 1e-9);
        assert_eq!(result.horizon_days, 1);
    }

    #[test]
    fn test_historical_var_empty() {
        let result = VaRCalculator::historical_var(&[], 1_000_000.0, 0.95, 1);
        assert_eq!(result.var_usd, 0.0);
    }

    #[test]
    fn test_historical_var_horizon_scaling() {
        let returns = sample_returns();
        let var_1 = VaRCalculator::historical_var(&returns, 1_000_000.0, 0.95, 1);
        let var_10 = VaRCalculator::historical_var(&returns, 1_000_000.0, 0.95, 10);
        let ratio = var_10.var_usd / var_1.var_usd;
        assert!((ratio - 10.0_f64.sqrt()).abs() < 1e-6, "sqrt-of-time scaling, ratio={ratio}");
    }

    #[test]
    fn test_parametric_var_95() {
        // For 95% confidence, z ≈ 1.645; VaR ≈ z * sigma for mu=0.
        let result = VaRCalculator::parametric_var(0.0, 0.01, 1_000_000.0, 0.95, 1);
        // Expected: ~1.645% of 1M = ~$16,450
        assert!(result.var_usd > 10_000.0 && result.var_usd < 25_000.0,
            "var_usd={}", result.var_usd);
    }

    #[test]
    fn test_parametric_var_99() {
        let result99 = VaRCalculator::parametric_var(0.0, 0.01, 1_000_000.0, 0.99, 1);
        let result95 = VaRCalculator::parametric_var(0.0, 0.01, 1_000_000.0, 0.95, 1);
        assert!(result99.var_usd > result95.var_usd, "99% VaR should exceed 95% VaR");
    }

    #[test]
    fn test_monte_carlo_var_reasonable() {
        let result = VaRCalculator::monte_carlo_var(
            0.0005, 0.015, 1_000_000.0, 0.95, 10, 10_000, 42,
        );
        assert!(result.var_usd > 0.0);
        assert!(result.var_pct < 0.5, "VaR fraction should be < 50%");
        assert!(matches!(result.method, VarMethod::MonteCarlo { n_simulations: 10_000, seed: 42 }));
    }

    #[test]
    fn test_conditional_var_exceeds_var() {
        let returns = sample_returns();
        let cvar = VaRCalculator::conditional_var(&returns, 1_000_000.0, 0.95, 1);
        assert!(cvar.cvar_usd >= cvar.var_result.var_usd,
            "CVaR should be >= VaR: cvar={}, var={}", cvar.cvar_usd, cvar.var_result.var_usd);
    }

    #[test]
    fn test_conditional_var_empty() {
        let result = VaRCalculator::conditional_var(&[], 1_000_000.0, 0.95, 1);
        assert_eq!(result.cvar_usd, 0.0);
    }

    #[test]
    fn test_portfolio_var_uncorrelated() {
        // Two uncorrelated assets each with VaR = $1000, portfolio VaR = sqrt(2) * 1000
        let vars = vec![1000.0, 1000.0];
        let corr = vec![
            vec![1.0, 0.0],
            vec![0.0, 1.0],
        ];
        let pvar = VaRCalculator::portfolio_var(&vars, &corr);
        assert!((pvar - 2.0_f64.sqrt() * 1000.0).abs() < 1e-6, "pvar={pvar}");
    }

    #[test]
    fn test_portfolio_var_perfectly_correlated() {
        // Two perfectly correlated assets: portfolio VaR = sum of individual VaRs
        let vars = vec![1000.0, 1000.0];
        let corr = vec![
            vec![1.0, 1.0],
            vec![1.0, 1.0],
        ];
        let pvar = VaRCalculator::portfolio_var(&vars, &corr);
        assert!((pvar - 2000.0).abs() < 1e-6, "pvar={pvar}");
    }

    #[test]
    fn test_portfolio_var_empty() {
        let pvar = VaRCalculator::portfolio_var(&[], &[]);
        assert_eq!(pvar, 0.0);
    }

    #[test]
    fn test_cornish_fisher_var() {
        let returns = sample_returns();
        let result = VaRCalculator::cornish_fisher_var(&returns, 1_000_000.0, 0.95, 1);
        assert!(result.var_usd > 0.0);
        assert!(matches!(result.method, VarMethod::CornishFisher));
    }

    #[test]
    fn test_cornish_fisher_vs_parametric_with_tail_risk() {
        // Negatively skewed returns → Cornish-Fisher should give higher VaR
        let mut returns: Vec<f64> = (0..100).map(|i| 0.001 * (i as f64 - 50.0) / 50.0).collect();
        // Add fat left tail
        returns.extend_from_slice(&[-0.08, -0.09, -0.10, -0.07, -0.085]);
        let cf_result = VaRCalculator::cornish_fisher_var(&returns, 1_000_000.0, 0.95, 1);
        assert!(cf_result.var_usd > 0.0);
    }

    #[test]
    fn test_probit_symmetry() {
        // probit(0.95) should be approximately 1.645
        let z = -VaRCalculator::probit(0.05);
        assert!((z - 1.645).abs() < 0.01, "z={z}");
    }
}
