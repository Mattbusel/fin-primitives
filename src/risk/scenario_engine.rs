//! Scenario analysis and stress testing engine.
//!
//! Supports historical, hypothetical, Monte Carlo, and regulatory scenarios with
//! per-position shock application and portfolio-level P&L reporting.

/// Classification of how a scenario was constructed.
#[derive(Debug, Clone, PartialEq)]
pub enum ScenarioType {
    /// Historically observed episode identified by name (e.g. "GFC 2008").
    Historical(String),
    /// Analyst-constructed hypothetical scenario.
    Hypothetical,
    /// Monte Carlo simulation with reproducible seed and iteration count.
    MonteCarlo {
        /// Seed for the pseudo-random number generator.
        seed: u64,
        /// Number of simulation paths.
        simulations: usize,
    },
    /// Regulatory stress scenario (e.g. "DFAST Severely Adverse").
    Regulatory(String),
}

/// How a shock magnitude is applied to a position.
#[derive(Debug, Clone, PartialEq)]
pub enum ShockType {
    /// Add a fixed return to the position (e.g. −0.40 for a 40 % loss).
    AbsoluteReturn,
    /// Scale the position value by (1 + magnitude) (e.g. −0.40 for −40 %).
    RelativeReturn,
    /// Multiply the implied volatility by `magnitude`.
    VolatilityMultiplier,
    /// Shift pairwise correlations by `magnitude` (clamped to [−1, 1]).
    CorrelationShift,
}

/// A single market shock within a scenario.
#[derive(Debug, Clone)]
pub struct MarketShock {
    /// Name of the asset, sector, or risk factor being shocked.
    pub asset_or_factor: String,
    /// The type of shock applied.
    pub shock_type: ShockType,
    /// Numeric magnitude of the shock (interpretation depends on `shock_type`).
    pub magnitude: f64,
}

/// A complete stress scenario consisting of one or more correlated market shocks.
#[derive(Debug, Clone)]
pub struct Scenario {
    /// Short identifier for the scenario.
    pub name: String,
    /// Longer narrative description.
    pub description: String,
    /// How this scenario was derived.
    pub scenario_type: ScenarioType,
    /// Ordered list of shocks to apply.
    pub shocks: Vec<MarketShock>,
}

/// Output produced by running one scenario against a portfolio.
#[derive(Debug, Clone)]
pub struct ScenarioResult {
    /// Name of the scenario that produced this result.
    pub scenario_name: String,
    /// Absolute portfolio P&L under the scenario (USD or base currency).
    pub portfolio_pnl: f64,
    /// Portfolio P&L as a percentage of total portfolio value.
    pub portfolio_pnl_pct: f64,
    /// Position with the largest loss: (symbol, pnl).
    pub worst_position: (String, f64),
    /// Position with the largest gain: (symbol, pnl).
    pub best_position: (String, f64),
    /// Change in portfolio VaR implied by the scenario.
    pub var_impact: f64,
}

/// Scenario analysis and stress-testing engine.
#[derive(Debug, Clone, Default)]
pub struct ScenarioEngine;

impl ScenarioEngine {
    /// Apply a single `MarketShock` to a position value and return the resulting P&L.
    ///
    /// - `AbsoluteReturn`: P&L = `position_value * magnitude`
    /// - `RelativeReturn`: P&L = `position_value * magnitude`  (same formula, semantically % change)
    /// - `VolatilityMultiplier`: approximated as vol expansion impact = `position_value * 0.5 * (magnitude - 1.0) * 0.2`
    /// - `CorrelationShift`: approximated as `position_value * 0.1 * magnitude`
    pub fn apply_shock(position_value: f64, shock: &MarketShock) -> f64 {
        match shock.shock_type {
            ShockType::AbsoluteReturn => position_value * shock.magnitude,
            ShockType::RelativeReturn => position_value * shock.magnitude,
            ShockType::VolatilityMultiplier => {
                position_value * 0.5 * (shock.magnitude - 1.0) * 0.2
            }
            ShockType::CorrelationShift => position_value * 0.1 * shock.magnitude,
        }
    }

