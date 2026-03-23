//! Value-at-Risk (VaR) calculation engine.
//!
//! Supports Historical, Parametric, and Monte Carlo methods with CVaR (Expected Shortfall).

/// Method used to compute VaR.
#[derive(Debug, Clone, PartialEq)]
pub enum VarMethod {
    /// Historical simulation from empirical return distribution.
    Historical,
    /// Parametric normal distribution using Cornish-Fisher z-scores.
    Parametric,
    /// Monte Carlo simulation using LCG + Box-Muller transform.
    MonteCarlo {
        /// Number of simulated paths.
        simulations: usize,
        /// Random seed for reproducibility.
        seed: u64,
    },
}

/// Result of a VaR calculation.
#[derive(Debug, Clone)]
pub struct VarResult {
    /// Confidence level (e.g. 0.95 for 95%).
    pub confidence_level: f64,
    /// VaR expressed as an absolute dollar loss.
    pub var_abs: f64,
    /// VaR expressed as a fraction of portfolio value.
    pub var_pct: f64,
    /// Conditional VaR (Expected Shortfall) in absolute terms.
    pub cvar_abs: f64,
    /// Conditional VaR as a fraction of portfolio value.
    pub cvar_pct: f64,
    /// Method used for this calculation.
    pub method: VarMethod,
}

/// Portfolio return series with position weights.
#[derive(Debug, Clone)]
pub struct PortfolioReturns {
    /// Per-asset return series (each inner Vec is one asset's returns).
    pub returns: Vec<f64>,
    /// Weight for each asset (must match length of `returns`).
    pub weights: Vec<f64>,
    /// Total portfolio market value in dollars.
    pub portfolio_value: f64,
}

impl PortfolioReturns {
    /// Compute the weighted portfolio return for each observation.
    ///
    /// Returns a single `Vec<f64>` where each element is the dot product
    /// of the weight vector and the corresponding per-asset return.
    /// When `returns` and `weights` have the same length (flat single-asset
    /// case), this is equivalent to `weight[i] * return[i]`.
    pub fn portfolio_returns(&self) -> Vec<f64> {
        self.returns
            .iter()
            .zip(self.weights.iter())
            .map(|(r, w)| r * w)
            .collect()
    }

    /// Arithmetic mean of the (scalar) return series.
    pub fn mean(&self) -> f64 {
        let pr = self.portfolio_returns();
        if pr.is_empty() {
            return 0.0;
        }
        pr.iter().sum::<f64>() / pr.len() as f64
    }

    /// Population standard deviation of the (scalar) return series.
    pub fn std_dev(&self) -> f64 {
        let pr = self.portfolio_returns();
        if pr.len() < 2 {
            return 0.0;
        }
        let mean = pr.iter().sum::<f64>() / pr.len() as f64;
        let variance = pr.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / pr.len() as f64;
        variance.sqrt()
    }
}

/// Stateless VaR calculation engine.
pub struct VarEngine;

impl VarEngine {
    /// Historical VaR: sort returns, pick the loss at the (1-confidence) percentile.
    pub fn historical_var(returns: &[f64], confidence: f64, portfolio_value: f64) -> VarResult {
        if returns.is_empty() {
            return VarResult {
                confidence_level: confidence,
                var_abs: 0.0,
                var_pct: 0.0,
                cvar_abs: 0.0,
                cvar_pct: 0.0,
                method: VarMethod::Historical,
            };
        }

        let mut sorted = returns.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let n = sorted.len();
        let var_idx = ((1.0 - confidence) * n as f64).floor() as usize;
        let var_idx = var_idx.min(n - 1);

        let var_pct = -sorted[var_idx].min(0.0);
        let var_abs = var_pct * portfolio_value;

        let cvar_pct = Self::cvar_from_var(&sorted, var_idx);
        let cvar_abs = cvar_pct * portfolio_value;

        VarResult {
            confidence_level: confidence,
            var_abs,
            var_pct,
            cvar_abs,
            cvar_pct,
            method: VarMethod::Historical,
        }
    }

    /// Parametric VaR using Cornish-Fisher z-scores scaled by horizon.
    ///
    /// z values: 2.326 for 99%, 1.645 for 95%, 1.282 for 90%, linear interpolation otherwise.
    pub fn parametric_var(
        mean: f64,
        std_dev: f64,
        confidence: f64,
        portfolio_value: f64,
        horizon_days: u32,
    ) -> VarResult {
        let z = Self::cornish_fisher_z(confidence);
        let horizon_scale = (horizon_days as f64).sqrt();
        let var_pct = (z * std_dev - mean) * horizon_scale;
        let var_pct = var_pct.max(0.0);
        let var_abs = var_pct * portfolio_value;

        // CVaR for normal: phi(z) / (1 - confidence) * sigma * sqrt(horizon)
        let phi_z = Self::standard_normal_pdf(z);
        let cvar_pct = if (1.0 - confidence).abs() < 1e-12 {
            var_pct
        } else {
            (phi_z / (1.0 - confidence) * std_dev - mean) * horizon_scale
        };
        let cvar_pct = cvar_pct.max(var_pct);
        let cvar_abs = cvar_pct * portfolio_value;

        VarResult {
            confidence_level: confidence,
            var_abs,
            var_pct,
            cvar_abs,
            cvar_pct,
            method: VarMethod::Parametric,
        }
    }

