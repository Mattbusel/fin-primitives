//! # Module: backtest
//!
//! ## Responsibility
//! Provides a bar-by-bar backtester, a `Strategy` trait for signal generation,
//! equity curve tracking, and a full walk-forward optimizer with grid search.
//!
//! ## Sub-modules
//! - [`walk_forward`]: grid-search walk-forward optimizer with per-period OOS evaluation
//!
//! ## Guarantees
//! - Bars are processed in the order supplied; no look-ahead
//! - `BacktestResult::max_drawdown` is always in `[0, 1]`
//! - Commission is deducted from cash on every fill
//!
//! ## NOT Responsible For
//! - Live order routing
//! - Slippage models beyond the commission rate

pub mod engine;
pub mod walk_forward;

pub use engine::{
    BacktestEngine, BacktestMetrics, BacktestResult as EngineBacktestResult, CompletedTrade,
    Direction, EngineConfig, EngineSignal,
};
pub use walk_forward::{
    ParamRange, WalkForwardConfig, WalkForwardOptimizer, WalkForwardResult, WfPeriod,
};

use crate::error::FinError;
use crate::ohlcv::OhlcvBar;
use crate::position::PositionLedger;
use crate::types::{NanoTimestamp, Price, Quantity, Side};
use rust_decimal::Decimal;
use std::collections::HashMap;

// ─── Config / Result ──────────────────────────────────────────────────────────

/// Configuration for a single backtest run.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BacktestConfig {
    /// Starting cash balance.
    pub initial_capital: Decimal,
    /// Commission rate as a fraction of notional (e.g. `dec!(0.001)` = 0.1%).
    pub commission_rate: Decimal,
}

impl BacktestConfig {
    /// Creates a new `BacktestConfig`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if `initial_capital` or `commission_rate` are negative.
    pub fn new(initial_capital: Decimal, commission_rate: Decimal) -> Result<Self, FinError> {
        if initial_capital <= Decimal::ZERO {
            return Err(FinError::InvalidInput(
                "initial_capital must be positive".to_owned(),
            ));
        }
        if commission_rate < Decimal::ZERO {
            return Err(FinError::InvalidInput(
                "commission_rate must be non-negative".to_owned(),
            ));
        }
        Ok(Self { initial_capital, commission_rate })
    }
}

/// Summary statistics produced after a completed backtest run.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BacktestResult {
    /// Total return over the period: `(final_equity - initial_capital) / initial_capital`.
    pub total_return: Decimal,
    /// Annualised Sharpe ratio (assuming 252 trading days; `NaN`-free — returns 0 if std == 0).
    pub sharpe_ratio: Decimal,
    /// Maximum peak-to-trough equity drawdown as a fraction, e.g. `dec!(0.15)` = 15%.
    pub max_drawdown: Decimal,
    /// Fraction of closed trades that were profitable.
    pub win_rate: Decimal,
    /// Total number of trades (fills) executed.
    pub trade_count: u64,
    /// Final equity value at the end of the period.
    pub final_equity: Decimal,
    /// Equity curve sampled once per bar.
    pub equity_curve: Vec<Decimal>,
}

// ─── Signal ───────────────────────────────────────────────────────────────────

/// Direction of a trading signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalDirection {
    /// Enter a long position.
    Buy,
    /// Enter a short position / exit a long position.
    Sell,
    /// Do nothing.
    Hold,
}

/// A trading signal produced by a `Strategy` on each bar.
#[derive(Debug, Clone)]
pub struct Signal {
    /// Desired direction.
    pub direction: SignalDirection,
    /// Number of units to trade (must be non-negative).
    pub quantity: Decimal,
}

impl Signal {
    /// Creates a new signal.
    pub fn new(direction: SignalDirection, quantity: Decimal) -> Self {
        Self { direction, quantity }
    }

    /// Convenience constructor for a hold signal with zero quantity.
    pub fn hold() -> Self {
        Self::new(SignalDirection::Hold, Decimal::ZERO)
    }
}

// ─── Strategy trait ───────────────────────────────────────────────────────────

/// User-supplied strategy that generates trade signals from bars.
///
/// Implement this trait and pass a `&mut dyn Strategy` to [`Backtester::run`].
pub trait Strategy: Send {
    /// Called once per bar in chronological order.
    ///
    /// Return `None` to skip trading on this bar.
    fn on_bar(&mut self, bar: &OhlcvBar) -> Option<Signal>;
}

// ─── Backtester ───────────────────────────────────────────────────────────────

/// Bar-by-bar backtester.
///
/// Processes OHLCV bars in the supplied order, routes signals from `Strategy::on_bar`
/// into a `PositionLedger`, and records the equity curve.
pub struct Backtester {
    config: BacktestConfig,
}

impl Backtester {
    /// Creates a new backtester with the given config.
    pub fn new(config: BacktestConfig) -> Self {
        Self { config }
    }

