//! # Walk-Forward Optimizer
//!
//! ## Responsibility
//! Provides rigorous out-of-sample validation by splitting a bar series into
//! rolling train/test windows, optimizing strategy parameters on the training
//! slice (in-sample), and evaluating the result on the held-out test slice
//! (out-of-sample).
//!
//! ## Algorithm
//!
//! ```text
//! |─── train ───|─ test ─|
//!     step ──►
//!              |─── train ───|─ test ─|
//!                  step ──►
//!                           |─── train ───|─ test ─|
//! ```
//!
//! Grid search is performed over every combination of parameter values defined
//! in [`ParamRange`]. The combination with the highest in-sample Sharpe ratio
//! is selected; then the same strategy is evaluated on the out-of-sample test
//! slice, and the result is recorded in [`WfPeriod`].
//!
//! ## Stability Score
//!
//! The `stability_score` in [`WalkForwardResult`] measures how consistently
//! out-of-sample Sharpe ratios are positive across periods:
//!
//! ```text
//! stability_score = (# periods with OOS Sharpe > 0) / (total periods)
//! ```
//!
//! A score of 1.0 means every OOS window was profitable; 0.0 means none were.
//!
//! ## Example
//!
//! ```rust
//! use fin_primitives::backtest::walk_forward::{
//!     WalkForwardOptimizer, WalkForwardConfig, ParamRange,
//! };
//! use fin_primitives::backtest::{BacktestConfig, Signal, SignalDirection, Strategy};
//! use fin_primitives::ohlcv::OhlcvBar;
//! use std::collections::HashMap;
//! use rust_decimal_macros::dec;
//!
//! # fn make_bar(c: f64, ts: i64) -> OhlcvBar {
//! #   use fin_primitives::types::{NanoTimestamp, Price, Quantity, Symbol};
//! #   let sym = Symbol::new("T").unwrap();
//! #   let p = Price::new(rust_decimal::Decimal::try_from(c).unwrap()).unwrap();
//! #   OhlcvBar { symbol: sym, open: p, high: p, low: p, close: p,
//! #     volume: Quantity::new(dec!(100)).unwrap(),
//! #     ts_open: NanoTimestamp(ts), ts_close: NanoTimestamp(ts+1), tick_count: 1 }
//! # }
//! let bars: Vec<OhlcvBar> = (0..200).map(|i| make_bar(100.0 + i as f64 * 0.1, i)).collect();
//!
//! let config = WalkForwardConfig {
//!     train_window: 60,
//!     test_window: 20,
//!     step: 20,
//!     param_space: vec![
//!         ParamRange { name: "sma_period".to_owned(), min: 5.0, max: 20.0, step: 5.0 },
//!     ],
//! };
//!
//! let bt_config = BacktestConfig::new(dec!(10_000), dec!(0.001)).unwrap();
//! let opt = WalkForwardOptimizer::new(config, bt_config).unwrap();
//!
//! let result = opt.run(&bars, |train, params| {
//!     let _period = params.get("sma_period").copied().unwrap_or(10.0) as usize;
//!     Box::new(HoldStrategy)
//! }).unwrap();
//!
//! println!("Aggregate Sharpe: {:.2}", result.aggregate_sharpe);
//! println!("Stability score:  {:.2}", result.stability_score);
//!
//! # struct HoldStrategy;
//! # impl Strategy for HoldStrategy {
//! #   fn on_bar(&mut self, _b: &OhlcvBar) -> Option<Signal> { None }
//! # }
//! ```

use crate::backtest::{BacktestConfig, Backtester, BacktestResult, Strategy};
use crate::error::FinError;
use crate::ohlcv::OhlcvBar;
use rust_decimal::Decimal;
use std::collections::HashMap;

// ─── ParamRange ───────────────────────────────────────────────────────────────

/// A single named parameter range for grid search.
///
/// The grid is `[min, min+step, min+2*step, ..., max]` (inclusive).
/// At least one value is always produced (when `min == max`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParamRange {
    /// Name of the parameter (e.g. `"rsi_period"`, `"sma_period"`).
    pub name: String,
    /// Minimum value (inclusive).
    pub min: f64,
    /// Maximum value (inclusive, up to floating-point precision).
    pub max: f64,
    /// Grid step size. Must be positive.
    pub step: f64,
}

