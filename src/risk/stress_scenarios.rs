//! Multi-factor stress testing with correlation shocks.
//!
//! Defines risk factors, stress scenarios, portfolio positions, and a
//! `StressTester` that applies named scenarios to compute P&L impacts.
//! Also provides a `CorrelationStressMatrix` for shocked portfolio VaR.

/// A single market risk factor with a shock expressed in basis points.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RiskFactor {
    /// Human-readable name, e.g. `"rate_usd_10y"` or `"equity_sp500"`.
    pub name: String,
    /// Current value of the factor (e.g. 0.04 for a 4 % rate).
    pub current_value: f64,
    /// Shock in basis points; can be negative.
    pub shock_bps: f64,
}

/// A named stress scenario: a collection of risk-factor shocks plus a
/// correlation multiplier applied to all off-diagonal entries.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StressScenario {
    /// Scenario identifier, e.g. `"2008 Financial Crisis"`.
    pub name: String,
    /// Narrative description.
    pub description: String,
    /// Factors and their shocks in this scenario.
    pub risk_factors: Vec<RiskFactor>,
    /// Multiplier applied to off-diagonal correlations (0 = zeroed, 1 = unchanged, >1 = amplified).
    pub correlation_shock: f64,
}

/// A single portfolio position.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PortfolioPosition {
    /// Unique identifier for the asset.
    pub asset_id: String,
    /// Current market value in currency units.
    pub market_value: f64,
    /// Modified duration (for fixed-income positions).
    pub duration: f64,
    /// Equity beta (for equity positions).
    pub beta: f64,
    /// Option delta (for derivatives).
    pub delta: f64,
}

/// Output of a single stress scenario run.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StressResult {
    /// Name of the scenario that produced this result.
    pub scenario_name: String,
    /// Aggregate portfolio P&L under the scenario.
    pub portfolio_pnl: f64,
    /// Asset ID of the position with the largest loss.
    pub worst_position: String,
    /// Loss amount for the worst position (negative number).
    pub worst_loss: f64,
    /// (factor_name, pnl_contribution) for every factor in the scenario.
    pub factor_contributions: Vec<(String, f64)>,
}

/// Stress testing engine: holds positions and scenarios, runs them on demand.
#[derive(Debug, Clone)]
pub struct StressTester {
    /// Portfolio positions to be stressed.
    pub positions: Vec<PortfolioPosition>,
    /// Registered stress scenarios.
    pub scenarios: Vec<StressScenario>,
}

impl StressTester {
    /// Create a new tester with the given positions and no scenarios.
    pub fn new(positions: Vec<PortfolioPosition>) -> Self {
        Self { positions, scenarios: Vec::new() }
    }

    /// Register a new stress scenario.
    pub fn add_scenario(&mut self, scenario: StressScenario) {
        self.scenarios.push(scenario);
    }

    /// Compute the P&L impact of a single risk-factor shock on one position.
    ///
    /// Dispatch rules (based on factor name):
    /// - contains `"rate"` → interest-rate DV01 loss
    /// - contains `"equity"` → beta-adjusted equity move
    /// - contains `"vol"` → delta-vega proxy
    /// - otherwise → proportional shock
    pub fn position_pnl(position: &PortfolioPosition, factor: &RiskFactor) -> f64 {
        let name = factor.name.to_lowercase();
        if name.contains("rate") {
            -position.duration * position.market_value * factor.shock_bps / 10_000.0
        } else if name.contains("equity") {
            position.beta * position.market_value * factor.shock_bps / 10_000.0
        } else if name.contains("vol") {
            position.delta * position.market_value * factor.shock_bps / 10_000.0 * 0.01
        } else {
            position.market_value * factor.shock_bps / 10_000.0
        }
    }