    /// Run a single scenario against a portfolio of positions.
    ///
    /// `positions` is a slice of `(symbol, market_value, beta)` tuples.
    /// Each position's P&L is computed by applying the first shock that matches
    /// its symbol; if none matches, a market beta-scaled version of the first
    /// market shock is used as a fallback.
    pub fn run_scenario(
        &self,
        scenario: &Scenario,
        positions: &[(String, f64, f64)],
    ) -> ScenarioResult {
        if positions.is_empty() || scenario.shocks.is_empty() {
            return ScenarioResult {
                scenario_name: scenario.name.clone(),
                portfolio_pnl: 0.0,
                portfolio_pnl_pct: 0.0,
                worst_position: (String::new(), 0.0),
                best_position: (String::new(), 0.0),
                var_impact: 0.0,
            };
        }

        // Default market shock (first AbsoluteReturn or RelativeReturn shock, else first shock).
        let market_shock = scenario
            .shocks
            .iter()
            .find(|s| {
                s.shock_type == ShockType::AbsoluteReturn
                    || s.shock_type == ShockType::RelativeReturn
            })
            .unwrap_or(&scenario.shocks[0]);

        let mut position_pnls: Vec<(String, f64)> = Vec::with_capacity(positions.len());
        let mut total_value = 0.0_f64;

        for (symbol, value, beta) in positions.iter() {
            total_value += value.abs();

            // Find a symbol-specific shock first.
            let pnl = if let Some(shock) = scenario
                .shocks
                .iter()
                .find(|s| s.asset_or_factor.eq_ignore_ascii_case(symbol))
            {
                Self::apply_shock(*value, shock)
            } else {
                // Scale market shock by beta.
                let effective_magnitude = market_shock.magnitude * beta;
                let scaled_shock = MarketShock {
                    asset_or_factor: symbol.clone(),
                    shock_type: market_shock.shock_type.clone(),
                    magnitude: effective_magnitude,
                };
                Self::apply_shock(*value, &scaled_shock)
            };

            position_pnls.push((symbol.clone(), pnl));
        }

        let portfolio_pnl: f64 = position_pnls.iter().map(|(_, p)| p).sum();
        let portfolio_pnl_pct = if total_value < 1e-14 {
            0.0
        } else {
            portfolio_pnl / total_value * 100.0
        };

        let worst_position = position_pnls
            .iter()
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .cloned()
            .unwrap_or_default();

        let best_position = position_pnls
            .iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .cloned()
            .unwrap_or_default();

        // Simple VaR impact: 1.65 * |portfolio_pnl| / sqrt(252).
        let var_impact = 1.65 * portfolio_pnl.abs() / 252.0_f64.sqrt();

        ScenarioResult {
            scenario_name: scenario.name.clone(),
            portfolio_pnl,
            portfolio_pnl_pct,
            worst_position,
            best_position,
            var_impact,
        }
    }

