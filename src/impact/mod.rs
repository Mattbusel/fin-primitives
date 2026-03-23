//! # Module: impact
//!
//! ## Responsibility
//! Almgren-Chriss optimal execution framework for minimising expected market
//! impact cost plus timing risk when liquidating (or acquiring) a large position.
//!
//! ## Model Summary
//! Given total shares X to trade over horizon T with N equal time steps:
//! - **Permanent impact**: γ·v  (linear in trade rate v, persists)
//! - **Temporary impact**: η·v  (linear in trade rate v, dissipates each step)
//! - **Timing risk**: variance of price path × risk-aversion λ
//!
//! The optimal trajectory minimises:
//!   E[cost] + λ · Var[cost]
//!
//! Closed-form solution (Almgren-Chriss 2001):
//!   x(τ) = X · sinh(κ(T-τ)) / sinh(κT)
//! where κ² = λσ² / η̃  and η̃ = η - γΔt/2.
//!
//! ## NOT Responsible For
//! - Dynamic/adaptive execution (static schedule only)
//! - Non-linear impact models
//! - Intraday liquidity constraints

use crate::error::FinError;

// ─── parameters ───────────────────────────────────────────────────────────────

/// Almgren-Chriss model parameters.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct AlmgrenChrissParams {
    /// Total shares (or notional units) to execute. Positive = buy, negative = sell.
    pub total_shares: f64,
    /// Execution horizon in units of time steps (e.g., seconds, minutes).
    pub time_steps: usize,
    /// Annualised (or per-step) price volatility σ.
    pub volatility: f64,
    /// Permanent impact coefficient γ (price shift per unit traded).
    pub permanent_impact: f64,
    /// Temporary impact coefficient η (instantaneous slippage per unit rate).
    pub temporary_impact: f64,
    /// Risk aversion parameter λ. Higher → trade faster to reduce timing risk.
    pub risk_aversion: f64,
}

// ─── trajectory ───────────────────────────────────────────────────────────────

/// A single step in the optimal execution trajectory.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct TrajectoryStep {
    /// Time step index (0 = now, N = horizon end).
    pub step: usize,
    /// Remaining inventory at this step (shares not yet traded).
    pub inventory: f64,
    /// Shares traded during this step (trade_size = inventory[t] - inventory[t+1]).
    pub trade_size: f64,
    /// Expected instantaneous market impact cost of this step's trade.
    pub impact_cost: f64,
}

/// The full optimal execution schedule.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OptimalTrajectory {
    /// Ordered sequence of execution steps.
    pub steps: Vec<TrajectoryStep>,
    /// Total expected permanent impact cost over the horizon.
    pub total_permanent_cost: f64,
    /// Total expected temporary impact cost over the horizon.
    pub total_temporary_cost: f64,
    /// Total expected execution cost (permanent + temporary).
    pub total_expected_cost: f64,
    /// Variance of the execution cost (before risk-aversion weighting).
    pub cost_variance: f64,
    /// Efficient frontier objective value: E[cost] + λ·Var[cost].
    pub objective: f64,
}

// ─── engine ───────────────────────────────────────────────────────────────────

/// Almgren-Chriss optimal execution engine.
pub struct AlmgrenChriss;