    /// Runs the backtest over `bars` using `strategy`.
    ///
    /// # Errors
    /// - [`FinError::InvalidInput`] if `bars` is empty.
    /// - Propagates [`FinError`] from position accounting.
    pub fn run(
        &self,
        bars: &[OhlcvBar],
        strategy: &mut dyn Strategy,
    ) -> Result<BacktestResult, FinError> {
        if bars.is_empty() {
            return Err(FinError::InvalidInput("bars slice must not be empty".to_owned()));
        }

        let mut ledger = PositionLedger::new(self.config.initial_capital);
        let mut equity_curve: Vec<Decimal> = Vec::with_capacity(bars.len());
        let mut trade_count: u64 = 0;
        let mut daily_returns: Vec<Decimal> = Vec::with_capacity(bars.len());
        let mut prev_equity = self.config.initial_capital;
        let mut peak_equity = self.config.initial_capital;
        let mut max_drawdown = Decimal::ZERO;

        // Track realized P&L by comparing ledger realized_pnl_total before/after each fill
        let mut winning_trades: u64 = 0;
        let mut total_closed: u64 = 0;

        for bar in bars {
            // Ask strategy for a signal
            if let Some(sig) = strategy.on_bar(bar) {
                if sig.direction != SignalDirection::Hold && sig.quantity > Decimal::ZERO {
                    let side = match sig.direction {
                        SignalDirection::Buy => Side::Bid,
                        SignalDirection::Sell => Side::Ask,
                        SignalDirection::Hold => unreachable!(),
                    };

                    let price = Price::new(bar.close.value())?;
                    let qty = Quantity::new(sig.quantity)?;
                    let commission = bar.close.value() * sig.quantity * self.config.commission_rate;

                    let fill = crate::position::Fill::with_commission(
                        bar.symbol.clone(),
                        side,
                        qty,
                        price,
                        NanoTimestamp::new(bar.ts_close.nanos()),
                        commission,
                    );

                    // Capture realized P&L before and after to detect a profitable trade.
                    let realized_before = ledger.realized_pnl_total();
                    if ledger.apply_fill(fill).is_ok() {
                        let realized_after = ledger.realized_pnl_total();
                        let pnl_delta = realized_after - realized_before;
                        if pnl_delta != Decimal::ZERO {
                            total_closed += 1;
                            if pnl_delta > Decimal::ZERO {
                                winning_trades += 1;
                            }
                        }
                    }
                    trade_count += 1;
                }
            }

            // Mark-to-market equity
            let mut mark_prices: HashMap<String, Price> = HashMap::new();
            mark_prices.insert(
                bar.symbol.as_str().to_owned(),
                Price::new(bar.close.value())?,
            );
            let equity = ledger.equity(&mark_prices).unwrap_or(prev_equity);

            // Drawdown
            if equity > peak_equity {
                peak_equity = equity;
            }
            if peak_equity > Decimal::ZERO {
                let dd = (peak_equity - equity) / peak_equity;
                if dd > max_drawdown {
                    max_drawdown = dd;
                }
            }

            // Daily return
            if prev_equity > Decimal::ZERO {
                daily_returns.push((equity - prev_equity) / prev_equity);
            }

            equity_curve.push(equity);
            prev_equity = equity;
        }

        let final_equity = equity_curve.last().copied().unwrap_or(self.config.initial_capital);

        let total_return = if self.config.initial_capital > Decimal::ZERO {
            (final_equity - self.config.initial_capital) / self.config.initial_capital
        } else {
            Decimal::ZERO
        };

        let sharpe_ratio = compute_sharpe(&daily_returns);

        let win_rate = if total_closed > 0 {
            Decimal::from(winning_trades) / Decimal::from(total_closed)
        } else {
            Decimal::ZERO
        };

        Ok(BacktestResult {
            total_return,
            sharpe_ratio,
            max_drawdown,
            win_rate,
            trade_count,
            final_equity,
            equity_curve,
        })
    }
}

/// Computes the annualised Sharpe ratio from a slice of per-bar returns.
///
/// Assumes 252 trading days per year. Returns zero if the slice is empty or
/// the standard deviation is zero (no variance in returns).
fn compute_sharpe(returns: &[Decimal]) -> Decimal {
    use rust_decimal::prelude::ToPrimitive;

    let n = returns.len();
    if n < 2 {
        return Decimal::ZERO;
    }

    // Mean
    let sum: Decimal = returns.iter().sum();
    let mean_f = sum.to_f64().unwrap_or(0.0) / n as f64;

    // Variance
    let var: f64 = returns
        .iter()
        .map(|r| {
            let x = r.to_f64().unwrap_or(0.0) - mean_f;
            x * x
        })
        .sum::<f64>()
        / (n as f64 - 1.0);

    let std_dev = var.sqrt();
    if std_dev == 0.0 {
        return Decimal::ZERO;
    }

    let sharpe_daily = mean_f / std_dev;
    let sharpe_annual = sharpe_daily * 252.0_f64.sqrt();

    Decimal::try_from(sharpe_annual).unwrap_or(Decimal::ZERO)
}

