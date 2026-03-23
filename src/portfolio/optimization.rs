//! Mean-variance portfolio optimization.
//!
//! Implements gradient-based mean-variance optimization supporting:
//! - Maximum Sharpe ratio via gradient ascent
//! - Minimum variance subject to a target return
//! - Efficient frontier generation by sweeping target returns

/// A single investable asset with expected return and current weight.
#[derive(Debug, Clone)]
pub struct Asset {
    /// Asset name or ticker.
    pub name: String,
    /// Annualized expected return (e.g. 0.08 = 8%).
    pub expected_return: f64,
    /// Current portfolio weight (0.0 – 1.0).
    pub weight: f64,
}

/// Portfolio optimization constraint.
#[derive(Debug, Clone)]
pub enum OptimizationConstraint {
    /// Weights must sum to one.
    SumToOne,
    /// All weights must be non-negative (long-only).
    LongOnly,
    /// No single weight may exceed this value.
    MaxWeight(f64),
    /// Every weight must be at least this value.
    MinWeight(f64),
    /// Maximum concentration: single-asset weight ceiling.
    MaxConcentration(f64),
}

/// A single point on the efficient frontier.
#[derive(Debug, Clone)]
pub struct EfficientFrontierPoint {
    /// Expected portfolio return.
    pub expected_return: f64,
    /// Portfolio volatility (annualized standard deviation).
    pub volatility: f64,
    /// Sharpe ratio at this point.
    pub sharpe: f64,
    /// Asset weights at this frontier point.
    pub weights: Vec<f64>,
}

/// Result of a portfolio optimization run.
#[derive(Debug, Clone)]
pub struct OptimizationResult {
    /// Optimal asset weights.
    pub weights: Vec<f64>,
    /// Expected portfolio return.
    pub expected_return: f64,
    /// Portfolio volatility.
    pub volatility: f64,
    /// Sharpe ratio of the optimized portfolio.
    pub sharpe: f64,
    /// Number of gradient iterations taken.
    pub iterations: usize,
}

/// Mean-variance portfolio optimizer using gradient descent.
pub struct MeanVarianceOptimizer;

impl MeanVarianceOptimizer {
    /// Compute expected portfolio return: dot product of weights and returns.
    pub fn portfolio_return(weights: &[f64], returns: &[f64]) -> f64 {
        weights.iter().zip(returns.iter()).map(|(w, r)| w * r).sum()
    }

    /// Compute portfolio variance: w^T Σ w.
    pub fn portfolio_variance(weights: &[f64], cov: &[Vec<f64>]) -> f64 {
        let n = weights.len();
        let mut var = 0.0_f64;
        for i in 0..n {
            for j in 0..n {
                var += weights[i] * weights[j] * cov[i][j];
            }
        }
        var
    }

    /// Compute portfolio volatility: sqrt(w^T Σ w).
    pub fn portfolio_volatility(weights: &[f64], cov: &[Vec<f64>]) -> f64 {
        Self::portfolio_variance(weights, cov).max(0.0).sqrt()
    }

    /// Perform one gradient step minimizing variance subject to a target return.
    ///
    /// Uses the gradient of `Var(w) - lambda * (Return(w) - target)` and projects
    /// onto the unit simplex afterwards.
    pub fn gradient_step(
        weights: &mut Vec<f64>,
        returns: &[f64],
        cov: &[Vec<f64>],
        target_return: f64,
        lr: f64,
    ) {
        let n = weights.len();
        // gradient of variance w.r.t. weights: 2 * Σ w
        let mut grad_var = vec![0.0_f64; n];
        for i in 0..n {
            for j in 0..n {
                grad_var[i] += 2.0 * cov[i][j] * weights[j];
            }
        }
        // Lagrange multiplier for return constraint
        let actual_return = Self::portfolio_return(weights, returns);
        let lambda = 2.0 * (actual_return - target_return);

        // Combined gradient: variance gradient - lambda * return gradient
        for i in 0..n {
            weights[i] -= lr * (grad_var[i] - lambda * returns[i]);
        }
    }

