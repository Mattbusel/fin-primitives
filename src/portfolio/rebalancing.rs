//! Portfolio rebalancing strategies and execution planning.
//!
//! Provides trigger logic ([`RebalancingTrigger`]), drift analytics
//! ([`HoldingDrift`]), plan generation ([`RebalancingPlan`]), and the
//! [`RebalancingEngine`] that ties everything together.

use std::collections::HashMap;

// ─────────────────────────────────────────────────────────────────────────────
//  Enums and supporting structs
// ─────────────────────────────────────────────────────────────────────────────

/// Defines when a rebalance should be triggered.
#[derive(Debug, Clone)]
pub enum RebalancingTrigger {
    /// Rebalance on a fixed calendar schedule.
    Calendar {
        /// Number of calendar days between rebalances.
        frequency_days: u32,
    },
    /// Rebalance when any holding drifts beyond `drift_pct` from target.
    Threshold {
        /// Maximum allowable drift as a fraction (e.g. 0.05 = 5 %).
        drift_pct: f64,
    },
    /// Tolerance-band approach: act only when the outer band is breached.
    BandBased {
        /// Inner tolerance (no action below this).
        inner_band: f64,
        /// Outer tolerance (always rebalance when exceeded).
        outer_band: f64,
    },
    /// Rebalance when either the calendar or threshold condition is met.
    Hybrid {
        /// Maximum days between forced rebalances.
        max_days: u32,
        /// Drift threshold that also triggers a rebalance.
        threshold_pct: f64,
    },
}

/// Estimated costs of executing a rebalancing plan.
#[derive(Debug, Clone, Default)]
pub struct RebalancingCost {
    /// Bid-ask spread and broker commission costs.
    pub transaction_costs: f64,
    /// Market impact estimate.
    pub market_impact: f64,
    /// Tax drag from realising gains.
    pub tax_cost: f64,
    /// Sum of all cost components.
    pub total: f64,
}

/// Drift metrics for a single holding.
#[derive(Debug, Clone)]
pub struct HoldingDrift {
    /// Asset symbol or identifier.
    pub symbol: String,
    /// Target portfolio weight (0–1).
    pub target_weight: f64,
    /// Current portfolio weight (0–1).
    pub current_weight: f64,
    /// Signed drift: `current_weight - target_weight`.
    pub drift_pct: f64,
    /// Whether this holding alone would trigger a threshold rebalance.
    pub requires_rebalance: bool,
}

/// A concrete plan for rebalancing a portfolio.
#[derive(Debug, Clone)]
pub struct RebalancingPlan {
    /// Trades to execute: `(symbol, notional_amount, is_buy)`.
    pub trades: Vec<(String, f64, bool)>,
    /// Total turnover as a fraction of portfolio value.
    pub total_turnover: f64,
    /// Estimated execution costs.
    pub estimated_cost: RebalancingCost,
    /// Human-readable reason the plan was generated.
    pub trigger_reason: String,
}

// ─────────────────────────────────────────────────────────────────────────────
//  RebalancingEngine
// ─────────────────────────────────────────────────────────────────────────────

/// Stateless engine that generates rebalancing analytics and plans.
pub struct RebalancingEngine;

impl RebalancingEngine {
    /// Compute per-holding drift from target weights.
    ///
    /// `targets` maps symbol → target weight; `current_values` maps
    /// symbol → current market value.  Weights are derived by normalising
    /// the values by total portfolio value.
    pub fn compute_drifts(
        targets: &HashMap<String, f64>,
        current_values: &HashMap<String, f64>,
    ) -> Vec<HoldingDrift> {
        let total_value: f64 = current_values.values().sum();
        if total_value <= 0.0 {
            return targets
                .iter()
                .map(|(sym, &tw)| HoldingDrift {
                    symbol: sym.clone(),
                    target_weight: tw,
                    current_weight: 0.0,
                    drift_pct: -tw,
                    requires_rebalance: false,
                })
                .collect();
        }

        let mut drifts = Vec::with_capacity(targets.len());
        for (sym, &target_weight) in targets {
            let value = current_values.get(sym).copied().unwrap_or(0.0);
            let current_weight = value / total_value;
            let drift_pct = current_weight - target_weight;
            drifts.push(HoldingDrift {
                symbol: sym.clone(),
                target_weight,
                current_weight,
                drift_pct,
                requires_rebalance: false, // filled by should_rebalance helpers
            });
        }
        drifts
    }