    /// Run a single scenario and return the aggregated result.
    pub fn run_scenario(&self, scenario: &StressScenario) -> StressResult {
        let mut portfolio_pnl = 0.0_f64;
        let mut factor_contributions: Vec<(String, f64)> = Vec::new();
        let mut worst_position = String::new();
        let mut worst_loss = 0.0_f64;

        // For each factor, accumulate P&L across all positions.
        for factor in &scenario.risk_factors {
            let mut factor_pnl = 0.0;
            for pos in &self.positions {
                let pnl = Self::position_pnl(pos, factor);
                factor_pnl += pnl;
                // Track worst individual position loss across all factors.
                if pnl < worst_loss {
                    worst_loss = pnl;
                    worst_position = pos.asset_id.clone();
                }
            }
            factor_contributions.push((factor.name.clone(), factor_pnl));
            portfolio_pnl += factor_pnl;
        }

        StressResult {
            scenario_name: scenario.name.clone(),
            portfolio_pnl,
            worst_position,
            worst_loss,
            factor_contributions,
        }
    }

    /// Run all registered scenarios and return their results.
    pub fn run_all(&self) -> Vec<StressResult> {
        self.scenarios.iter().map(|s| self.run_scenario(s)).collect()
    }

    /// Return a reference to the result with the minimum portfolio P&L, if any.
    ///
    /// Note: this runs all scenarios internally; the caller does not need to
    /// hold on to a separate `Vec<StressResult>`.
    pub fn worst_scenario(&self) -> Option<StressResult> {
        let results = self.run_all();
        results.into_iter().min_by(|a, b| {
            a.portfolio_pnl.partial_cmp(&b.portfolio_pnl).unwrap_or(std::cmp::Ordering::Equal)
        })
    }
}

// ─── Correlation stress matrix ────────────────────────────────────────────────

/// An n×n correlation matrix with a uniform off-diagonal shock multiplier.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CorrelationStressMatrix {
    /// Dimension of the matrix.
    pub n: usize,
    /// Flat row-major n×n base correlation values.
    pub base_correlations: Vec<f64>,
    /// Multiplier applied to all off-diagonal entries.
    pub shock_multiplier: f64,
}

/// Return the stressed correlation between assets `i` and `j`.
///
/// Diagonal entries are always 1.0. Off-diagonal entries are
/// `base * shock_multiplier`, clamped to `[−1, 1]`.
pub fn stressed_correlation(mat: &CorrelationStressMatrix, i: usize, j: usize) -> f64 {
    if i == j {
        return 1.0;
    }
    let base = mat.base_correlations[i * mat.n + j];
    (base * mat.shock_multiplier).clamp(-1.0, 1.0)
}