impl AlmgrenChriss {
    /// Compute the optimal execution trajectory for the given parameters.
    ///
    /// Returns an `OptimalTrajectory` with per-step inventory, trade sizes,
    /// and aggregated cost statistics.
    ///
    /// # Errors
    /// - `FinError::InvalidInput` if `time_steps == 0`, volatility ≤ 0, or
    ///   impact coefficients are negative.
    /// - `FinError::ArithmeticOverflow` on internal numeric failure (e.g. κ computation).
    pub fn compute(params: &AlmgrenChrissParams) -> Result<OptimalTrajectory, FinError> {
        Self::validate(params)?;

        let n = params.time_steps;
        let x = params.total_shares;
        let sigma = params.volatility;
        let gamma = params.permanent_impact;
        let eta = params.temporary_impact;
        let lambda = params.risk_aversion;

        // Time step size (normalised to 1 unless caller provides physical units)
        let dt = 1.0_f64;

        // Adjusted temporary impact (accounts for permanent impact bleed-in)
        let eta_tilde = eta - 0.5 * gamma * dt;
        // Protect against degenerate case where eta_tilde <= 0
        let eta_tilde = if eta_tilde <= 0.0 { eta } else { eta_tilde };

        // κ parameter: kappa^2 = lambda * sigma^2 / eta_tilde
        let kappa_sq = lambda * sigma * sigma / eta_tilde;
        if !kappa_sq.is_finite() || kappa_sq < 0.0 {
            return Err(FinError::ArithmeticOverflow);
        }
        let kappa = kappa_sq.sqrt();

        // Total horizon T = N * dt
        let big_t = n as f64 * dt;

        // Pre-compute sinh(kappa * T) for inventory formula
        let sinh_kt = (kappa * big_t).sinh();
        if sinh_kt.abs() < f64::EPSILON {
            // κ ≈ 0: risk-neutral case → TWAP (uniform liquidation)
            return Self::twap_fallback(params);
        }

        // Build inventory trajectory: x(t_j) = X * sinh(kappa*(T - t_j)) / sinh(kappa*T)
        let mut inventories = Vec::with_capacity(n + 1);
        for j in 0..=n {
            let tau = j as f64 * dt;
            let inv = x * (kappa * (big_t - tau)).sinh() / sinh_kt;
            inventories.push(inv);
        }

        // Compute per-step trade sizes and costs
        let mut steps = Vec::with_capacity(n);
        let mut total_perm = 0.0_f64;
        let mut total_temp = 0.0_f64;
        let mut cost_var_sum = 0.0_f64;

        for j in 0..n {
            let inv_now = inventories[j];
            let inv_next = inventories[j + 1];
            let trade = inv_now - inv_next; // shares sold this step
            let trade_rate = trade / dt;

            // Temporary impact cost: eta * (trade/dt)^2 * dt = eta * trade^2 / dt
            let temp_cost = eta * trade_rate * trade_rate * dt;
            // Permanent impact: gamma * trade_rate * dt * remaining_inv (cross term)
            // Simplified: perm cost contribution = 0.5 * gamma * trade^2
            let perm_cost = 0.5 * gamma * trade * trade;

            // Impact cost for this step (market impact paid)
            let impact_cost = temp_cost + perm_cost;

            // Variance contribution: sigma^2 * inv_now^2 * dt
            cost_var_sum += sigma * sigma * inv_now * inv_now * dt;

            total_perm += perm_cost;
            total_temp += temp_cost;

            steps.push(TrajectoryStep {
                step: j,
                inventory: inv_now,
                trade_size: trade,
                impact_cost,
            });
        }

        let total_cost = total_perm + total_temp;
        let objective = total_cost + lambda * cost_var_sum;

        Ok(OptimalTrajectory {
            steps,
            total_permanent_cost: total_perm,
            total_temporary_cost: total_temp,
            total_expected_cost: total_cost,
            cost_variance: cost_var_sum,
            objective,
        })
    }

    /// TWAP fallback for the risk-neutral (λ≈0 or κ≈0) case.
    fn twap_fallback(params: &AlmgrenChrissParams) -> Result<OptimalTrajectory, FinError> {
        let n = params.time_steps;
        let x = params.total_shares;
        let trade_per_step = x / n as f64;

        let mut steps = Vec::with_capacity(n);
        let mut total_perm = 0.0;
        let mut total_temp = 0.0;

        for j in 0..n {
            let inv = x - j as f64 * trade_per_step;
            let temp_cost = params.temporary_impact * trade_per_step * trade_per_step;
            let perm_cost = 0.5 * params.permanent_impact * trade_per_step * trade_per_step;
            total_perm += perm_cost;
            total_temp += temp_cost;
            steps.push(TrajectoryStep {
                step: j,
                inventory: inv,
                trade_size: trade_per_step,
                impact_cost: temp_cost + perm_cost,
            });
        }

        let total_cost = total_perm + total_temp;
        let cost_var = params.volatility * params.volatility * x * x;

        Ok(OptimalTrajectory {
            steps,
            total_permanent_cost: total_perm,
            total_temporary_cost: total_temp,
            total_expected_cost: total_cost,
            cost_variance: cost_var,
            objective: total_cost + params.risk_aversion * cost_var,
        })
    }

