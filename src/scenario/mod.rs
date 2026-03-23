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
}