impl ParamRange {
    /// Enumerates all grid values for this range.
    ///
    /// Returns `[min, min+step, min+2*step, ..., max]`.
    /// If `step <= 0` or `min > max`, returns `[min]` as a degenerate case.
    pub fn values(&self) -> Vec<f64> {
        if self.step <= 0.0 || self.min > self.max {
            return vec![self.min];
        }
        let mut vals = Vec::new();
        let mut v = self.min;
        while v <= self.max + f64::EPSILON {
            vals.push(v);
            v += self.step;
        }
        vals
    }
}

// ─── WalkForwardConfig ────────────────────────────────────────────────────────

/// Configuration for a walk-forward optimization run.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WalkForwardConfig {
    /// Number of bars in each training (in-sample) window.
    pub train_window: usize,
    /// Number of bars in each test (out-of-sample) window.
    pub test_window: usize,
    /// Number of bars to advance the window on each step.
    /// Typically set to `test_window` for non-overlapping OOS periods.
    pub step: usize,
    /// Parameter space to search over. Each element defines one axis.
    pub param_space: Vec<ParamRange>,
}

impl WalkForwardConfig {
    /// Validates the configuration.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if `train_window`, `test_window`,
    /// or `step` are zero.
    pub fn validate(&self) -> Result<(), FinError> {
        if self.train_window == 0 {
            return Err(FinError::InvalidInput(
                "train_window must be > 0".to_owned(),
            ));
        }
        if self.test_window == 0 {
            return Err(FinError::InvalidInput(
                "test_window must be > 0".to_owned(),
            ));
        }
        if self.step == 0 {
            return Err(FinError::InvalidInput(
                "step must be > 0".to_owned(),
            ));
        }
        Ok(())
    }
}

// ─── WfPeriod ─────────────────────────────────────────────────────────────────

/// Results for a single walk-forward period.
///
/// Each period corresponds to one train/test split. The `best_params`
/// are the parameter combination that maximized in-sample Sharpe ratio.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WfPeriod {
    /// Bar index of the first bar in the training window.
    pub train_start: usize,
    /// Bar index one past the last bar in the training window (exclusive).
    pub train_end: usize,
    /// Bar index of the first bar in the test window.
    pub test_start: usize,
    /// Bar index one past the last bar in the test window (exclusive).
    pub test_end: usize,
    /// Parameter values that achieved the best in-sample Sharpe.
    pub best_params: HashMap<String, f64>,
    /// Sharpe ratio achieved on the training (in-sample) window.
    pub in_sample_sharpe: f64,
    /// Sharpe ratio achieved on the test (out-of-sample) window.
    pub out_of_sample_sharpe: f64,
    /// Full backtest result on the out-of-sample window.
    pub oos_result: BacktestResult,
}

// ─── WalkForwardResult ────────────────────────────────────────────────────────

/// Aggregated output of a walk-forward optimization run.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WalkForwardResult {
    /// Per-period details.
    pub periods: Vec<WfPeriod>,
    /// Mean out-of-sample Sharpe ratio across all periods.
    pub aggregate_sharpe: f64,
    /// Fraction of periods in which the OOS Sharpe was positive.
    ///
    /// Range: `[0.0, 1.0]`. 1.0 = every OOS window was profitable.
    pub stability_score: f64,
    /// Mean out-of-sample total return across all periods.
    pub mean_oos_return: f64,
    /// Worst (maximum) out-of-sample drawdown across all periods.
    pub worst_oos_drawdown: f64,
}

impl WalkForwardResult {
    /// Returns `true` if the strategy passed a basic robustness check:
    /// aggregate Sharpe > 0 and stability score >= `min_stability`.
    pub fn is_robust(&self, min_stability: f64) -> bool {
        self.aggregate_sharpe > 0.0 && self.stability_score >= min_stability
    }