    /// Apply constraints to a weight vector (projection onto constraint set).
    pub fn apply_constraints(weights: &mut Vec<f64>, constraints: &[OptimizationConstraint]) {
        let n = weights.len();
        for c in constraints {
            match c {
                OptimizationConstraint::LongOnly => {
                    for w in weights.iter_mut() {
                        if *w < 0.0 {
                            *w = 0.0;
                        }
                    }
                }
                OptimizationConstraint::MaxWeight(max) => {
                    for w in weights.iter_mut() {
                        if *w > *max {
                            *w = *max;
                        }
                    }
                }
                OptimizationConstraint::MinWeight(min) => {
                    for w in weights.iter_mut() {
                        if *w < *min {
                            *w = *min;
                        }
                    }
                }
                OptimizationConstraint::MaxConcentration(max_conc) => {
                    for w in weights.iter_mut() {
                        if *w > *max_conc {
                            *w = *max_conc;
                        }
                    }
                }
                OptimizationConstraint::SumToOne => {
                    let sum: f64 = weights.iter().sum();
                    if sum.abs() > 1e-12 {
                        for w in weights.iter_mut() {
                            *w /= sum;
                        }
                    } else {
                        // Fallback: equal weights
                        let eq = 1.0 / n as f64;
                        for w in weights.iter_mut() {
                            *w = eq;
                        }
                    }
                }
            }
        }
    }

    /// Maximize Sharpe ratio via gradient ascent.
    ///
    /// Iterates gradient ascent on the Sharpe ratio objective:
    /// `(Return(w) - rf) / Volatility(w)`.
    pub fn maximize_sharpe(
        returns: &[f64],
        cov: &[Vec<f64>],
        rf_rate: f64,
        constraints: &[OptimizationConstraint],
    ) -> OptimizationResult {
        let n = returns.len();
        if n == 0 {
            return OptimizationResult {
                weights: vec![],
                expected_return: 0.0,
                volatility: 0.0,
                sharpe: 0.0,
                iterations: 0,
            };
        }

        let mut weights = vec![1.0 / n as f64; n];
        Self::apply_constraints(&mut weights, constraints);

        let max_iter = 1000_usize;
        let lr = 0.01_f64;
        let tol = 1e-8_f64;
        let mut prev_sharpe = f64::NEG_INFINITY;

        let mut iters = 0_usize;
        for iter in 0..max_iter {
            iters = iter + 1;
            let ret = Self::portfolio_return(&weights, returns);
            let vol = Self::portfolio_volatility(&weights, cov);

            if vol < 1e-12 {
                break;
            }

            let excess = ret - rf_rate;
            let sharpe = excess / vol;

            // Gradient of Sharpe w.r.t. weights
            // d(Sharpe)/dw_i = (r_i * vol - excess * dVol/dw_i) / vol^2
            // dVol/dw_i = (Σ w)_i / vol  = sum_j(cov[i][j]*w[j]) / vol
            let mut grad = vec![0.0_f64; n];
            for i in 0..n {
                let dcov_i: f64 = (0..n).map(|j| cov[i][j] * weights[j]).sum();
                let dvol_i = dcov_i / vol;
                grad[i] = (returns[i] * vol - excess * dvol_i) / (vol * vol);
            }

            for i in 0..n {
                weights[i] += lr * grad[i];
            }

            Self::apply_constraints(&mut weights, constraints);

            if (sharpe - prev_sharpe).abs() < tol {
                break;
            }
            prev_sharpe = sharpe;
        }

        let ret = Self::portfolio_return(&weights, returns);
        let vol = Self::portfolio_volatility(&weights, cov);
        let sharpe = if vol > 1e-12 { (ret - rf_rate) / vol } else { 0.0 };

        OptimizationResult {
            weights,
            expected_return: ret,
            volatility: vol,
            sharpe,
            iterations: iters,
        }
    }