    /// Return a set of pre-loaded historical stress scenarios.
    pub fn historical_scenarios() -> Vec<Scenario> {
        vec![
            Scenario {
                name: "GFC 2008".to_string(),
                description: "Global Financial Crisis — Lehman Brothers collapse, Oct 2008".to_string(),
                scenario_type: ScenarioType::Historical("2008-10".to_string()),
                shocks: vec![
                    MarketShock {
                        asset_or_factor: "EQUITY".to_string(),
                        shock_type: ShockType::RelativeReturn,
                        magnitude: -0.40,
                    },
                    MarketShock {
                        asset_or_factor: "CREDIT".to_string(),
                        shock_type: ShockType::AbsoluteReturn,
                        magnitude: -0.25,
                    },
                    MarketShock {
                        asset_or_factor: "VOLATILITY".to_string(),
                        shock_type: ShockType::VolatilityMultiplier,
                        magnitude: 4.0,
                    },
                ],
            },
            Scenario {
                name: "COVID-19 2020".to_string(),
                description: "COVID-19 pandemic market crash — Feb/Mar 2020".to_string(),
                scenario_type: ScenarioType::Historical("2020-03".to_string()),
                shocks: vec![
                    MarketShock {
                        asset_or_factor: "EQUITY".to_string(),
                        shock_type: ShockType::RelativeReturn,
                        magnitude: -0.34,
                    },
                    MarketShock {
                        asset_or_factor: "OIL".to_string(),
                        shock_type: ShockType::RelativeReturn,
                        magnitude: -0.60,
                    },
                    MarketShock {
                        asset_or_factor: "VOLATILITY".to_string(),
                        shock_type: ShockType::VolatilityMultiplier,
                        magnitude: 5.0,
                    },
                ],
            },
            Scenario {
                name: "Dot-com 2000".to_string(),
                description: "Technology bubble burst — NASDAQ peak-to-trough 2000-2002".to_string(),
                scenario_type: ScenarioType::Historical("2000-03".to_string()),
                shocks: vec![
                    MarketShock {
                        asset_or_factor: "TECH".to_string(),
                        shock_type: ShockType::RelativeReturn,
                        magnitude: -0.78,
                    },
                    MarketShock {
                        asset_or_factor: "EQUITY".to_string(),
                        shock_type: ShockType::RelativeReturn,
                        magnitude: -0.49,
                    },
                ],
            },
            Scenario {
                name: "Black Monday 1987".to_string(),
                description: "Single-day 22 % equity crash — 19 October 1987".to_string(),
                scenario_type: ScenarioType::Historical("1987-10-19".to_string()),
                shocks: vec![MarketShock {
                    asset_or_factor: "EQUITY".to_string(),
                    shock_type: ShockType::RelativeReturn,
                    magnitude: -0.22,
                }],
            },
            Scenario {
                name: "Bond Crash 1994".to_string(),
                description: "Fed rate hike cycle triggers global bond sell-off — 1994".to_string(),
                scenario_type: ScenarioType::Historical("1994".to_string()),
                shocks: vec![
                    MarketShock {
                        asset_or_factor: "BONDS".to_string(),
                        shock_type: ShockType::RelativeReturn,
                        magnitude: -0.08,
                    },
                    MarketShock {
                        asset_or_factor: "RATES".to_string(),
                        shock_type: ShockType::AbsoluteReturn,
                        magnitude: 0.025, // +250 bps
                    },
                ],
            },
            Scenario {
                name: "EUR Crisis 2011".to_string(),
                description: "European sovereign debt crisis — peripheral spreads blow out, 2011".to_string(),
                scenario_type: ScenarioType::Historical("2011".to_string()),
                shocks: vec![
                    MarketShock {
                        asset_or_factor: "EUR_EQUITY".to_string(),
                        shock_type: ShockType::RelativeReturn,
                        magnitude: -0.20,
                    },
                    MarketShock {
                        asset_or_factor: "PERIPHERAL_BONDS".to_string(),
                        shock_type: ShockType::AbsoluteReturn,
                        magnitude: -0.15,
                    },
                    MarketShock {
                        asset_or_factor: "EUR".to_string(),
                        shock_type: ShockType::RelativeReturn,
                        magnitude: -0.12,
                    },
                ],
            },
        ]
    }

