//! Portfolio rebalancing: drift calculation, threshold and calendar triggers,
//! trade generation, turnover estimation, and tax-aware rebalancing.
//!
//! [`Rebalancer`] is a stateless utility — all methods take explicit snapshots of
//! positions and targets so callers maintain full control over state.

/// Target allocation band for a single asset.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetAllocation {
    /// Ticker symbol.
    pub symbol: String,
    /// Ideal portfolio weight (0.0–1.0).
    pub target_weight: f64,
    /// Lower drift band — weight below this triggers rebalancing.
    pub min_weight: f64,
    /// Upper drift band — weight above this triggers rebalancing.
    pub max_weight: f64,
}

/// Current snapshot of a position's market value and weight.
#[derive(Debug, Clone, PartialEq)]
pub struct PortfolioPosition {
    /// Ticker symbol.
    pub symbol: String,
    /// Current market value of the position.
    pub market_value: f64,
    /// Current portfolio weight (market_value / total_portfolio_value).
    pub current_weight: f64,
}

/// Drift metrics for a single asset.
#[derive(Debug, Clone, PartialEq)]
pub struct RebalanceDrift {
    /// Ticker symbol.
    pub symbol: String,
    /// Current portfolio weight.
    pub current_weight: f64,
    /// Target portfolio weight.
    pub target_weight: f64,
    /// Signed drift: current_weight − target_weight.
    pub drift: f64,
    /// Absolute drift (|drift|).
    pub abs_drift: f64,
}

/// Condition that triggers a rebalance.
#[derive(Debug, Clone, PartialEq)]
pub enum RebalanceTrigger {
    /// Rebalance if any asset's absolute drift exceeds `max_drift`.
    ThresholdBreach(f64),
    /// Rebalance if `days_since_last` >= `interval_days`.
    CalendarBased(u32),
    /// Rebalance if either the threshold OR the calendar condition is met.
    BothConditions,
}

/// Direction of a rebalance trade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TradeDirection {
    /// Buy additional shares to increase allocation.
    Buy,
    /// Sell shares to decrease allocation.
    Sell,
}

/// A single trade required to move toward target allocation.
#[derive(Debug, Clone, PartialEq)]
pub struct RebalanceTrade {
    /// Ticker symbol.
    pub symbol: String,
    /// Whether to buy or sell.
    pub direction: TradeDirection,
    /// Dollar amount of the trade (always positive).
    pub amount: f64,
    /// Target weight this trade is working toward.
    pub target_weight: f64,
}

/// Stateless portfolio rebalancer.
pub struct Rebalancer;

impl Rebalancer {
    /// Compute the drift of each position relative to its target allocation.
    ///
    /// Positions without a matching target are assigned a target weight of 0.0.
    pub fn compute_drift(
        positions: &[PortfolioPosition],
        targets: &[TargetAllocation],
    ) -> Vec<RebalanceDrift> {
        positions
            .iter()
            .map(|pos| {
                let target_weight = targets
                    .iter()
                    .find(|t| t.symbol == pos.symbol)
                    .map(|t| t.target_weight)
                    .unwrap_or(0.0);
                let drift = pos.current_weight - target_weight;
                RebalanceDrift {
                    symbol: pos.symbol.clone(),
                    current_weight: pos.current_weight,
                    target_weight,
                    drift,
                    abs_drift: drift.abs(),
                }
            })
            .collect()
    }

    /// Determine whether a rebalance should be triggered.
    ///
    /// - [`RebalanceTrigger::ThresholdBreach`]: true if any asset's `abs_drift` exceeds the threshold.
    /// - [`RebalanceTrigger::CalendarBased`]: true if `days_since_last >= interval_days`.
    /// - [`RebalanceTrigger::BothConditions`]: true if either sub-condition fires.
    pub fn should_rebalance(
        drift: &[RebalanceDrift],
        trigger: &RebalanceTrigger,
        days_since_last: u32,
    ) -> bool {
        match trigger {
            RebalanceTrigger::ThresholdBreach(max_drift) => {
                drift.iter().any(|d| d.abs_drift > *max_drift)
            }
            RebalanceTrigger::CalendarBased(interval_days) => {
                days_since_last >= *interval_days
            }
            RebalanceTrigger::BothConditions => {
                // "Both" in common parlance means "either condition" (union trigger).
                let threshold_hit = drift.iter().any(|d| d.abs_drift > 0.05);
                let calendar_hit = days_since_last >= 90;
                threshold_hit || calendar_hit
            }
        }
    }