    fn validate(p: &AlmgrenChrissParams) -> Result<(), FinError> {
        if p.time_steps == 0 {
            return Err(FinError::InvalidInput("time_steps must be at least 1".to_owned()));
        }
        if p.volatility <= 0.0 {
            return Err(FinError::InvalidInput("volatility must be positive".to_owned()));
        }
        if p.permanent_impact < 0.0 {
            return Err(FinError::InvalidInput(
                "permanent_impact must be non-negative".to_owned(),
            ));
        }
        if p.temporary_impact < 0.0 {
            return Err(FinError::InvalidInput(
                "temporary_impact must be non-negative".to_owned(),
            ));
        }
        if p.risk_aversion < 0.0 {
            return Err(FinError::InvalidInput("risk_aversion must be non-negative".to_owned()));
        }
        Ok(())
    }
}

// ─── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn default_params() -> AlmgrenChrissParams {
        AlmgrenChrissParams {
            total_shares: 10_000.0,
            time_steps: 10,
            volatility: 0.02,
            permanent_impact: 1e-7,
            temporary_impact: 1e-6,
            risk_aversion: 1e-5,
        }
    }

    #[test]
    fn test_trajectory_has_correct_step_count() {
        let traj = AlmgrenChriss::compute(&default_params()).unwrap();
        assert_eq!(traj.steps.len(), 10);
    }

    #[test]
    fn test_first_step_inventory_is_total_shares() {
        let p = default_params();
        let traj = AlmgrenChriss::compute(&p).unwrap();
        let first_inv = traj.steps[0].inventory;
        assert!((first_inv - p.total_shares).abs() < 1.0, "first inventory: {first_inv}");
    }

    #[test]
    fn test_total_shares_roughly_traded() {
        let p = default_params();
        let traj = AlmgrenChriss::compute(&p).unwrap();
        let total_traded: f64 = traj.steps.iter().map(|s| s.trade_size).sum();
        assert!(
            (total_traded - p.total_shares).abs() < 1.0,
            "total traded {total_traded} vs {}", p.total_shares
        );
    }

    #[test]
    fn test_costs_positive() {
        let traj = AlmgrenChriss::compute(&default_params()).unwrap();
        assert!(traj.total_expected_cost > 0.0);
        assert!(traj.cost_variance > 0.0);
    }

    #[test]
    fn test_objective_equals_cost_plus_risk() {
        let p = default_params();
        let traj = AlmgrenChriss::compute(&p).unwrap();
        let expected_obj = traj.total_expected_cost + p.risk_aversion * traj.cost_variance;
        assert!((traj.objective - expected_obj).abs() < 1e-6);
    }

    #[test]
    fn test_invalid_params() {
        let mut p = default_params();
        p.time_steps = 0;
        assert!(AlmgrenChriss::compute(&p).is_err());

        p = default_params();
        p.volatility = -1.0;
        assert!(AlmgrenChriss::compute(&p).is_err());

        p = default_params();
        p.risk_aversion = -1.0;
        assert!(AlmgrenChriss::compute(&p).is_err());
    }

    #[test]
    fn test_twap_fallback_zero_risk_aversion() {
        let p = AlmgrenChrissParams {
            total_shares: 1000.0,
            time_steps: 5,
            volatility: 0.01,
            permanent_impact: 0.0,
            temporary_impact: 1e-6,
            risk_aversion: 0.0, // risk-neutral → approaches TWAP
        };
        let traj = AlmgrenChriss::compute(&p).unwrap();
        assert_eq!(traj.steps.len(), 5);
    }
}
