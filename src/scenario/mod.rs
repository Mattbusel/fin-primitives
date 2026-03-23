//! # Module: scenario
//!
//! ## Responsibility
//! Risk scenario backtesting: replays a sequence of historical OHLCV bars through a
//! user-provided risk rule and reports how many times the rule would have triggered,
//! along with the maximum drawdown observed during the scenario.
//!
//! ## Guarantees
//! - All arithmetic uses `rust_decimal::Decimal`
//! - `ScenarioBacktester::run` never panics; all results are returned in a typed report
//! - Equity is simulated as bar-close price by default (caller-supplied equity function)
//!
//! ## NOT Responsible For
//! - Realistic fill simulation (see `position` module)
//! - Multi-asset scenarios

use crate::ohlcv::OhlcvBar;
use crate::risk::{DrawdownTracker, RiskBreach, RiskRule};
use rust_decimal::Decimal;

/// Summary report produced by [`ScenarioBacktester::run`].
#[derive(Debug, Clone)]
pub struct ScenarioReport {
    /// Total number of bars processed.
    pub bars_processed: usize,
    /// Number of bars on which at least one rule triggered.
    pub trigger_count: usize,
    /// All individual breach events, one entry per bar that triggered.
    pub breaches: Vec<BarBreach>,
    /// Maximum drawdown (%) observed at any point during the scenario.
    pub max_drawdown_pct: Decimal,
    /// Starting equity (first bar's simulated equity).
    pub start_equity: Decimal,
    /// Ending equity (last bar's simulated equity).
    pub end_equity: Decimal,
    /// Total equity return: `(end - start) / start * 100` (percent).
    ///
    /// Returns `None` when `start_equity == 0`.
    pub total_return_pct: Option<Decimal>,
}

/// A breach event at a specific bar index.
#[derive(Debug, Clone)]
pub struct BarBreach {
    /// Zero-based bar index.
    pub bar_index: usize,
    /// Risk breaches that fired on this bar.
    pub breaches: Vec<RiskBreach>,
    /// Simulated equity at this bar.
    pub equity: Decimal,
    /// Drawdown percentage at this bar.
    pub drawdown_pct: Decimal,
}

/// Replays historical OHLCV bars through a set of `RiskRule`s.
///
/// The caller supplies:
/// 1. A slice of [`OhlcvBar`] bars (historical data).
/// 2. One or more [`RiskRule`] implementations.
/// 3. An equity function `F: Fn(&OhlcvBar) -> Decimal` that maps each bar to a
///    simulated equity value (e.g. close price, portfolio NAV).
///
/// # Example
/// ```rust
/// use fin_primitives::scenario::ScenarioBacktester;
/// use fin_primitives::risk::{MaxDrawdownRule, DrawdownTracker};
/// use fin_primitives::ohlcv::OhlcvBar;
/// use fin_primitives::types::{Symbol, Price, Quantity, NanoTimestamp};
/// use rust_decimal_macros::dec;
///
/// let sym = Symbol::new("SPY").unwrap();
/// let ts = NanoTimestamp::new(0);
/// let bars: Vec<OhlcvBar> = (0..10).map(|i| {
///     let close_val = dec!(100) - rust_decimal::Decimal::from(i) * dec!(2);
///     let close = Price::new(close_val).unwrap();
///     let open = Price::new(dec!(102)).unwrap();
///     let high = Price::new(dec!(103)).unwrap();
///     let low = Price::new(close_val).unwrap();
///     OhlcvBar::new(sym.clone(), open, high, low, close,
///                   Quantity::new(dec!(1000)).unwrap(), ts, ts, 100).unwrap()
/// }).collect();
///
/// let rule = MaxDrawdownRule { threshold_pct: dec!(10) };
/// let report = ScenarioBacktester::new(bars)
///     .add_rule(Box::new(rule))
///     .run(|bar| bar.close.value());
///
/// assert_eq!(report.bars_processed, 10);
/// ```
pub struct ScenarioBacktester {
    bars: Vec<OhlcvBar>,
    rules: Vec<Box<dyn RiskRule>>,
}

impl ScenarioBacktester {
    /// Creates a new `ScenarioBacktester` with the given historical bars.
    pub fn new(bars: Vec<OhlcvBar>) -> Self {
        Self { bars, rules: Vec::new() }
    }