    /// Returns the period with the highest out-of-sample Sharpe ratio.
    pub fn best_period(&self) -> Option<&WfPeriod> {
        self.periods
            .iter()
            .max_by(|a, b| a.out_of_sample_sharpe.partial_cmp(&b.out_of_sample_sharpe).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// Returns the period with the lowest (worst) out-of-sample Sharpe ratio.
    pub fn worst_period(&self) -> Option<&WfPeriod> {
        self.periods
            .iter()
            .min_by(|a, b| a.out_of_sample_sharpe.partial_cmp(&b.out_of_sample_sharpe).unwrap_or(std::cmp::Ordering::Equal))
    }
}

// ─── WalkForwardOptimizer ─────────────────────────────────────────────────────

/// Walk-forward optimizer: splits bars into rolling train/test windows,
/// runs a grid search on training data, and evaluates the winning
/// parameters on held-out test data.
///
/// The strategy factory closure receives the training bar slice and the
/// candidate parameter `HashMap<String, f64>`. It should return a
/// `Box<dyn Strategy>` initialized with those parameters.
///
/// # Example
/// See the [module-level documentation](self) for a complete example.
pub struct WalkForwardOptimizer {
    config: WalkForwardConfig,
    bt_config: BacktestConfig,
}

impl WalkForwardOptimizer {
    /// Constructs a new optimizer.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if the config is invalid.
    pub fn new(config: WalkForwardConfig, bt_config: BacktestConfig) -> Result<Self, FinError> {
        config.validate()?;
        Ok(Self { config, bt_config })
    }

    /// Runs the walk-forward optimization.
    ///
    /// `make_strategy` is called once per (parameter combination × window).
    /// It receives:
    /// - `train_bars`: the training slice for fitting
    /// - `params`: a `HashMap<String, f64>` with the current grid point
    ///
    /// It must return a `Box<dyn Strategy>` configured with `params`.
    ///
    /// # Errors
    /// - [`FinError::InvalidInput`] if `bars` is too short for one window.
    /// - Propagates any [`FinError`] from individual backtest runs.
    pub fn run<F>(
        &self,
        bars: &[OhlcvBar],
        mut make_strategy: F,
    ) -> Result<WalkForwardResult, FinError>
    where
        F: FnMut(&[OhlcvBar], &HashMap<String, f64>) -> Box<dyn Strategy>,
    {
        let window = self.config.train_window + self.config.test_window;
        if bars.len() < window {
            return Err(FinError::InvalidInput(format!(
                "need at least {} bars for one walk-forward window, got {}",
                window,
                bars.len()
            )));
        }

        let backtester = Backtester::new(self.bt_config.clone());
        let grid = build_grid(&self.config.param_space);
        let mut periods: Vec<WfPeriod> = Vec::new();
        let mut offset = 0usize;

        while offset + window <= bars.len() {
            let train_start = offset;
            let train_end = offset + self.config.train_window;
            let test_start = train_end;
            let test_end = train_end + self.config.test_window;

            let train_bars = &bars[train_start..train_end];
            let test_bars = &bars[test_start..test_end];

            // ── Grid search on training window ────────────────────────────────
            let mut best_is_sharpe = f64::NEG_INFINITY;
            let mut best_params: HashMap<String, f64> = HashMap::new();
            let mut best_oos_result: Option<BacktestResult> = None;

            let search_grid: &[HashMap<String, f64>] = &grid;

            for param_set in search_grid {
                // In-sample evaluation
                let mut is_strategy = make_strategy(train_bars, param_set);
                let is_result = match backtester.run(train_bars, is_strategy.as_mut()) {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                let is_sharpe = is_result
                    .sharpe_ratio
                    .to_string()
                    .parse::<f64>()
                    .unwrap_or(f64::NEG_INFINITY);

                if is_sharpe > best_is_sharpe {
                    best_is_sharpe = is_sharpe;
                    best_params = param_set.clone();
                }
            }

            // ── Out-of-sample evaluation with best params ─────────────────────
            let mut oos_strategy = make_strategy(test_bars, &best_params);
            let oos_result = backtester.run(test_bars, oos_strategy.as_mut())?;

            let oos_sharpe = oos_result
                .sharpe_ratio
                .to_string()
                .parse::<f64>()
                .unwrap_or(0.0);

            best_oos_result = Some(oos_result.clone());

            periods.push(WfPeriod {
                train_start,
                train_end,
                test_start,
                test_end,
                best_params,
                in_sample_sharpe: best_is_sharpe.max(0.0), // clamp for display
                out_of_sample_sharpe: oos_sharpe,
                oos_result: best_oos_result.unwrap_or(oos_result),
            });

            offset += self.config.step;
        }

        if periods.is_empty() {
            return Err(FinError::InvalidInput(
                "no walk-forward periods could be constructed".to_owned(),
            ));
        }

        // ── Aggregate metrics ─────────────────────────────────────────────────
        let n = periods.len() as f64;
        let aggregate_sharpe = periods.iter().map(|p| p.out_of_sample_sharpe).sum::<f64>() / n;
        let positive_count = periods.iter().filter(|p| p.out_of_sample_sharpe > 0.0).count();
        let stability_score = positive_count as f64 / n;

        let mean_oos_return = periods
            .iter()
            .map(|p| {
                p.oos_result
                    .total_return
                    .to_string()
                    .parse::<f64>()
                    .unwrap_or(0.0)
            })
            .sum::<f64>()
            / n;

        let worst_oos_drawdown = periods
            .iter()
            .map(|p| {
                p.oos_result
                    .max_drawdown
                    .to_string()
                    .parse::<f64>()
                    .unwrap_or(0.0)
            })
            .fold(0.0_f64, f64::max);

        Ok(WalkForwardResult {
            periods,
            aggregate_sharpe,
            stability_score,
            mean_oos_return,
            worst_oos_drawdown,
        })
    }

    /// Returns the configuration.
    pub fn config(&self) -> &WalkForwardConfig {
        &self.config
    }

    /// Returns the backtest configuration.
    pub fn bt_config(&self) -> &BacktestConfig {
        &self.bt_config
    }
}

// ─── Grid builder ─────────────────────────────────────────────────────────────

/// Builds the full Cartesian product of all parameter ranges.
///
/// Each element of the returned Vec is a `HashMap<String, f64>` mapping
/// parameter names to their values for one grid point.
fn build_grid(param_space: &[ParamRange]) -> Vec<HashMap<String, f64>> {
    if param_space.is_empty() {
        return vec![HashMap::new()];
    }

    let mut grid: Vec<HashMap<String, f64>> = vec![HashMap::new()];

    for param in param_space {
        let vals = param.values();
        let mut new_grid = Vec::with_capacity(grid.len() * vals.len());
        for existing in &grid {
            for &v in &vals {
                let mut m = existing.clone();
                m.insert(param.name.clone(), v);
                new_grid.push(m);
            }
        }
        grid = new_grid;
    }

    grid
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backtest::{Signal, SignalDirection};
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn make_bar(close: f64, ts: i64) -> OhlcvBar {
        let sym = Symbol::new("TEST").unwrap();
        let p = Price::new(Decimal::try_from(close).unwrap()).unwrap();
        OhlcvBar {
            symbol: sym,
            open: p,
            high: p,
            low: p,
            close: p,
            volume: Quantity::new(dec!(1000)).unwrap(),
            ts_open: NanoTimestamp::new(ts),
            ts_close: NanoTimestamp::new(ts + 1),
            tick_count: 1,
        }
    }

    /// Strategy that always holds.
    struct HoldAll;
    impl Strategy for HoldAll {
        fn on_bar(&mut self, _: &OhlcvBar) -> Option<Signal> {
            None
        }
    }

    /// Strategy that buys on bar 0 then holds, configurable by params.
    struct BuyOnce {
        bought: bool,
        qty: f64,
    }
    impl BuyOnce {
        fn from_params(params: &HashMap<String, f64>) -> Self {
            Self {
                bought: false,
                qty: params.get("qty").copied().unwrap_or(1.0),
            }
        }
    }
    impl Strategy for BuyOnce {
        fn on_bar(&mut self, _: &OhlcvBar) -> Option<Signal> {
            if !self.bought {
                self.bought = true;
                let qty = Decimal::try_from(self.qty).unwrap_or(dec!(1));
                return Some(Signal::new(SignalDirection::Buy, qty));
            }
            Some(Signal::hold())
        }
    }

    // ── ParamRange ────────────────────────────────────────────────────────────

    #[test]
    fn test_param_range_values_basic() {
        let r = ParamRange { name: "x".to_owned(), min: 1.0, max: 3.0, step: 1.0 };
        let vals = r.values();
        assert_eq!(vals.len(), 3);
        assert!((vals[0] - 1.0).abs() < 1e-10);
        assert!((vals[2] - 3.0).abs() < 1e-10);
    }

    #[test]
    fn test_param_range_single_value_when_min_equals_max() {
        let r = ParamRange { name: "x".to_owned(), min: 5.0, max: 5.0, step: 1.0 };
        let vals = r.values();
        assert_eq!(vals.len(), 1);
        assert!((vals[0] - 5.0).abs() < 1e-10);
    }

    #[test]
    fn test_param_range_degenerate_step() {
        let r = ParamRange { name: "x".to_owned(), min: 1.0, max: 5.0, step: 0.0 };
        let vals = r.values();
        assert_eq!(vals.len(), 1); // degenerate fallback
    }

    // ── build_grid ────────────────────────────────────────────────────────────

    #[test]
    fn test_build_grid_empty_space() {
        let grid = build_grid(&[]);
        assert_eq!(grid.len(), 1);
        assert!(grid[0].is_empty());
    }

    #[test]
    fn test_build_grid_single_param() {
        let params = vec![ParamRange { name: "p".to_owned(), min: 5.0, max: 15.0, step: 5.0 }];
        let grid = build_grid(&params);
        assert_eq!(grid.len(), 3); // 5, 10, 15
        for m in &grid {
            assert!(m.contains_key("p"));
        }
    }

    #[test]
    fn test_build_grid_two_params_cartesian() {
        let params = vec![
            ParamRange { name: "a".to_owned(), min: 1.0, max: 2.0, step: 1.0 },
            ParamRange { name: "b".to_owned(), min: 10.0, max: 20.0, step: 10.0 },
        ];
        let grid = build_grid(&params);
        assert_eq!(grid.len(), 4); // 2 × 2
    }

    // ── WalkForwardConfig ─────────────────────────────────────────────────────

    #[test]
    fn test_config_validation_zero_train() {
        let cfg = WalkForwardConfig {
            train_window: 0,
            test_window: 20,
            step: 20,
            param_space: vec![],
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_config_validation_zero_test() {
        let cfg = WalkForwardConfig {
            train_window: 60,
            test_window: 0,
            step: 20,
            param_space: vec![],
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_config_validation_zero_step() {
        let cfg = WalkForwardConfig {
            train_window: 60,
            test_window: 20,
            step: 0,
            param_space: vec![],
        };
        assert!(cfg.validate().is_err());
    }

    // ── WalkForwardOptimizer ──────────────────────────────────────────────────

    #[test]
    fn test_optimizer_too_few_bars() {
        let bars: Vec<OhlcvBar> = (0..5).map(|i| make_bar(100.0, i)).collect();
        let cfg = WalkForwardConfig {
            train_window: 60,
            test_window: 20,
            step: 20,
            param_space: vec![],
        };
        let bt_cfg = BacktestConfig::new(dec!(10_000), dec!(0)).unwrap();
        let opt = WalkForwardOptimizer::new(cfg, bt_cfg).unwrap();
        let result = opt.run(&bars, |_, _| Box::new(HoldAll));
        assert!(result.is_err());
    }

    #[test]
    fn test_optimizer_hold_strategy_returns_result() {
        let bars: Vec<OhlcvBar> = (0..100).map(|i| make_bar(100.0 + i as f64 * 0.1, i)).collect();
        let cfg = WalkForwardConfig {
            train_window: 40,
            test_window: 20,
            step: 20,
            param_space: vec![],
        };
        let bt_cfg = BacktestConfig::new(dec!(10_000), dec!(0)).unwrap();
        let opt = WalkForwardOptimizer::new(cfg, bt_cfg).unwrap();
        let result = opt.run(&bars, |_, _| Box::new(HoldAll)).unwrap();
        assert!(!result.periods.is_empty());
        // Hold strategy → Sharpe = 0
        assert_eq!(result.aggregate_sharpe, 0.0);
        assert_eq!(result.stability_score, 0.0);
    }

    #[test]
    fn test_optimizer_with_param_grid() {
        let bars: Vec<OhlcvBar> = (0..120).map(|i| make_bar(100.0 + i as f64 * 0.5, i)).collect();
        let cfg = WalkForwardConfig {
            train_window: 50,
            test_window: 20,
            step: 20,
            param_space: vec![
                ParamRange { name: "qty".to_owned(), min: 1.0, max: 3.0, step: 1.0 },
            ],
        };
        let bt_cfg = BacktestConfig::new(dec!(10_000), dec!(0)).unwrap();
        let opt = WalkForwardOptimizer::new(cfg, bt_cfg).unwrap();
        let result = opt.run(&bars, |_, params| Box::new(BuyOnce::from_params(params))).unwrap();
        assert!(!result.periods.is_empty());
        for p in &result.periods {
            assert!(!p.best_params.is_empty());
            assert!(p.best_params.contains_key("qty"));
        }
    }

    #[test]
    fn test_optimizer_stability_score_bounds() {
        let bars: Vec<OhlcvBar> = (0..100).map(|i| make_bar(100.0, i)).collect();
        let cfg = WalkForwardConfig {
            train_window: 40,
            test_window: 20,
            step: 20,
            param_space: vec![],
        };
        let bt_cfg = BacktestConfig::new(dec!(10_000), dec!(0)).unwrap();
        let opt = WalkForwardOptimizer::new(cfg, bt_cfg).unwrap();
        let result = opt.run(&bars, |_, _| Box::new(HoldAll)).unwrap();
        assert!((0.0..=1.0).contains(&result.stability_score));
    }

    #[test]
    fn test_wf_result_robustness_check() {
        let result = WalkForwardResult {
            periods: vec![],
            aggregate_sharpe: 1.5,
            stability_score: 0.8,
            mean_oos_return: 0.05,
            worst_oos_drawdown: 0.1,
        };
        assert!(result.is_robust(0.7));
        assert!(!result.is_robust(0.9));
    }

    #[test]
    fn test_wf_result_best_worst_period() {
        let make_period = |oos_sharpe: f64| WfPeriod {
            train_start: 0,
            train_end: 50,
            test_start: 50,
            test_end: 70,
            best_params: HashMap::new(),
            in_sample_sharpe: 1.0,
            out_of_sample_sharpe: oos_sharpe,
            oos_result: crate::backtest::BacktestResult {
                total_return: Decimal::ZERO,
                sharpe_ratio: Decimal::ZERO,
                max_drawdown: Decimal::ZERO,
                win_rate: Decimal::ZERO,
                trade_count: 0,
                final_equity: dec!(10_000),
                equity_curve: vec![],
            },
        };
        let result = WalkForwardResult {
            periods: vec![make_period(0.5), make_period(2.0), make_period(-0.3)],
            aggregate_sharpe: 0.73,
            stability_score: 0.67,
            mean_oos_return: 0.0,
            worst_oos_drawdown: 0.0,
        };
        assert!((result.best_period().unwrap().out_of_sample_sharpe - 2.0).abs() < 1e-10);
        assert!((result.worst_period().unwrap().out_of_sample_sharpe + 0.3).abs() < 1e-10);
    }

    #[test]
    fn test_optimizer_step_advances_window() {
        // With step < test_window, windows overlap
        let bars: Vec<OhlcvBar> = (0..150).map(|i| make_bar(100.0 + i as f64 * 0.1, i)).collect();
        let cfg = WalkForwardConfig {
            train_window: 50,
            test_window: 30,
            step: 10, // smaller than test_window → overlapping OOS
            param_space: vec![],
        };
        let bt_cfg = BacktestConfig::new(dec!(10_000), dec!(0)).unwrap();
        let opt = WalkForwardOptimizer::new(cfg, bt_cfg).unwrap();
        let result = opt.run(&bars, |_, _| Box::new(HoldAll)).unwrap();
        // Should produce multiple periods due to small step
        assert!(result.periods.len() > 2);
    }
}