    /// Minimize portfolio variance subject to a target return.
    pub fn minimize_variance(
        returns: &[f64],
        cov: &[Vec<f64>],
        target_return: f64,
        constraints: &[OptimizationConstraint],
    ) -> OptimizationResult {
        let n = returns.len();
        if n == 0 {
            return OptimizationResult {
                weights: vec![],
                expected_return: 0.0,
                volatility: 0.0,
                sharpe: 0.0,
                iterations: 0,
            };
        }

        let mut weights = vec![1.0 / n as f64; n];
        Self::apply_constraints(&mut weights, constraints);

        let max_iter = 2000_usize;
        let lr = 0.005_f64;
        let tol = 1e-10_f64;
        let mut prev_var = f64::MAX;

        let mut iters = 0_usize;
        for iter in 0..max_iter {
            iters = iter + 1;
            Self::gradient_step(&mut weights, returns, cov, target_return, lr);
            Self::apply_constraints(&mut weights, constraints);

            let var = Self::portfolio_variance(&weights, cov);
            if (var - prev_var).abs() < tol {
                break;
            }
            prev_var = var;
        }

        let ret = Self::portfolio_return(&weights, returns);
        let vol = Self::portfolio_volatility(&weights, cov);
        let sharpe = if vol > 1e-12 { ret / vol } else { 0.0 };

        OptimizationResult {
            weights,
            expected_return: ret,
            volatility: vol,
            sharpe,
            iterations: iters,
        }
    }

    /// Generate the efficient frontier by sweeping target returns.
    ///
    /// Produces `n_points` `EfficientFrontierPoint` values spanning the range
    /// from the minimum expected return to the maximum expected return.
    pub fn efficient_frontier(
        returns: &[f64],
        cov: &[Vec<f64>],
        n_points: usize,
        rf_rate: f64,
    ) -> Vec<EfficientFrontierPoint> {
        if returns.is_empty() || n_points == 0 {
            return vec![];
        }

        let min_ret = returns.iter().cloned().fold(f64::MAX, f64::min);
        let max_ret = returns.iter().cloned().fold(f64::MIN, f64::max);

        if (max_ret - min_ret).abs() < 1e-12 {
            return vec![];
        }

        let constraints = &[OptimizationConstraint::SumToOne, OptimizationConstraint::LongOnly];
        let mut frontier = Vec::with_capacity(n_points);

        for k in 0..n_points {
            let t = k as f64 / (n_points - 1).max(1) as f64;
            let target = min_ret + t * (max_ret - min_ret);
            let result = Self::minimize_variance(returns, cov, target, constraints);
            let sharpe = if result.volatility > 1e-12 {
                (result.expected_return - rf_rate) / result.volatility
            } else {
                0.0
            };
            frontier.push(EfficientFrontierPoint {
                expected_return: result.expected_return,
                volatility: result.volatility,
                sharpe,
                weights: result.weights,
            });
        }

        frontier
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simple_cov() -> Vec<Vec<f64>> {
        vec![
            vec![0.04, 0.006],
            vec![0.006, 0.09],
        ]
    }

    #[test]
    fn portfolio_return_correct() {
        let weights = vec![0.6, 0.4];
        let returns = vec![0.10, 0.08];
        let r = MeanVarianceOptimizer::portfolio_return(&weights, &returns);
        assert!((r - 0.092).abs() < 1e-9);
    }

    #[test]
    fn portfolio_variance_correct() {
        let weights = vec![1.0, 0.0];
        let cov = simple_cov();
        let v = MeanVarianceOptimizer::portfolio_variance(&weights, &cov);
        assert!((v - 0.04).abs() < 1e-9);
    }

    #[test]
    fn apply_sum_to_one() {
        let mut w = vec![2.0, 3.0];
        MeanVarianceOptimizer::apply_constraints(&mut w, &[OptimizationConstraint::SumToOne]);
        assert!((w[0] - 0.4).abs() < 1e-9);
        assert!((w[1] - 0.6).abs() < 1e-9);
    }

    #[test]
    fn maximize_sharpe_runs() {
        let returns = vec![0.10, 0.08, 0.12];
        let cov = vec![
            vec![0.04, 0.006, 0.002],
            vec![0.006, 0.09, 0.003],
            vec![0.002, 0.003, 0.16],
        ];
        let constraints = [OptimizationConstraint::SumToOne, OptimizationConstraint::LongOnly];
        let result = MeanVarianceOptimizer::maximize_sharpe(&returns, &cov, 0.02, &constraints);
        let sum: f64 = result.weights.iter().sum();
        assert!((sum - 1.0).abs() < 1e-6);
        assert!(result.weights.iter().all(|&w| w >= -1e-9));
    }

    #[test]
    fn efficient_frontier_has_correct_length() {
        let returns = vec![0.05, 0.10];
        let cov = simple_cov();
        let frontier = MeanVarianceOptimizer::efficient_frontier(&returns, &cov, 5, 0.02);
        assert_eq!(frontier.len(), 5);
    }
}