    /// Monte Carlo VaR using LCG random number generator and Box-Muller transform.
    pub fn monte_carlo_var(
        mean: f64,
        std_dev: f64,
        confidence: f64,
        portfolio_value: f64,
        simulations: usize,
        seed: u64,
    ) -> VarResult {
        let mut sim_returns: Vec<f64> = Vec::with_capacity(simulations);
        let mut state = seed;

        let mut i = 0;
        while i < simulations {
            // LCG: constants from Numerical Recipes
            state = state
                .wrapping_mul(1_664_525)
                .wrapping_add(1_013_904_223);
            let u1 = (state as f64 + 0.5) / u64::MAX as f64;
            state = state
                .wrapping_mul(1_664_525)
                .wrapping_add(1_013_904_223);
            let u2 = (state as f64 + 0.5) / u64::MAX as f64;

            // Box-Muller transform
            let u1 = u1.clamp(1e-15, 1.0 - 1e-15);
            let u2 = u2.clamp(1e-15, 1.0 - 1e-15);
            let z0 = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
            let z1 = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).sin();

            sim_returns.push(mean + std_dev * z0);
            i += 1;
            if i < simulations {
                sim_returns.push(mean + std_dev * z1);
                i += 1;
            }
        }

        sim_returns.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let n = sim_returns.len();
        let var_idx = ((1.0 - confidence) * n as f64).floor() as usize;
        let var_idx = var_idx.min(n - 1);

        let var_pct = -sim_returns[var_idx].min(0.0);
        let var_abs = var_pct * portfolio_value;

        let cvar_pct = Self::cvar_from_var(&sim_returns, var_idx);
        let cvar_abs = cvar_pct * portfolio_value;

        VarResult {
            confidence_level: confidence,
            var_abs,
            var_pct,
            cvar_abs,
            cvar_pct,
            method: VarMethod::MonteCarlo { simulations, seed },
        }
    }

    /// Compute CVaR as the mean loss of the tail beyond the VaR index.
    ///
    /// `sorted_returns` must be sorted ascending. Returns the average negated
    /// return below index `var_idx` (exclusive), which is the expected shortfall.
    pub fn cvar_from_var(sorted_returns: &[f64], var_idx: usize) -> f64 {
        if var_idx == 0 {
            return -sorted_returns[0].min(0.0);
        }
        let tail = &sorted_returns[..var_idx];
        if tail.is_empty() {
            return 0.0;
        }
        let mean_tail: f64 = tail.iter().sum::<f64>() / tail.len() as f64;
        -mean_tail.min(0.0)
    }

    /// Rolling window VaR: slide a window of size `window` over `returns`.
    pub fn rolling_var(
        returns: &[f64],
        window: usize,
        confidence: f64,
        portfolio_value: f64,
    ) -> Vec<VarResult> {
        if window == 0 || returns.len() < window {
            return Vec::new();
        }
        returns
            .windows(window)
            .map(|w| Self::historical_var(w, confidence, portfolio_value))
            .collect()
    }

    // ── Private helpers ────────────────────────────────────────────────────────

    /// Cornish-Fisher z-score approximation for common confidence levels.
    fn cornish_fisher_z(confidence: f64) -> f64 {
        if confidence >= 0.99 {
            2.326
        } else if confidence >= 0.95 {
            1.645
        } else if confidence >= 0.90 {
            1.282
        } else {
            // Linear interpolation for other levels
            let alpha = 1.0 - confidence;
            // Rough approximation: z ≈ -ln(alpha * sqrt(2*pi)) for small alpha
            (-2.0 * (alpha * (2.0 * std::f64::consts::PI).sqrt()).ln()).sqrt()
        }
    }

    /// Standard normal PDF at x: (1/sqrt(2π)) * exp(-x²/2).
    fn standard_normal_pdf(x: f64) -> f64 {
        (-0.5 * x * x).exp() / (2.0 * std::f64::consts::PI).sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn historical_var_basic() {
        let returns: Vec<f64> = (-50i32..=50).map(|x| x as f64 / 1000.0).collect();
        let result = VarEngine::historical_var(&returns, 0.95, 100_000.0);
        assert!(result.var_abs > 0.0);
        assert!(result.cvar_abs >= result.var_abs);
        assert_eq!(result.confidence_level, 0.95);
    }

    #[test]
    fn parametric_var_99() {
        let result = VarEngine::parametric_var(0.0, 0.01, 0.99, 1_000_000.0, 1);
        // VaR at 99% with 1% daily vol on $1M should be ~$23,260
        assert!((result.var_abs - 23_260.0).abs() < 1000.0, "var_abs={}", result.var_abs);
    }

    #[test]
    fn monte_carlo_var_reasonable() {
        let result = VarEngine::monte_carlo_var(0.0, 0.01, 0.95, 1_000_000.0, 10_000, 42);
        assert!(result.var_abs > 10_000.0);
        assert!(result.var_abs < 30_000.0);
    }

    #[test]
    fn rolling_var_length() {
        let returns: Vec<f64> = (0..100).map(|i| i as f64 * 0.001 - 0.05).collect();
        let rolling = VarEngine::rolling_var(&returns, 20, 0.95, 100_000.0);
        assert_eq!(rolling.len(), 81);
    }

    #[test]
    fn portfolio_returns_weighted() {
        let pr = PortfolioReturns {
            returns: vec![0.01, 0.02, -0.01],
            weights: vec![0.5, 0.3, 0.2],
            portfolio_value: 100_000.0,
        };
        let r = pr.portfolio_returns();
        assert!((r[0] - 0.005).abs() < 1e-10);
        assert!((r[1] - 0.006).abs() < 1e-10);
        assert!((r[2] - (-0.002)).abs() < 1e-10);
    }
}