    /// Generate the trades needed to bring each position to its target weight.
    ///
    /// Proportional rebalancing: each asset's target dollar value = target_weight × total_value.
    /// Assets without a current position are also included (new buys).
    pub fn generate_trades(
        positions: &[PortfolioPosition],
        targets: &[TargetAllocation],
        total_value: f64,
    ) -> Vec<RebalanceTrade> {
        let mut trades: Vec<RebalanceTrade> = Vec::new();

        for target in targets {
            let current_value = positions
                .iter()
                .find(|p| p.symbol == target.symbol)
                .map(|p| p.market_value)
                .unwrap_or(0.0);
            let desired_value = target.target_weight * total_value;
            let diff = desired_value - current_value;

            if diff.abs() < 1e-6 {
                continue;
            }

            trades.push(RebalanceTrade {
                symbol: target.symbol.clone(),
                direction: if diff > 0.0 {
                    TradeDirection::Buy
                } else {
                    TradeDirection::Sell
                },
                amount: diff.abs(),
                target_weight: target.target_weight,
            });
        }

        // Also handle positions with no matching target (sell down to zero).
        for pos in positions {
            let has_target = targets.iter().any(|t| t.symbol == pos.symbol);
            if !has_target && pos.market_value > 1e-6 {
                trades.push(RebalanceTrade {
                    symbol: pos.symbol.clone(),
                    direction: TradeDirection::Sell,
                    amount: pos.market_value,
                    target_weight: 0.0,
                });
            }
        }

        trades
    }

    /// Estimate portfolio turnover as a fraction of total portfolio value.
    ///
    /// Turnover = sum(trade amounts) / total_value.
    /// Values above 1.0 are possible for large rebalances.
    pub fn estimated_turnover(trades: &[RebalanceTrade], total_value: f64) -> f64 {
        if total_value <= 0.0 {
            return 0.0;
        }
        let total_traded: f64 = trades.iter().map(|t| t.amount).sum();
        total_traded / total_value
    }