    /// Return a set of regulatory stress scenarios (DFAST / CCAR-style).
    pub fn regulatory_scenarios() -> Vec<Scenario> {
        vec![
            Scenario {
                name: "DFAST Severely Adverse".to_string(),
                description: "DFAST/CCAR severely adverse scenario: deep recession, high unemployment".to_string(),
                scenario_type: ScenarioType::Regulatory("DFAST".to_string()),
                shocks: vec![
                    MarketShock {
                        asset_or_factor: "EQUITY".to_string(),
                        shock_type: ShockType::RelativeReturn,
                        magnitude: -0.50,
                    },
                    MarketShock {
                        asset_or_factor: "RATES".to_string(),
                        shock_type: ShockType::AbsoluteReturn,
                        magnitude: -0.015,
                    },
                    MarketShock {
                        asset_or_factor: "CREDIT_SPREAD".to_string(),
                        shock_type: ShockType::AbsoluteReturn,
                        magnitude: 0.05,
                    },
                    MarketShock {
                        asset_or_factor: "REAL_ESTATE".to_string(),
                        shock_type: ShockType::RelativeReturn,
                        magnitude: -0.25,
                    },
                ],
            },
            Scenario {
                name: "CCAR Adverse".to_string(),
                description: "CCAR adverse scenario: moderate recession, elevated but not extreme stress".to_string(),
                scenario_type: ScenarioType::Regulatory("CCAR".to_string()),
                shocks: vec![
                    MarketShock {
                        asset_or_factor: "EQUITY".to_string(),
                        shock_type: ShockType::RelativeReturn,
                        magnitude: -0.30,
                    },
                    MarketShock {
                        asset_or_factor: "RATES".to_string(),
                        shock_type: ShockType::AbsoluteReturn,
                        magnitude: -0.01,
                    },
                    MarketShock {
                        asset_or_factor: "CREDIT_SPREAD".to_string(),
                        shock_type: ShockType::AbsoluteReturn,
                        magnitude: 0.02,
                    },
                ],
            },
            Scenario {
                name: "CCAR Baseline".to_string(),
                description: "CCAR baseline scenario: moderate economic growth, stable rates".to_string(),
                scenario_type: ScenarioType::Regulatory("CCAR".to_string()),
                shocks: vec![
                    MarketShock {
                        asset_or_factor: "EQUITY".to_string(),
                        shock_type: ShockType::RelativeReturn,
                        magnitude: 0.05,
                    },
                    MarketShock {
                        asset_or_factor: "RATES".to_string(),
                        shock_type: ShockType::AbsoluteReturn,
                        magnitude: 0.005,
                    },
                ],
            },
        ]
    }

    /// Run all historical and regulatory scenarios against the portfolio.
    pub fn run_all(&self, positions: &[(String, f64, f64)]) -> Vec<ScenarioResult> {
        let mut all_scenarios = Self::historical_scenarios();
        all_scenarios.extend(Self::regulatory_scenarios());
        all_scenarios
            .iter()
            .map(|s| self.run_scenario(s, positions))
            .collect()
    }