    /// Decide whether a rebalance is warranted given the drift set and trigger.
    pub fn should_rebalance(
        drifts: &[HoldingDrift],
        trigger: &RebalancingTrigger,
        days_since_last: u32,
    ) -> bool {
        match trigger {
            RebalancingTrigger::Calendar { frequency_days } => {
                days_since_last >= *frequency_days
            }
            RebalancingTrigger::Threshold { drift_pct } => drifts
                .iter()
                .any(|d| d.drift_pct.abs() >= *drift_pct),
            RebalancingTrigger::BandBased { outer_band, .. } => drifts
                .iter()
                .any(|d| d.drift_pct.abs() >= *outer_band),
            RebalancingTrigger::Hybrid {
                max_days,
                threshold_pct,
            } => {
                days_since_last >= *max_days
                    || drifts.iter().any(|d| d.drift_pct.abs() >= *threshold_pct)
            }
        }
    }

    /// Build a rebalancing plan from current drifts.
    ///
    /// Each drift that requires trading generates a trade for the notional
    /// needed to restore the holding to its target weight.
    pub fn generate_plan(
        drifts: &[HoldingDrift],
        portfolio_value: f64,
        trigger: &RebalancingTrigger,
        days_since_last: u32,
    ) -> RebalancingPlan {
        let trigger_reason = Self::trigger_reason(trigger, drifts, days_since_last);

        let mut trades: Vec<(String, f64, bool)> = Vec::new();
        let mut total_turnover = 0.0_f64;

        for drift in drifts {
            let notional = drift.drift_pct.abs() * portfolio_value;
            if notional < 1e-8 {
                continue;
            }
            let is_buy = drift.drift_pct < 0.0; // under-weight → buy
            trades.push((drift.symbol.clone(), notional, is_buy));
            total_turnover += notional;
        }

        total_turnover /= portfolio_value.max(1.0);

        RebalancingPlan {
            trades,
            total_turnover,
            estimated_cost: RebalancingCost::default(),
            trigger_reason,
        }
    }

    /// Estimate execution costs for a plan.
    ///
    /// - `bid_ask_bps`: half-spread in basis points.
    /// - `tax_rate`: effective capital-gains rate (0–1).
    /// - `holding_days`: average holding period; short-term vs long-term
    ///   is not modelled — `tax_rate` is applied uniformly.
    pub fn estimate_cost(
        plan: &RebalancingPlan,
        bid_ask_bps: f64,
        tax_rate: f64,
        _holding_days: u32,
    ) -> RebalancingCost {
        let total_notional: f64 = plan.trades.iter().map(|(_, n, _)| n).sum();
        let transaction_costs = total_notional * bid_ask_bps / 10_000.0;
        let market_impact = transaction_costs * 0.5; // simplified: half of spread
        let tax_cost = total_notional * tax_rate * 0.02; // assume 2 % avg gain
        let total = transaction_costs + market_impact + tax_cost;
        RebalancingCost {
            transaction_costs,
            market_impact,
            tax_cost,
            total,
        }
    }

    /// Net benefit of rebalancing: alpha gain minus total cost.
    ///
    /// `tracking_error_reduction` is the improvement in annualised tracking
    /// error (fraction) achieved by rebalancing.
    /// `annual_alpha_bps` is the incremental alpha in basis points per year.
    pub fn net_benefit(
        plan: &RebalancingPlan,
        tracking_error_reduction: f64,
        annual_alpha_bps: f64,
    ) -> f64 {
        let alpha_gain = tracking_error_reduction * annual_alpha_bps / 10_000.0;
        alpha_gain - plan.estimated_cost.total
    }

    /// Optimise a plan by preferring to sell holdings with unrealised losses
    /// (tax-loss harvesting) over holdings with gains.
    ///
    /// Holdings with negative unrealised P&L are sold first; remaining sell
    /// notional is filled from holdings ordered by smallest gain.
    pub fn tax_lot_optimization(
        plan: &RebalancingPlan,
        unrealized_gains: &HashMap<String, f64>,
    ) -> RebalancingPlan {
        let mut sells: Vec<(String, f64, bool)> = plan
            .trades
            .iter()
            .filter(|(_, _, is_buy)| !is_buy)
            .cloned()
            .collect();

        // Sort sells: losses first (most negative gain first).
        sells.sort_by(|(sym_a, _, _), (sym_b, _, _)| {
            let ga = unrealized_gains.get(sym_a).copied().unwrap_or(0.0);
            let gb = unrealized_gains.get(sym_b).copied().unwrap_or(0.0);
            ga.partial_cmp(&gb).unwrap_or(std::cmp::Ordering::Equal)
        });

        let buys: Vec<(String, f64, bool)> = plan
            .trades
            .iter()
            .filter(|(_, _, is_buy)| *is_buy)
            .cloned()
            .collect();

        let mut trades = sells;
        trades.extend(buys);

        RebalancingPlan {
            trades,
            total_turnover: plan.total_turnover,
            estimated_cost: plan.estimated_cost.clone(),
            trigger_reason: plan.trigger_reason.clone(),
        }
    }