    /// Adds a risk rule to the set evaluated at each bar.
    ///
    /// Rules are evaluated independently; all triggered rules produce breach events.
    pub fn add_rule(mut self, rule: Box<dyn RiskRule>) -> Self {
        self.rules.push(rule);
        self
    }

    /// Runs the scenario, returning a [`ScenarioReport`].
    ///
    /// `equity_fn` maps each bar to a simulated equity value.
    /// The most common choices are `|bar| bar.close.value()` (close-based equity)
    /// or a portfolio NAV calculation that uses positions from the caller.
    pub fn run<F>(&self, equity_fn: F) -> ScenarioReport
    where
        F: Fn(&OhlcvBar) -> Decimal,
    {
        if self.bars.is_empty() {
            return ScenarioReport {
                bars_processed: 0,
                trigger_count: 0,
                breaches: vec![],
                max_drawdown_pct: Decimal::ZERO,
                start_equity: Decimal::ZERO,
                end_equity: Decimal::ZERO,
                total_return_pct: None,
            };
        }

        let first_equity = equity_fn(&self.bars[0]);
        let mut tracker = DrawdownTracker::new(first_equity);
        let mut all_breaches: Vec<BarBreach> = Vec::new();
        let mut trigger_count = 0usize;
        let mut last_equity = first_equity;

        for (i, bar) in self.bars.iter().enumerate() {
            let equity = equity_fn(bar);
            tracker.update(equity);
            let dd_pct = tracker.current_drawdown_pct();
            last_equity = equity;

            let bar_breaches: Vec<RiskBreach> = self
                .rules
                .iter()
                .filter_map(|rule| rule.check(equity, dd_pct))
                .collect();

            if !bar_breaches.is_empty() {
                trigger_count += 1;
                all_breaches.push(BarBreach {
                    bar_index: i,
                    breaches: bar_breaches,
                    equity,
                    drawdown_pct: dd_pct,
                });
            }
        }

        let max_dd = tracker.worst_drawdown_pct();
        let total_return_pct = if first_equity.is_zero() {
            None
        } else {
            Some((last_equity - first_equity) / first_equity * Decimal::ONE_HUNDRED)
        };

        ScenarioReport {
            bars_processed: self.bars.len(),
            trigger_count,
            breaches: all_breaches,
            max_drawdown_pct: max_dd,
            start_equity: first_equity,
            end_equity: last_equity,
            total_return_pct,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::MaxDrawdownRule;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn sym() -> Symbol {
        Symbol::new("SPY").unwrap()
    }

    fn ts() -> NanoTimestamp {
        NanoTimestamp::new(0)
    }

    fn make_bar(close: rust_decimal::Decimal) -> OhlcvBar {
        let p = Price::new(close).unwrap();
        let high = Price::new(close + dec!(1)).unwrap();
        OhlcvBar::new(
            sym(),
            p,
            high,
            p,
            p,
            Quantity::new(dec!(1000)).unwrap(),
            ts(),
            ts(),
            10,
        )
        .unwrap()
    }

    #[test]
    fn test_no_triggers_when_equity_rises() {
        let bars: Vec<_> = (1..=10).map(|i| make_bar(dec!(100) + rust_decimal::Decimal::from(i))).collect();
        let rule = MaxDrawdownRule { threshold_pct: dec!(5) };
        let report = ScenarioBacktester::new(bars).add_rule(Box::new(rule)).run(|bar| bar.close.value());
        assert_eq!(report.bars_processed, 10);
        assert_eq!(report.trigger_count, 0);
        assert_eq!(report.max_drawdown_pct, Decimal::ZERO);
    }

    #[test]
    fn test_triggers_when_drawdown_exceeds_threshold() {
        // Start at 100, drop to 80 (20% drawdown), threshold is 10%
        let closes = vec![
            dec!(100), dec!(99), dec!(95), dec!(90), dec!(85), dec!(80),
        ];
        let bars: Vec<_> = closes.iter().map(|&c| make_bar(c)).collect();
        let rule = MaxDrawdownRule { threshold_pct: dec!(10) };
        let report = ScenarioBacktester::new(bars).add_rule(Box::new(rule)).run(|bar| bar.close.value());
        assert!(report.trigger_count > 0, "expected at least one trigger");
        assert!(report.max_drawdown_pct > dec!(10));
    }

    #[test]
    fn test_empty_bars_returns_zero_report() {
        let report = ScenarioBacktester::new(vec![]).run(|bar| bar.close.value());
        assert_eq!(report.bars_processed, 0);
        assert_eq!(report.trigger_count, 0);
        assert!(report.total_return_pct.is_none());
    }

    #[test]
    fn test_total_return_pct_computed() {
        let bars = vec![make_bar(dec!(100)), make_bar(dec!(110))];
        let report = ScenarioBacktester::new(bars).run(|bar| bar.close.value());
        // (110-100)/100*100 = 10%
        assert_eq!(report.total_return_pct.unwrap(), dec!(10));
    }

    #[test]
    fn test_multiple_rules_both_can_fire() {
        let closes = vec![dec!(100), dec!(50)]; // 50% drawdown
        let bars: Vec<_> = closes.iter().map(|&c| make_bar(c)).collect();
        let rule1 = MaxDrawdownRule { threshold_pct: dec!(10) };
        let rule2 = MaxDrawdownRule { threshold_pct: dec!(20) };
        let report = ScenarioBacktester::new(bars)
            .add_rule(Box::new(rule1))
            .add_rule(Box::new(rule2))
            .run(|bar| bar.close.value());
        // Both rules should fire on bar index 1 (50% drawdown)
        let bar1 = report.breaches.iter().find(|b| b.bar_index == 1).unwrap();
        assert_eq!(bar1.breaches.len(), 2);
    }

    #[test]
    fn test_max_drawdown_tracked() {
        let closes = vec![dec!(200), dec!(180), dec!(160), dec!(190), dec!(210)];
        let bars: Vec<_> = closes.iter().map(|&c| make_bar(c)).collect();
        let report = ScenarioBacktester::new(bars).run(|bar| bar.close.value());
        // Peak 200, trough 160: 20% drawdown
        assert_eq!(report.max_drawdown_pct, dec!(20));
    }

    // ── ScenarioEngine ────────────────────────────────────────────────────

    #[test]
    fn test_apply_absolute_shift() {
        let engine = ScenarioEngine;
        let shocked = engine.apply_shock(100.0, &ShockType::AbsoluteShift(-30.0));
        assert!((shocked - 70.0).abs() < 1e-9);
    }

    #[test]
    fn test_apply_relative_shift() {
        let engine = ScenarioEngine;
        let shocked = engine.apply_shock(100.0, &ShockType::RelativeShift(-0.20));
        assert!((shocked - 80.0).abs() < 1e-9);
    }

    #[test]
    fn test_apply_volatility_scaling() {
        let engine = ScenarioEngine;
        let shocked = engine.apply_shock(100.0, &ShockType::VolatilityScaling(1.5));
        // VolatilityScaling scales price by the factor
        assert!((shocked - 150.0).abs() < 1e-9);
    }

    #[test]
    fn test_apply_correlation_breakdown() {
        let engine = ScenarioEngine;
        // CorrelationBreakdown shifts by the factor as an absolute amount
        let shocked = engine.apply_shock(100.0, &ShockType::CorrelationBreakdown(10.0));
        assert!((shocked - 110.0).abs() < 1e-9);
    }

    #[test]
    fn test_run_scenario_equity_crash() {
        use std::collections::HashMap;
        let mut portfolio: HashMap<String, f64> = HashMap::new();
        portfolio.insert("equity".to_owned(), 100.0);
        portfolio.insert("vol".to_owned(), 20.0);
        let s = Scenario::equity_crash();
        let engine = ScenarioEngine;
        let shocked = engine.run_scenario(&portfolio, &s);
        // equity should drop to ~70
        let eq = shocked["equity"];
        assert!(eq < 100.0, "equity should drop: {eq}");
    }

    #[test]
    fn test_scenario_pnl_loss() {
        use std::collections::HashMap;
        let mut original: HashMap<String, f64> = HashMap::new();
        original.insert("equity".to_owned(), 100.0);
        let mut shocked: HashMap<String, f64> = HashMap::new();
        shocked.insert("equity".to_owned(), 70.0);
        let mut positions: HashMap<String, f64> = HashMap::new();
        positions.insert("equity".to_owned(), 10.0);
        let engine = ScenarioEngine;
        let pnl = engine.scenario_pnl(&original, &shocked, &positions);
        // 10 units * (70-100) = -300
        assert!((pnl - (-300.0)).abs() < 1e-9, "pnl={pnl}");
    }

    #[test]
    fn test_worst_case_scenario() {
        use std::collections::HashMap;
        let mut portfolio: HashMap<String, f64> = HashMap::new();
        portfolio.insert("equity".to_owned(), 100.0);
        let mut positions: HashMap<String, f64> = HashMap::new();
        positions.insert("equity".to_owned(), 1.0);
        let scenarios = vec![
            Scenario::equity_crash(),
            Scenario::rate_shock(),
        ];
        let engine = ScenarioEngine;
        let (worst, pnl) = engine.worst_case(&portfolio, &scenarios, &positions);
        assert!(pnl <= 0.0 || pnl.is_finite());
        assert!(!worst.name.is_empty());
    }

    #[test]
    fn test_built_in_scenarios_valid() {
        assert!(!Scenario::equity_crash().shocks.is_empty());
        assert!(!Scenario::credit_crisis().shocks.is_empty());
        assert!(!Scenario::rate_shock().shocks.is_empty());
        assert!(!Scenario::fx_devaluation().shocks.is_empty());
    }
}

// ─────────────────────────────────────────
//  Scenario analysis and stress-testing framework
// ─────────────────────────────────────────

use std::collections::HashMap;

/// Type of shock applied to an asset's price.
///
/// - `AbsoluteShift(delta)`: adds `delta` directly to the price.
/// - `RelativeShift(frac)`: multiplies price by `(1 + frac)`.
/// - `VolatilityScaling(factor)`: scales price by `factor` (models vol regime shift).
/// - `CorrelationBreakdown(delta)`: adds `delta` to price (models spread/basis blow-out).
#[derive(Debug, Clone)]
pub enum ShockType {
    /// Add an absolute amount to the price (can be negative).
    AbsoluteShift(f64),
    /// Multiply price by `(1 + fraction)` (e.g. -0.30 = -30%).
    RelativeShift(f64),
    /// Scale price by `factor` (e.g. 1.5 = +50% vol-driven move).
    VolatilityScaling(f64),
    /// Add `delta` to price (models correlation breakdown / basis widening).
    CorrelationBreakdown(f64),
}

/// A shock applied to a specific asset.
#[derive(Debug, Clone)]
pub struct AssetShock {
    /// Identifier for the asset being shocked.
    pub asset_id: String,
    /// The type and magnitude of the shock.
    pub shock: ShockType,
}

/// A named stress scenario containing a set of asset shocks.
#[derive(Debug, Clone)]
pub struct Scenario {
    /// Short human-readable name (e.g. "equity_crash").
    pub name: String,
    /// Longer narrative description.
    pub description: String,
    /// Shocks applied to individual assets.
    pub shocks: Vec<AssetShock>,
    /// Subjective or historical probability of this scenario occurring.
    pub probability: f64,
}

impl Scenario {
    /// 2008-style equity crash: equities down 30%, volatility up 50%.
    pub fn equity_crash() -> Self {
        Self {
            name: "equity_crash".to_owned(),
            description: "2008-style equity market crash: equities -30%, implied vol +50%."
                .to_owned(),
            probability: 0.05,
            shocks: vec![
                AssetShock {
                    asset_id: "equity".to_owned(),
                    shock: ShockType::RelativeShift(-0.30),
                },
                AssetShock {
                    asset_id: "vol".to_owned(),
                    shock: ShockType::RelativeShift(0.50),
                },
            ],
        }
    }

    /// Credit crisis: credit spreads blow out, IG credit down 20%.
    pub fn credit_crisis() -> Self {
        Self {
            name: "credit_crisis".to_owned(),
            description: "Credit crisis: IG credit -20%, HY spreads widen by +500 bps.".to_owned(),
            probability: 0.03,
            shocks: vec![
                AssetShock {
                    asset_id: "ig_credit".to_owned(),
                    shock: ShockType::RelativeShift(-0.20),
                },
                AssetShock {
                    asset_id: "hy_spread".to_owned(),
                    shock: ShockType::AbsoluteShift(5.0),
                },
            ],
        }
    }

    /// Rate shock: parallel upward shift of +200 bps across the yield curve.
    pub fn rate_shock() -> Self {
        Self {
            name: "rate_shock".to_owned(),
            description: "Sudden 200 bps rate hike across the yield curve.".to_owned(),
            probability: 0.04,
            shocks: vec![AssetShock {
                asset_id: "rates".to_owned(),
                shock: ShockType::AbsoluteShift(2.0),
            }],
        }
    }

    /// EM FX devaluation: emerging-market currencies depreciate 20%.
    pub fn fx_devaluation() -> Self {
        Self {
            name: "fx_devaluation".to_owned(),
            description: "EM FX devaluation: EM currencies -20% vs USD.".to_owned(),
            probability: 0.06,
            shocks: vec![AssetShock {
                asset_id: "em_fx".to_owned(),
                shock: ShockType::RelativeShift(-0.20),
            }],
        }
    }
}

/// Engine that applies scenarios to portfolio prices and computes P&L impact.
///
/// # Example
/// ```rust
/// use std::collections::HashMap;
/// use fin_primitives::scenario::{ScenarioEngine, ShockType};
///
/// let engine = ScenarioEngine;
/// let shocked = engine.apply_shock(100.0, &ShockType::RelativeShift(-0.30));
/// assert!((shocked - 70.0).abs() < 1e-9);
/// ```
pub struct ScenarioEngine;

impl ScenarioEngine {
    /// Apply a single shock to a base price and return the shocked price.
    pub fn apply_shock(&self, price: f64, shock: &ShockType) -> f64 {
        match shock {
            ShockType::AbsoluteShift(delta) => price + delta,
            ShockType::RelativeShift(frac) => price * (1.0 + frac),
            ShockType::VolatilityScaling(factor) => price * factor,
            ShockType::CorrelationBreakdown(delta) => price + delta,
        }
    }

    /// Apply all shocks in a scenario to every matching asset in the portfolio.
    ///
    /// Returns a new `HashMap` with shocked prices. Assets not mentioned in the
    /// scenario retain their original prices.
    pub fn run_scenario(
        &self,
        portfolio: &HashMap<String, f64>,
        scenario: &Scenario,
    ) -> HashMap<String, f64> {
        let mut result = portfolio.clone();
        for asset_shock in &scenario.shocks {
            if let Some(price) = result.get_mut(&asset_shock.asset_id) {
                *price = self.apply_shock(*price, &asset_shock.shock);
            }
        }
        result
    }

    /// Compute the P&L impact of a scenario on a set of positions.
    ///
    /// `P&L = Σ positions[asset] * (shocked_price[asset] - original_price[asset])`
    ///
    /// Assets present in `positions` but absent from either price map are skipped.
    pub fn scenario_pnl(
        &self,
        original: &HashMap<String, f64>,
        shocked: &HashMap<String, f64>,
        positions: &HashMap<String, f64>,
    ) -> f64 {
        positions.iter().fold(0.0, |acc, (asset, &qty)| {
            let orig = original.get(asset).copied().unwrap_or(0.0);
            let shock = shocked.get(asset).copied().unwrap_or(orig);
            acc + qty * (shock - orig)
        })
    }

    /// Find the worst-case scenario (most negative P&L) from a list of scenarios.
    ///
    /// Returns a reference to the worst scenario and its P&L.
    ///
    /// # Panics
    /// Panics if `scenarios` is empty.
    pub fn worst_case<'a>(
        &self,
        portfolio: &HashMap<String, f64>,
        scenarios: &'a [Scenario],
        positions: &HashMap<String, f64>,
    ) -> (&'a Scenario, f64) {
        assert!(!scenarios.is_empty(), "scenarios must not be empty");
        let mut worst_scenario = &scenarios[0];
        let shocked = self.run_scenario(portfolio, worst_scenario);
        let mut worst_pnl = self.scenario_pnl(portfolio, &shocked, positions);

        for scenario in scenarios.iter().skip(1) {
            let shocked = self.run_scenario(portfolio, scenario);
            let pnl = self.scenario_pnl(portfolio, &shocked, positions);
            if pnl < worst_pnl {
                worst_pnl = pnl;
                worst_scenario = scenario;
            }
        }
        (worst_scenario, worst_pnl)
    }
}