    /// Generate a Monte Carlo portfolio P&L distribution using a simple
    /// correlated log-normal model.
    ///
    /// Uses a linear congruential generator for reproducibility.
    pub fn monte_carlo_scenarios(
        &self,
        positions: &[(String, f64, f64)],
        n: usize,
        vol: f64,
        corr: f64,
        seed: u64,
    ) -> Vec<f64> {
        let total_value: f64 = positions.iter().map(|(_, v, _)| v.abs()).sum();
        if total_value < 1e-14 || n == 0 {
            return vec![0.0; n];
        }

        // LCG parameters (Numerical Recipes).
        let mut state = seed.wrapping_add(1);
        let lcg_next = |s: &mut u64| -> f64 {
            *s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            // Box-Muller (half-step — use two calls per normal).
            *s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let u1 = (*s >> 11) as f64 / (1u64 << 53) as f64;
            *s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let u2 = (*s >> 11) as f64 / (1u64 << 53) as f64;
            let u1 = u1.max(1e-12);
            (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
        };

        (0..n)
            .map(|_| {
                // Market factor shock.
                let market_z = lcg_next(&mut state);
                let port_pnl: f64 = positions
                    .iter()
                    .map(|(_, value, beta)| {
                        let idio_z = lcg_next(&mut state);
                        let ret = vol
                            * (corr.sqrt() * beta * market_z
                                + (1.0 - corr).sqrt() * idio_z);
                        value * ret
                    })
                    .sum();
                port_pnl
            })
            .collect()
    }

    /// Compute VaR and CVaR from a sorted P&L distribution at the given confidence level.
    ///
    /// Returns `(VaR, CVaR)` where VaR is the loss exceeded with probability `1 - confidence`.
    /// Both are returned as positive numbers representing losses.
    pub fn tail_scenarios(&self, results: &[f64], confidence: f64) -> (f64, f64) {
        if results.is_empty() {
            return (0.0, 0.0);
        }
        let mut sorted = results.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let idx = ((1.0 - confidence) * sorted.len() as f64) as usize;
        let idx = idx.min(sorted.len() - 1);

        let var = -sorted[idx]; // positive loss

        let cvar_slice = &sorted[..=idx];
        let cvar = if cvar_slice.is_empty() {
            var
        } else {
            -cvar_slice.iter().sum::<f64>() / cvar_slice.len() as f64
        };

        (var, cvar)
    }

    /// Format a human-readable stress-test report from a list of scenario results.
    pub fn scenario_report(&self, results: &[ScenarioResult]) -> String {
        let mut out = String::from("=== Scenario Analysis Report ===\n\n");
        for r in results {
            out.push_str(&format!(
                "Scenario : {}\n\
                 P&L      : {:.2} ({:.2} %)\n\
                 Worst    : {} = {:.2}\n\
                 Best     : {} = {:.2}\n\
                 VaR Δ    : {:.2}\n\
                 ---\n",
                r.scenario_name,
                r.portfolio_pnl,
                r.portfolio_pnl_pct,
                r.worst_position.0,
                r.worst_position.1,
                r.best_position.0,
                r.best_position.1,
                r.var_impact,
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_positions() -> Vec<(String, f64, f64)> {
        vec![
            ("AAPL".to_string(), 100_000.0, 1.2),
            ("MSFT".to_string(), 80_000.0, 1.0),
            ("TLT".to_string(), 50_000.0, -0.3),
        ]
    }

    #[test]
    fn test_historical_scenarios_count() {
        assert_eq!(ScenarioEngine::historical_scenarios().len(), 6);
    }

    #[test]
    fn test_regulatory_scenarios_count() {
        assert_eq!(ScenarioEngine::regulatory_scenarios().len(), 3);
    }

    #[test]
    fn test_run_scenario_gfc() {
        let engine = ScenarioEngine::default();
        let scenarios = ScenarioEngine::historical_scenarios();
        let gfc = scenarios.iter().find(|s| s.name == "GFC 2008").expect("GFC scenario");
        let result = engine.run_scenario(gfc, &sample_positions());
        assert!(result.portfolio_pnl < 0.0, "GFC should produce a loss");
    }

    #[test]
    fn test_run_all_count() {
        let engine = ScenarioEngine::default();
        let results = engine.run_all(&sample_positions());
        assert_eq!(results.len(), 9); // 6 historical + 3 regulatory
    }

    #[test]
    fn test_monte_carlo_len() {
        let engine = ScenarioEngine::default();
        let pnls = engine.monte_carlo_scenarios(&sample_positions(), 1000, 0.01, 0.6, 42);
        assert_eq!(pnls.len(), 1000);
    }

    #[test]
    fn test_tail_scenarios() {
        let engine = ScenarioEngine::default();
        let pnls: Vec<f64> = engine.monte_carlo_scenarios(&sample_positions(), 10_000, 0.01, 0.6, 7);
        let (var, cvar) = engine.tail_scenarios(&pnls, 0.95);
        assert!(var >= 0.0);
        assert!(cvar >= var, "CVaR should be >= VaR");
    }

    #[test]
    fn test_report_non_empty() {
        let engine = ScenarioEngine::default();
        let results = engine.run_all(&sample_positions());
        let report = engine.scenario_report(&results);
        assert!(report.contains("Scenario Analysis Report"));
    }
}