    /// Minimum-variance rebalancing: generate a plan that minimises the
    /// portfolio's variance using the provided covariance matrix.
    ///
    /// Currently implements a simplified proportional deviation approach;
    /// full quadratic programming is left as an extension point.
    pub fn minimum_variance_rebalance(
        targets: &HashMap<String, f64>,
        current: &HashMap<String, f64>,
        cov_matrix: &[Vec<f64>],
    ) -> RebalancingPlan {
        let symbols: Vec<&String> = targets.keys().collect();
        let n = symbols.len();
        let total_value: f64 = current.values().sum();

        // Compute per-asset marginal variance contribution as a scaling factor.
        let mut trades: Vec<(String, f64, bool)> = Vec::new();
        let mut total_turnover = 0.0;

        for (i, sym) in symbols.iter().enumerate() {
            let target_w = targets.get(*sym).copied().unwrap_or(0.0);
            let current_v = current.get(*sym).copied().unwrap_or(0.0);
            let current_w = if total_value > 0.0 {
                current_v / total_value
            } else {
                0.0
            };

            // Marginal variance: row sum of cov_matrix for this asset.
            let mv: f64 = if i < cov_matrix.len() {
                cov_matrix[i].iter().sum::<f64>() / n.max(1) as f64
            } else {
                1.0
            };

            // Scale the rebalance notional by inverse marginal variance.
            let drift = target_w - current_w;
            let scale = (1.0 / mv.abs().max(1e-10)).min(2.0);
            let notional = drift.abs() * total_value * scale;
            if notional < 1e-8 {
                continue;
            }
            let is_buy = drift > 0.0;
            trades.push(((*sym).clone(), notional, is_buy));
            total_turnover += notional;
        }

        total_turnover /= total_value.max(1.0);

        RebalancingPlan {
            trades,
            total_turnover,
            estimated_cost: RebalancingCost::default(),
            trigger_reason: "MinimumVariance".to_string(),
        }
    }

    // ── private helpers ────────────────────────────────────────────────────

    fn trigger_reason(
        trigger: &RebalancingTrigger,
        drifts: &[HoldingDrift],
        days_since_last: u32,
    ) -> String {
        match trigger {
            RebalancingTrigger::Calendar { frequency_days } => {
                format!("Calendar: {} days elapsed (freq={})", days_since_last, frequency_days)
            }
            RebalancingTrigger::Threshold { drift_pct } => {
                let max_drift = drifts
                    .iter()
                    .map(|d| d.drift_pct.abs())
                    .fold(0.0_f64, f64::max);
                format!("Threshold: max drift {:.4} >= {:.4}", max_drift, drift_pct)
            }
            RebalancingTrigger::BandBased { inner_band, outer_band } => {
                format!(
                    "BandBased: inner={:.4} outer={:.4}",
                    inner_band, outer_band
                )
            }
            RebalancingTrigger::Hybrid { max_days: _, threshold_pct } => {
                format!(
                    "Hybrid: days={} threshold={:.4}",
                    days_since_last, threshold_pct
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_targets() -> HashMap<String, f64> {
        [("AAPL".into(), 0.40), ("MSFT".into(), 0.60)]
            .iter()
            .cloned()
            .collect()
    }

    fn sample_values() -> HashMap<String, f64> {
        [("AAPL".into(), 350.0_f64), ("MSFT".into(), 650.0)]
            .iter()
            .cloned()
            .collect()
    }

    #[test]
    fn test_compute_drifts_balanced() {
        let targets = sample_targets();
        let values = sample_values();
        let drifts = RebalancingEngine::compute_drifts(&targets, &values);
        // Total value = 1000, AAPL current = 0.35 (target 0.40), drift = -0.05
        for d in &drifts {
            if d.symbol == "AAPL" {
                assert!((d.drift_pct - (-0.05)).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn test_should_rebalance_threshold() {
        let targets = sample_targets();
        let values = sample_values();
        let drifts = RebalancingEngine::compute_drifts(&targets, &values);
        let trigger = RebalancingTrigger::Threshold { drift_pct: 0.03 };
        assert!(RebalancingEngine::should_rebalance(&drifts, &trigger, 0));
        let trigger_high = RebalancingTrigger::Threshold { drift_pct: 0.10 };
        assert!(!RebalancingEngine::should_rebalance(&drifts, &trigger_high, 0));
    }

    #[test]
    fn test_generate_plan() {
        let targets = sample_targets();
        let values = sample_values();
        let drifts = RebalancingEngine::compute_drifts(&targets, &values);
        let trigger = RebalancingTrigger::Threshold { drift_pct: 0.03 };
        let plan = RebalancingEngine::generate_plan(&drifts, 1_000.0, &trigger, 0);
        assert!(!plan.trades.is_empty());
        assert!(plan.total_turnover > 0.0);
    }
}
