//! Execution cost estimation and turnover optimization.
//!
//! ## Overview
//!
//! This module provides:
//! - [`ExecutionCost`]: breakdown of round-trip execution costs (commission, spread, market impact).
//! - [`CostParams`]: parameters for the cost model (commission rate, spread, impact coefficient).
//! - [`CostModel`]: estimates execution cost for a given order.
//! - [`TurnoverOptimizer`]: finds trades minimizing cost while tracking a target portfolio.
//! - [`Trade`]: a single rebalancing trade with estimated cost.

use std::collections::HashMap;

// ─── ExecutionCost ────────────────────────────────────────────────────────────

/// Full breakdown of estimated round-trip execution cost for one order.
#[derive(Debug, Clone)]
pub struct ExecutionCost {
    /// Commission paid to the broker, in USD.
    pub commission_usd: f64,
    /// Half-spread cost (buy at ask, sell at bid), in USD.
    pub spread_cost_usd: f64,
    /// Estimated market impact cost (price concession due to order size), in USD.
    pub market_impact_usd: f64,
    /// Sum of all cost components, in USD.
    pub total_cost_usd: f64,
    /// Total cost expressed as basis points of notional: `total_cost_usd / notional * 10_000`.
    pub cost_bps: f64,
}

// ─── CostParams ───────────────────────────────────────────────────────────────

/// Parameters controlling the execution cost model.
#[derive(Debug, Clone)]
pub struct CostParams {
    /// Commission per share (USD/share).
    pub commission_per_share: f64,
    /// One-way bid-ask spread in basis points.
    pub spread_bps: f64,
    /// Almgren-Chriss impact coefficient `η` (dimensionless).
    pub impact_coefficient: f64,
    /// Average daily volume for the instrument (shares/day).
    pub avg_daily_volume: f64,
}

// ─── CostModel ────────────────────────────────────────────────────────────────

/// Estimates round-trip execution cost for a given order.
///
/// ## Formula
///
/// ```text
/// commission_usd  = commission_per_share * shares
/// spread_cost_usd = (spread_bps / 10_000) * notional_usd
/// impact_bps      = impact_coefficient * sqrt(shares / avg_daily_volume) * 10_000
/// market_impact_usd = (impact_bps / 10_000) * notional_usd
/// total_cost_usd  = commission_usd + spread_cost_usd + market_impact_usd
/// cost_bps        = total_cost_usd / notional_usd * 10_000
/// ```
pub struct CostModel;

impl CostModel {
    /// Estimates round-trip execution cost for an order of `shares` at `price`.
    ///
    /// `notional_usd` = `shares * price` (passed explicitly to avoid rounding differences).
    pub fn estimate(notional_usd: f64, shares: f64, price: f64, params: &CostParams) -> ExecutionCost {
        let _ = price; // price is implicit in notional/shares

        let commission_usd = params.commission_per_share * shares;

        let spread_cost_usd = (params.spread_bps / 10_000.0) * notional_usd;

        let impact_bps = if params.avg_daily_volume > 0.0 {
            params.impact_coefficient * (shares / params.avg_daily_volume).sqrt() * 10_000.0
        } else {
            0.0
        };
        let market_impact_usd = (impact_bps / 10_000.0) * notional_usd;

        let total_cost_usd = commission_usd + spread_cost_usd + market_impact_usd;
        let cost_bps = if notional_usd.abs() > 0.0 {
            total_cost_usd / notional_usd * 10_000.0
        } else {
            0.0
        };

        ExecutionCost {
            commission_usd,
            spread_cost_usd,
            market_impact_usd,
            total_cost_usd,
            cost_bps,
        }
    }
}

// ─── Trade ────────────────────────────────────────────────────────────────────

/// Direction of a rebalancing trade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TradeDirection {
    /// Buy (increase position / add weight).
    Buy,
    /// Sell (decrease position / reduce weight).
    Sell,
}

/// A single rebalancing trade recommended by [`TurnoverOptimizer`].
#[derive(Debug, Clone)]
pub struct Trade {
    /// Instrument symbol.
    pub symbol: String,
    /// Buy or sell.
    pub direction: TradeDirection,
    /// Absolute change in portfolio weight.
    pub weight_change: f64,
    /// Estimated one-way cost in basis points.
    pub estimated_cost_bps: f64,
}