    /// Tax-aware rebalancing: prefer selling positions with losses (negative gain)
    /// before selling positions with gains, to minimize realized tax liability.
    ///
    /// `gains` is a slice of `(symbol, unrealized_gain)` pairs. Symbols with negative
    /// gains are prioritized for selling; symbols with positive gains are sold last.
    pub fn tax_aware_rebalance(
        positions: &[PortfolioPosition],
        targets: &[TargetAllocation],
        gains: &[(String, f64)],
    ) -> Vec<RebalanceTrade> {
        let total_value: f64 = positions.iter().map(|p| p.market_value).sum();
        let mut trades = Self::generate_trades(positions, targets, total_value);

        // Separate sells by loss vs gain status.
        let gain_map: std::collections::HashMap<&str, f64> = gains
            .iter()
            .map(|(s, g)| (s.as_str(), *g))
            .collect();

        // Sort: sells — loss positions first (smallest/most negative gain first),
        //       then gains; buys keep their natural order.
        trades.sort_by(|a, b| {
            // Both buys: equal priority.
            if a.direction == TradeDirection::Buy && b.direction == TradeDirection::Buy {
                return std::cmp::Ordering::Equal;
            }
            // Sells before buys in the ordering (buys come after).
            if a.direction == TradeDirection::Sell && b.direction == TradeDirection::Buy {
                return std::cmp::Ordering::Less;
            }
            if a.direction == TradeDirection::Buy && b.direction == TradeDirection::Sell {
                return std::cmp::Ordering::Greater;
            }
            // Both sells: sort by gain ascending (losses first).
            let ga = gain_map.get(a.symbol.as_str()).copied().unwrap_or(0.0);
            let gb = gain_map.get(b.symbol.as_str()).copied().unwrap_or(0.0);
            ga.partial_cmp(&gb).unwrap_or(std::cmp::Ordering::Equal)
        });

        trades
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(sym: &str, mv: f64, w: f64) -> PortfolioPosition {
        PortfolioPosition {
            symbol: sym.to_string(),
            market_value: mv,
            current_weight: w,
        }
    }

    fn tgt(sym: &str, tw: f64) -> TargetAllocation {
        TargetAllocation {
            symbol: sym.to_string(),
            target_weight: tw,
            min_weight: tw - 0.05,
            max_weight: tw + 0.05,
        }
    }

    #[test]
    fn test_compute_drift_basic() {
        let positions = vec![
            pos("AAPL", 6000.0, 0.60),
            pos("MSFT", 4000.0, 0.40),
        ];
        let targets = vec![tgt("AAPL", 0.50), tgt("MSFT", 0.50)];
        let drift = Rebalancer::compute_drift(&positions, &targets);
        assert_eq!(drift.len(), 2);
        let aapl = drift.iter().find(|d| d.symbol == "AAPL").unwrap();
        assert!((aapl.drift - 0.10).abs() < 1e-9);
        assert!((aapl.abs_drift - 0.10).abs() < 1e-9);
        let msft = drift.iter().find(|d| d.symbol == "MSFT").unwrap();
        assert!((msft.drift - (-0.10)).abs() < 1e-9);
    }

    #[test]
    fn test_should_rebalance_threshold() {
        let drift = vec![RebalanceDrift {
            symbol: "AAPL".to_string(),
            current_weight: 0.60,
            target_weight: 0.50,
            drift: 0.10,
            abs_drift: 0.10,
        }];
        assert!(Rebalancer::should_rebalance(
            &drift,
            &RebalanceTrigger::ThresholdBreach(0.05),
            0
        ));
        assert!(!Rebalancer::should_rebalance(
            &drift,
            &RebalanceTrigger::ThresholdBreach(0.15),
            0
        ));
    }

    #[test]
    fn test_should_rebalance_calendar() {
        let drift: Vec<RebalanceDrift> = Vec::new();
        assert!(Rebalancer::should_rebalance(
            &drift,
            &RebalanceTrigger::CalendarBased(90),
            90
        ));
        assert!(!Rebalancer::should_rebalance(
            &drift,
            &RebalanceTrigger::CalendarBased(90),
            89
        ));
    }

    #[test]
    fn test_generate_trades_balanced() {
        // Already balanced — no trades.
        let positions = vec![
            pos("AAPL", 5000.0, 0.50),
            pos("MSFT", 5000.0, 0.50),
        ];
        let targets = vec![tgt("AAPL", 0.50), tgt("MSFT", 0.50)];
        let trades = Rebalancer::generate_trades(&positions, &targets, 10000.0);
        assert!(trades.is_empty());
    }

    #[test]
    fn test_generate_trades_unbalanced() {
        let positions = vec![
            pos("AAPL", 7000.0, 0.70),
            pos("MSFT", 3000.0, 0.30),
        ];
        let targets = vec![tgt("AAPL", 0.50), tgt("MSFT", 0.50)];
        let trades = Rebalancer::generate_trades(&positions, &targets, 10000.0);
        let aapl_trade = trades.iter().find(|t| t.symbol == "AAPL").unwrap();
        let msft_trade = trades.iter().find(|t| t.symbol == "MSFT").unwrap();
        assert_eq!(aapl_trade.direction, TradeDirection::Sell);
        assert!((aapl_trade.amount - 2000.0).abs() < 1e-6);
        assert_eq!(msft_trade.direction, TradeDirection::Buy);
        assert!((msft_trade.amount - 2000.0).abs() < 1e-6);
    }

    #[test]
    fn test_sell_untargeted_position() {
        let positions = vec![
            pos("AAPL", 5000.0, 0.50),
            pos("JUNK", 5000.0, 0.50),
        ];
        let targets = vec![tgt("AAPL", 1.0)];
        let trades = Rebalancer::generate_trades(&positions, &targets, 10000.0);
        let junk_trade = trades.iter().find(|t| t.symbol == "JUNK").unwrap();
        assert_eq!(junk_trade.direction, TradeDirection::Sell);
        assert!((junk_trade.amount - 5000.0).abs() < 1e-6);
    }

    #[test]
    fn test_estimated_turnover() {
        let trades = vec![
            RebalanceTrade {
                symbol: "AAPL".to_string(),
                direction: TradeDirection::Sell,
                amount: 1000.0,
                target_weight: 0.50,
            },
            RebalanceTrade {
                symbol: "MSFT".to_string(),
                direction: TradeDirection::Buy,
                amount: 1000.0,
                target_weight: 0.50,
            },
        ];
        let turnover = Rebalancer::estimated_turnover(&trades, 10000.0);
        assert!((turnover - 0.20).abs() < 1e-9);
    }

    #[test]
    fn test_estimated_turnover_zero_value() {
        let trades: Vec<RebalanceTrade> = Vec::new();
        assert_eq!(Rebalancer::estimated_turnover(&trades, 0.0), 0.0);
    }

    #[test]
    fn test_tax_aware_rebalance_sells_losses_first() {
        let positions = vec![
            pos("AAPL", 4000.0, 0.40),
            pos("MSFT", 6000.0, 0.60),
        ];
        let targets = vec![tgt("AAPL", 0.50), tgt("MSFT", 0.50)];
        let gains = vec![
            ("AAPL".to_string(), -500.0),  // loss — should sell first
            ("MSFT".to_string(), 1000.0),  // gain — sell last
        ];
        let trades = Rebalancer::tax_aware_rebalance(&positions, &targets, &gains);
        // MSFT should be sold; AAPL should be bought.
        let sell = trades.iter().find(|t| t.direction == TradeDirection::Sell).unwrap();
        assert_eq!(sell.symbol, "MSFT");
    }

    #[test]
    fn test_compute_drift_missing_target() {
        let positions = vec![pos("AAPL", 5000.0, 1.0)];
        let targets: Vec<TargetAllocation> = Vec::new();
        let drift = Rebalancer::compute_drift(&positions, &targets);
        assert_eq!(drift[0].target_weight, 0.0);
        assert!((drift[0].drift - 1.0).abs() < 1e-9);
    }
}