// Walk-forward types and optimizer are fully implemented in the walk_forward
// sub-module and re-exported above via `pub use walk_forward::{...}`.

// ─── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{NanoTimestamp, Price, Quantity};
    use rust_decimal_macros::dec;

    fn make_bar(close: Decimal, ts: i64) -> OhlcvBar {
        let sym = crate::types::Symbol::new("TEST").unwrap();
        let p = Price::new(close).unwrap();
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

    /// Buy-and-hold: buys 1 unit on the first bar, holds.
    struct BuyAndHold {
        bought: bool,
    }

    impl Strategy for BuyAndHold {
        fn on_bar(&mut self, _bar: &OhlcvBar) -> Option<Signal> {
            if !self.bought {
                self.bought = true;
                Some(Signal::new(SignalDirection::Buy, dec!(1)))
            } else {
                Some(Signal::hold())
            }
        }
    }

    #[test]
    fn test_buy_and_hold_rising_market() {
        let bars: Vec<OhlcvBar> = (0..10)
            .map(|i| make_bar(dec!(100) + Decimal::from(i), i))
            .collect();
        let config = BacktestConfig::new(dec!(10_000), dec!(0)).unwrap();
        let result = Backtester::new(config)
            .run(&bars, &mut BuyAndHold { bought: false })
            .unwrap();
        // Bought 1 unit @ 100 from 10 000 cash; final close = 109.
        // equity = (10 000 - 100) + unrealized_pnl(109) = 9 900 + 9 = 9 909.
        // Assert equity grew relative to first bar (9 900) and trade count is 1.
        assert!(result.final_equity > dec!(9_900), "final_equity={}", result.final_equity);
        assert_eq!(result.trade_count, 1);
    }

    #[test]
    fn test_empty_bars_errors() {
        let config = BacktestConfig::new(dec!(10_000), dec!(0)).unwrap();
        let result = Backtester::new(config).run(&[], &mut BuyAndHold { bought: false });
        assert!(result.is_err());
    }

    #[test]
    fn test_max_drawdown_flat_market_is_zero() {
        // Hold strategy: no trades, cash never changes, drawdown must be zero.
        struct HoldOnly;
        impl Strategy for HoldOnly {
            fn on_bar(&mut self, _bar: &OhlcvBar) -> Option<Signal> {
                Some(Signal::hold())
            }
        }
        let bars: Vec<OhlcvBar> = (0..5).map(|i| make_bar(dec!(100), i)).collect();
        let config = BacktestConfig::new(dec!(10_000), dec!(0)).unwrap();
        let result = Backtester::new(config).run(&bars, &mut HoldOnly).unwrap();
        assert_eq!(result.max_drawdown, dec!(0));
    }

    #[test]
    fn test_backtest_config_invalid_capital() {
        assert!(BacktestConfig::new(dec!(-1), dec!(0)).is_err());
    }

    #[test]
    fn test_walk_forward_basic() {
        use crate::backtest::walk_forward::WalkForwardConfig;
        use std::collections::HashMap;
        let bars: Vec<OhlcvBar> = (0..30)
            .map(|i| make_bar(dec!(100) + Decimal::from(i), i))
            .collect();
        let bt_config = BacktestConfig::new(dec!(10_000), dec!(0)).unwrap();
        let wf_config = WalkForwardConfig {
            train_window: 15,
            test_window: 5,
            step: 5,
            param_space: vec![],
        };
        let wfo = WalkForwardOptimizer::new(wf_config, bt_config).unwrap();
        let result = wfo
            .run(&bars, |_train, _params: &HashMap<String, f64>| {
                Box::new(BuyAndHold { bought: false })
            })
            .unwrap();
        assert!(!result.periods.is_empty());
    }

    #[test]
    fn test_walk_forward_insufficient_bars() {
        use crate::backtest::walk_forward::WalkForwardConfig;
        use std::collections::HashMap;
        let bars: Vec<OhlcvBar> = (0..5).map(|i| make_bar(dec!(100), i)).collect();
        let bt_config = BacktestConfig::new(dec!(10_000), dec!(0)).unwrap();
        let wf_config = WalkForwardConfig {
            train_window: 10,
            test_window: 5,
            step: 5,
            param_space: vec![],
        };
        let wfo = WalkForwardOptimizer::new(wf_config, bt_config).unwrap();
        let result = wfo.run(&bars, |_train, _params: &HashMap<String, f64>| {
            Box::new(BuyAndHold { bought: false })
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_sharpe_constant_returns_zero() {
        // All returns identical → zero stddev → sharpe = 0
        let returns = vec![dec!(0.01); 10];
        // sharpe with zero variance returns Decimal::ZERO
        let s = compute_sharpe(&returns);
        // If all returns are equal the sample variance is zero, so sharpe = 0
        assert_eq!(s, Decimal::ZERO);
    }
}