// ─── TurnoverOptimizer ────────────────────────────────────────────────────────

/// Finds the minimal set of trades that moves a portfolio from `current` weights
/// to `target` weights while keeping total execution cost low.
///
/// ## Algorithm
///
/// 1. For each symbol in the union of current and target weights, compute the
///    weight delta `Δw = target - current`.
/// 2. If `|Δw| < tolerance`, skip (already within band).
/// 3. Otherwise add a [`Trade`] with the estimated cost for a unit-notional order
///    sized proportionally to `|Δw|`.
/// 4. Trades are sorted by `|Δw|` descending (largest rebalance first).
pub struct TurnoverOptimizer;

impl TurnoverOptimizer {
    /// Generate the minimal set of trades to rebalance from `current` to `target`.
    ///
    /// `tolerance` is the minimum absolute weight change that warrants a trade
    /// (e.g. 0.005 = 50 bps). Smaller changes are ignored to avoid excessive
    /// round-trip cost.
    ///
    /// `cost_params` are used to estimate the cost of each trade assuming a
    /// unit notional of $1, with `shares = |Δw| / price` where price is set
    /// to 1.0 for weight-space estimation.
    pub fn optimize(
        current: &HashMap<String, f64>,
        target: &HashMap<String, f64>,
        cost_params: &CostParams,
        tolerance: f64,
    ) -> Vec<Trade> {
        // Collect all symbols from both maps.
        let mut symbols: Vec<String> = current
            .keys()
            .chain(target.keys())
            .cloned()
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        symbols.sort();

        let mut trades: Vec<Trade> = Vec::new();

        for symbol in &symbols {
            let cur = current.get(symbol).copied().unwrap_or(0.0);
            let tgt = target.get(symbol).copied().unwrap_or(0.0);
            let delta = tgt - cur;

            if delta.abs() < tolerance {
                continue;
            }

            // Estimate cost: treat weight change as fraction of notional = 1 USD.
            // shares = |delta| * 1 USD / $1 per share = |delta|
            let notional = delta.abs();
            let shares = delta.abs();
            let cost = CostModel::estimate(notional, shares, 1.0, cost_params);

            trades.push(Trade {
                symbol: symbol.clone(),
                direction: if delta > 0.0 { TradeDirection::Buy } else { TradeDirection::Sell },
                weight_change: delta.abs(),
                estimated_cost_bps: cost.cost_bps,
            });
        }

        // Sort by weight change descending (largest rebalance first).
        trades.sort_by(|a, b| b.weight_change.partial_cmp(&a.weight_change).unwrap_or(std::cmp::Ordering::Equal));
        trades
    }
}