/// Portfolio volatility under the stressed correlation matrix.
///
/// Returns √(wᵀ Σ w) where Σ[i][j] = σ_i · σ_j · ρ_stressed(i, j).
pub fn portfolio_var_stressed(
    mat: &CorrelationStressMatrix,
    vols: &[f64],
    weights: &[f64],
) -> f64 {
    let n = mat.n;
    assert_eq!(vols.len(), n, "vols length must equal matrix dimension");
    assert_eq!(weights.len(), n, "weights length must equal matrix dimension");

    let mut variance = 0.0_f64;
    for i in 0..n {
        for j in 0..n {
            let rho = stressed_correlation(mat, i, j);
            variance += weights[i] * weights[j] * vols[i] * vols[j] * rho;
        }
    }
    variance.max(0.0).sqrt()
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn rate_position() -> PortfolioPosition {
        PortfolioPosition {
            asset_id: "bond_10y".to_string(),
            market_value: 1_000_000.0,
            duration: 8.0,
            beta: 0.0,
            delta: 0.0,
        }
    }

    fn equity_position() -> PortfolioPosition {
        PortfolioPosition {
            asset_id: "spy".to_string(),
            market_value: 500_000.0,
            duration: 0.0,
            beta: 1.2,
            delta: 0.0,
        }
    }

    #[test]
    fn rate_shock_reduces_duration_portfolio() {
        let pos = rate_position();
        let factor = RiskFactor {
            name: "rate_10y".to_string(),
            current_value: 0.04,
            shock_bps: 100.0, // +100 bps
        };
        // Expected: -8 * 1_000_000 * 0.01 = -80_000
        let pnl = StressTester::position_pnl(&pos, &factor);
        assert!((pnl - (-80_000.0)).abs() < 1e-6, "pnl={pnl}");
    }

    #[test]
    fn equity_shock_by_beta() {
        let pos = equity_position();
        let factor = RiskFactor {
            name: "equity_sp500".to_string(),
            current_value: 4500.0,
            shock_bps: -500.0, // -5 %
        };
        // Expected: 1.2 * 500_000 * (-500/10_000) = -30_000
        let pnl = StressTester::position_pnl(&pos, &factor);
        assert!((pnl - (-30_000.0)).abs() < 1e-6, "pnl={pnl}");
    }

    #[test]
    fn run_scenario_aggregates_factors() {
        let tester = StressTester::new(vec![rate_position(), equity_position()]);
        let scenario = StressScenario {
            name: "test".to_string(),
            description: "".to_string(),
            risk_factors: vec![
                RiskFactor { name: "rate_10y".to_string(), current_value: 0.04, shock_bps: 50.0 },
                RiskFactor {
                    name: "equity_sp500".to_string(),
                    current_value: 4500.0,
                    shock_bps: -200.0,
                },
            ],
            correlation_shock: 1.0,
        };
        let result = tester.run_scenario(&scenario);
        assert!(result.portfolio_pnl < 0.0, "combined shock should be negative");
        assert!(!result.worst_position.is_empty());
    }

    #[test]
    fn correlation_matrix_diagonal_is_one() {
        let mat = CorrelationStressMatrix {
            n: 3,
            base_correlations: vec![
                1.0, 0.5, 0.3, 0.5, 1.0, 0.4, 0.3, 0.4, 1.0,
            ],
            shock_multiplier: 2.0,
        };
        for i in 0..3 {
            assert_eq!(stressed_correlation(&mat, i, i), 1.0);
        }
    }

    #[test]
    fn stressed_correlation_off_diagonal_clamped() {
        let mat = CorrelationStressMatrix {
            n: 2,
            base_correlations: vec![1.0, 0.8, 0.8, 1.0],
            shock_multiplier: 2.0,
        };
        let c = stressed_correlation(&mat, 0, 1);
        // 0.8 * 2.0 = 1.6 → clamped to 1.0
        assert!((c - 1.0).abs() < 1e-10);
    }

    #[test]
    fn portfolio_var_increases_with_higher_correlation() {
        let n = 2;
        let vols = vec![0.2, 0.3];
        let weights = vec![0.5, 0.5];

        // Low correlation
        let mat_low = CorrelationStressMatrix {
            n,
            base_correlations: vec![1.0, 0.1, 0.1, 1.0],
            shock_multiplier: 1.0,
        };
        let var_low = portfolio_var_stressed(&mat_low, &vols, &weights);

        // High correlation
        let mat_high = CorrelationStressMatrix {
            n,
            base_correlations: vec![1.0, 0.9, 0.9, 1.0],
            shock_multiplier: 1.0,
        };
        let var_high = portfolio_var_stressed(&mat_high, &vols, &weights);

        assert!(var_high > var_low, "higher corr => higher VaR: {var_low} vs {var_high}");
    }

    #[test]
    fn worst_scenario_returns_most_negative() {
        let mut tester = StressTester::new(vec![rate_position()]);
        tester.add_scenario(StressScenario {
            name: "mild".to_string(),
            description: "".to_string(),
            risk_factors: vec![RiskFactor {
                name: "rate_10y".to_string(),
                current_value: 0.04,
                shock_bps: 10.0,
            }],
            correlation_shock: 1.0,
        });
        tester.add_scenario(StressScenario {
            name: "severe".to_string(),
            description: "".to_string(),
            risk_factors: vec![RiskFactor {
                name: "rate_10y".to_string(),
                current_value: 0.04,
                shock_bps: 300.0,
            }],
            correlation_shock: 1.5,
        });
        let worst = tester.worst_scenario().expect("should have result");
        assert_eq!(worst.scenario_name, "severe");
    }
}