// ─── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn default_params() -> CostParams {
        CostParams {
            commission_per_share: 0.005,
            spread_bps: 5.0,
            impact_coefficient: 0.1,
            avg_daily_volume: 1_000_000.0,
        }
    }

    // ── CostModel ──────────────────────────────────────────────────────────

    #[test]
    fn commission_computed_correctly() {
        let params = default_params();
        let cost = CostModel::estimate(10_000.0, 1_000.0, 10.0, &params);
        // commission = 0.005 * 1000 = 5.0
        assert!((cost.commission_usd - 5.0).abs() < 1e-9, "commission={}", cost.commission_usd);
    }

    #[test]
    fn spread_cost_computed_correctly() {
        let params = default_params();
        let cost = CostModel::estimate(10_000.0, 1_000.0, 10.0, &params);
        // spread = 5 bps * 10000 = 5.0 USD
        assert!((cost.spread_cost_usd - 5.0).abs() < 1e-9, "spread={}", cost.spread_cost_usd);
    }

    #[test]
    fn market_impact_formula() {
        // impact_bps = 0.1 * sqrt(1000 / 1_000_000) * 10_000 = 0.1 * 0.03162 * 10_000 = 31.62
        let params = default_params();
        let cost = CostModel::estimate(10_000.0, 1_000.0, 10.0, &params);
        let expected_impact_bps = 0.1 * (1_000.0f64 / 1_000_000.0).sqrt() * 10_000.0;
        let expected_impact_usd = expected_impact_bps / 10_000.0 * 10_000.0;
        assert!((cost.market_impact_usd - expected_impact_usd).abs() < 1e-6,
            "impact={} expected={}", cost.market_impact_usd, expected_impact_usd);
    }

    #[test]
    fn total_cost_is_sum_of_components() {
        let params = default_params();
        let cost = CostModel::estimate(10_000.0, 1_000.0, 10.0, &params);
        let expected = cost.commission_usd + cost.spread_cost_usd + cost.market_impact_usd;
        assert!((cost.total_cost_usd - expected).abs() < 1e-9);
    }

    #[test]
    fn cost_bps_equals_total_over_notional() {
        let params = default_params();
        let notional = 50_000.0;
        let cost = CostModel::estimate(notional, 5_000.0, 10.0, &params);
        let expected_bps = cost.total_cost_usd / notional * 10_000.0;
        assert!((cost.cost_bps - expected_bps).abs() < 1e-9);
    }

    #[test]
    fn zero_shares_zero_cost() {
        let params = default_params();
        let cost = CostModel::estimate(0.0, 0.0, 10.0, &params);
        assert_eq!(cost.commission_usd, 0.0);
        assert_eq!(cost.spread_cost_usd, 0.0);
        assert_eq!(cost.market_impact_usd, 0.0);
        assert_eq!(cost.total_cost_usd, 0.0);
        assert_eq!(cost.cost_bps, 0.0);
    }

    #[test]
    fn zero_adv_zero_impact() {
        let mut params = default_params();
        params.avg_daily_volume = 0.0;
        let cost = CostModel::estimate(10_000.0, 1_000.0, 10.0, &params);
        assert_eq!(cost.market_impact_usd, 0.0);
    }

    #[test]
    fn impact_increases_with_shares() {
        let params = default_params();
        let cost_small = CostModel::estimate(1_000.0, 100.0, 10.0, &params);
        let cost_large = CostModel::estimate(100_000.0, 10_000.0, 10.0, &params);
        assert!(cost_large.market_impact_usd > cost_small.market_impact_usd);
    }

    #[test]
    fn higher_spread_higher_cost() {
        let mut params_lo = default_params();
        let mut params_hi = default_params();
        params_lo.spread_bps = 1.0;
        params_hi.spread_bps = 20.0;
        let lo = CostModel::estimate(10_000.0, 1_000.0, 10.0, &params_lo);
        let hi = CostModel::estimate(10_000.0, 1_000.0, 10.0, &params_hi);
        assert!(hi.spread_cost_usd > lo.spread_cost_usd);
    }

    // ── TurnoverOptimizer ──────────────────────────────────────────────────

    #[test]
    fn no_trades_when_within_tolerance() {
        let current: HashMap<String, f64> = [("AAPL".to_string(), 0.3), ("MSFT".to_string(), 0.7)].into();
        let target: HashMap<String, f64> = [("AAPL".to_string(), 0.301), ("MSFT".to_string(), 0.699)].into();
        let params = default_params();
        let trades = TurnoverOptimizer::optimize(&current, &target, &params, 0.005);
        assert!(trades.is_empty(), "expected no trades, got {}", trades.len());
    }

    #[test]
    fn trades_generated_for_large_deltas() {
        let current: HashMap<String, f64> = [("AAPL".to_string(), 0.2)].into();
        let target: HashMap<String, f64> = [("AAPL".to_string(), 0.5)].into();
        let params = default_params();
        let trades = TurnoverOptimizer::optimize(&current, &target, &params, 0.005);
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].symbol, "AAPL");
        assert_eq!(trades[0].direction, TradeDirection::Buy);
        assert!((trades[0].weight_change - 0.3).abs() < 1e-9);
    }

    #[test]
    fn sell_direction_for_reduce() {
        let current: HashMap<String, f64> = [("SPY".to_string(), 0.6)].into();
        let target: HashMap<String, f64> = [("SPY".to_string(), 0.3)].into();
        let params = default_params();
        let trades = TurnoverOptimizer::optimize(&current, &target, &params, 0.005);
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].direction, TradeDirection::Sell);
        assert!((trades[0].weight_change - 0.3).abs() < 1e-9);
    }

    #[test]
    fn new_position_is_buy() {
        let current: HashMap<String, f64> = HashMap::new();
        let target: HashMap<String, f64> = [("GLD".to_string(), 0.1)].into();
        let params = default_params();
        let trades = TurnoverOptimizer::optimize(&current, &target, &params, 0.005);
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].direction, TradeDirection::Buy);
    }

    #[test]
    fn liquidate_position_is_sell() {
        let current: HashMap<String, f64> = [("TLT".to_string(), 0.25)].into();
        let target: HashMap<String, f64> = HashMap::new();
        let params = default_params();
        let trades = TurnoverOptimizer::optimize(&current, &target, &params, 0.005);
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].direction, TradeDirection::Sell);
    }

    #[test]
    fn trades_sorted_by_weight_change_desc() {
        let current: HashMap<String, f64> = [
            ("A".to_string(), 0.1),
            ("B".to_string(), 0.5),
            ("C".to_string(), 0.2),
        ].into();
        let target: HashMap<String, f64> = [
            ("A".to_string(), 0.5),  // +0.4
            ("B".to_string(), 0.1),  // -0.4
            ("C".to_string(), 0.4),  // +0.2
        ].into();
        let params = default_params();
        let trades = TurnoverOptimizer::optimize(&current, &target, &params, 0.005);
        assert_eq!(trades.len(), 3);
        for i in 0..trades.len() - 1 {
            assert!(trades[i].weight_change >= trades[i + 1].weight_change);
        }
    }

    #[test]
    fn estimated_cost_bps_non_negative() {
        let current: HashMap<String, f64> = [("X".to_string(), 0.0)].into();
        let target: HashMap<String, f64> = [("X".to_string(), 0.1)].into();
        let params = default_params();
        let trades = TurnoverOptimizer::optimize(&current, &target, &params, 0.005);
        for t in &trades {
            assert!(t.estimated_cost_bps >= 0.0, "cost_bps={}", t.estimated_cost_bps);
        }
    }

    #[test]
    fn multiple_symbols_multi_trade() {
        let current: HashMap<String, f64> = [
            ("AAPL".to_string(), 0.25),
            ("MSFT".to_string(), 0.25),
            ("GOOG".to_string(), 0.25),
            ("AMZN".to_string(), 0.25),
        ].into();
        let target: HashMap<String, f64> = [
            ("AAPL".to_string(), 0.4),
            ("MSFT".to_string(), 0.1),
            ("GOOG".to_string(), 0.35),
            ("AMZN".to_string(), 0.15),
        ].into();
        let params = default_params();
        let trades = TurnoverOptimizer::optimize(&current, &target, &params, 0.005);
        // All four have |delta| > 0.005
        assert_eq!(trades.len(), 4);
    }

    #[test]
    fn exact_tolerance_boundary_excluded() {
        // delta = exactly tolerance: should be excluded
        let current: HashMap<String, f64> = [("X".to_string(), 0.0)].into();
        let target: HashMap<String, f64> = [("X".to_string(), 0.005)].into();
        let params = default_params();
        let trades = TurnoverOptimizer::optimize(&current, &target, &params, 0.005);
        assert!(trades.is_empty(), "delta exactly = tolerance should be excluded");
    }

    #[test]
    fn cost_model_all_fields_populated() {
        let params = default_params();
        let cost = CostModel::estimate(10_000.0, 500.0, 20.0, &params);
        assert!(cost.commission_usd > 0.0);
        assert!(cost.spread_cost_usd > 0.0);
        assert!(cost.market_impact_usd > 0.0);
        assert!(cost.total_cost_usd > 0.0);
        assert!(cost.cost_bps > 0.0);
    }

    #[test]
    fn impact_coefficient_zero_no_impact() {
        let mut params = default_params();
        params.impact_coefficient = 0.0;
        let cost = CostModel::estimate(10_000.0, 1_000.0, 10.0, &params);
        assert_eq!(cost.market_impact_usd, 0.0);
    }

    #[test]
    fn trade_weight_change_is_absolute() {
        let current: HashMap<String, f64> = [("X".to_string(), 0.5)].into();
        let target: HashMap<String, f64> = [("X".to_string(), 0.2)].into();
        let params = default_params();
        let trades = TurnoverOptimizer::optimize(&current, &target, &params, 0.005);
        assert_eq!(trades.len(), 1);
        assert!(trades[0].weight_change > 0.0, "weight_change should be positive");
    }

    #[test]
    fn empty_portfolios_no_trades() {
        let current: HashMap<String, f64> = HashMap::new();
        let target: HashMap<String, f64> = HashMap::new();
        let params = default_params();
        let trades = TurnoverOptimizer::optimize(&current, &target, &params, 0.005);
        assert!(trades.is_empty());
    }
}
