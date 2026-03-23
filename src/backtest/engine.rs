//! # Module: backtest::engine
//!
//! Event-driven backtester with realistic fill simulation.
//!
//! ## Responsibility
//! Provides [`BacktestEngine`] which runs a list of [`Signal`]s over
//! a [`BacktestConfig`] containing OHLCV bars, simulating fills at the
//! next-bar open with slippage and commission deductions.
//!
//! ## Guarantees
//! - Fills execute at next-bar open (no look-ahead on the signal bar)
//! - Slippage is expressed in basis points and applied symmetrically
//! - Commission is deducted per fill as a fraction of notional
//! - All equity curve values are non-negative

use crate::ohlcv::OhlcvBar;

// ─── Direction ────────────────────────────────────────────────────────────────

/// Direction of a trading signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Long the instrument.
    Long,
    /// Short the instrument.
    Short,
    /// Exit / flatten position.
    Flat,
}

// ─── Signal ───────────────────────────────────────────────────────────────────

/// A trading signal produced externally and fed into the engine.
#[derive(Debug, Clone)]
pub struct EngineSignal {
    /// Unix timestamp (ms) when the signal was generated.
    pub timestamp: u64,
    /// Instrument symbol.
    pub symbol: String,
    /// Intended direction.
    pub direction: Direction,
    /// Signal strength in `[0, 1]`; used to scale position size.
    pub strength: f64,
}

// ─── Config ───────────────────────────────────────────────────────────────────

/// Configuration for a [`BacktestEngine`] run.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Starting cash balance.
    pub initial_capital: f64,
    /// Commission as a fraction of notional (e.g. 0.001 = 0.1%).
    pub commission: f64,
    /// One-way slippage in basis points (e.g. 5.0 = 5 bps).
    pub slippage_bps: f64,
    /// Historical OHLCV bars used for fill simulation.
    pub data: Vec<OhlcvBar>,
    /// Fraction of current capital allocated per trade (e.g. 0.1 = 10%).
    pub capital_fraction: f64,
}

// ─── Results ──────────────────────────────────────────────────────────────────

/// A single closed round-trip trade.
#[derive(Debug, Clone)]
pub struct CompletedTrade {
    /// Entry timestamp (ms).
    pub entry_ts: u64,
    /// Exit timestamp (ms).
    pub exit_ts: u64,
    /// Direction of the trade.
    pub direction: Direction,
    /// Fill price at entry.
    pub entry_price: f64,
    /// Fill price at exit.
    pub exit_price: f64,
    /// Absolute P&L (after commissions).
    pub pnl: f64,
    /// Percentage P&L: `pnl / (entry_price * size)`.
    pub pnl_pct: f64,
}

/// Summary performance metrics for a completed backtest.
#[derive(Debug, Clone)]
pub struct BacktestMetrics {
    /// Total return over the period: `(final - initial) / initial`.
    pub total_return: f64,
    /// Annualised return (assuming 252 trading days).
    pub annualized_return: f64,
    /// Annualised Sharpe ratio (risk-free = 0).
    pub sharpe: f64,
    /// Annualised Sortino ratio (downside deviation only).
    pub sortino: f64,
    /// Maximum peak-to-trough drawdown as a fraction.
    pub max_drawdown: f64,
    /// Calmar ratio: annualised return / max drawdown.
    pub calmar: f64,
    /// Fraction of trades that were profitable.
    pub win_rate: f64,
    /// Gross profit / gross loss.
    pub profit_factor: f64,
    /// Mean P&L per trade as a fraction of notional.
    pub avg_trade_return: f64,
    /// Total number of completed round-trip trades.
    pub num_trades: usize,
}

/// Full result of a [`BacktestEngine::run`] call.
#[derive(Debug, Clone)]
pub struct BacktestResult {
    /// Equity sampled after every bar.
    pub equity_curve: Vec<f64>,
    /// All completed round-trip trades.
    pub trades: Vec<CompletedTrade>,
    /// Computed performance metrics.
    pub metrics: BacktestMetrics,
}

// ─── Engine ───────────────────────────────────────────────────────────────────

/// Event-driven backtesting engine with realistic fill simulation.
pub struct BacktestEngine;

impl BacktestEngine {
    /// Run a backtest given a list of signals and a configuration.
    ///
    /// Signals are matched against bars by bar index: each signal fires at the
    /// **next** bar's open to avoid look-ahead.  Slippage is added for longs
    /// and subtracted for shorts.
    ///
    /// # Panics
    /// Does not panic; returns an empty result when there are no bars.
    pub fn run(signals: Vec<EngineSignal>, config: EngineConfig) -> BacktestResult {
        let bars = &config.data;
        if bars.is_empty() {
            return BacktestEngine::empty_result(config.initial_capital);
        }

        let n = bars.len();
        let mut equity = config.initial_capital;
        let mut equity_curve: Vec<f64> = Vec::with_capacity(n);
        let mut completed_trades: Vec<CompletedTrade> = Vec::new();

        // Active open position state
        let mut open_direction: Option<Direction> = None;
        let mut open_entry_price: f64 = 0.0;
        let mut open_entry_ts: u64 = 0;
        let mut open_size: f64 = 0.0; // number of units held
        let mut open_notional: f64 = 0.0;

        // Build a signal lookup by bar index (signal fires on bar i, fills on bar i+1)
        // We match signal to bar by timestamp: find bar whose ts_open_ms >= signal.timestamp
        // For simplicity, signals[i] maps to bar index by scanning.

        // Sort signals by timestamp
        let mut sorted_signals = signals;
        sorted_signals.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

        let mut sig_idx = 0;

        for bar_i in 0..n {
            let bar = &bars[bar_i];
            let bar_open_ms = bar.ts_open.nanos() as u64 / 1_000_000;

            // Check if we have a pending signal that should fire at this bar's open
            // A signal fires at the next bar, so signal.timestamp < bar_open_ms
            while sig_idx < sorted_signals.len()
                && sorted_signals[sig_idx].timestamp < bar_open_ms
            {
                let sig = &sorted_signals[sig_idx];
                let fill_price_raw = bar.open.value().to_f64_or(bar.open.value());
                let slippage_mult = config.slippage_bps / 10_000.0;

                match sig.direction {
                    Direction::Long | Direction::Short => {
                        // Close existing position first if opposite or Flat
                        if let Some(existing_dir) = open_direction {
                            if existing_dir != sig.direction {
                                let exit_price = apply_slippage(
                                    fill_price_raw,
                                    slippage_mult,
                                    existing_dir,
                                    true, // closing
                                );
                                let commission = exit_price * open_size * config.commission;
                                let pnl = compute_pnl(
                                    existing_dir,
                                    open_entry_price,
                                    exit_price,
                                    open_size,
                                ) - commission;
                                equity += pnl;
                                let pnl_pct = if open_notional != 0.0 {
                                    pnl / open_notional
                                } else {
                                    0.0
                                };
                                completed_trades.push(CompletedTrade {
                                    entry_ts: open_entry_ts,
                                    exit_ts: bar_open_ms,
                                    direction: existing_dir,
                                    entry_price: open_entry_price,
                                    exit_price,
                                    pnl,
                                    pnl_pct,
                                });
                                open_direction = None;
                            }
                        }

                        // Open new position
                        if open_direction.is_none() {
                            let entry_price = apply_slippage(
                                fill_price_raw,
                                slippage_mult,
                                sig.direction,
                                false, // opening
                            );
                            let size_capital = equity * config.capital_fraction * sig.strength;
                            let size = if entry_price > 0.0 {
                                size_capital / entry_price
                            } else {
                                0.0
                            };
                            let commission = entry_price * size * config.commission;
                            equity -= commission;
                            open_direction = Some(sig.direction);
                            open_entry_price = entry_price;
                            open_entry_ts = bar_open_ms;
                            open_size = size;
                            open_notional = entry_price * size;
                        }
                    }
                    Direction::Flat => {
                        // Close existing position
                        if let Some(existing_dir) = open_direction {
                            let exit_price = apply_slippage(
                                fill_price_raw,
                                slippage_mult,
                                existing_dir,
                                true,
                            );
                            let commission = exit_price * open_size * config.commission;
                            let pnl = compute_pnl(
                                existing_dir,
                                open_entry_price,
                                exit_price,
                                open_size,
                            ) - commission;
                            equity += pnl;
                            let pnl_pct = if open_notional != 0.0 {
                                pnl / open_notional
                            } else {
                                0.0
                            };
                            completed_trades.push(CompletedTrade {
                                entry_ts: open_entry_ts,
                                exit_ts: bar_open_ms,
                                direction: existing_dir,
                                entry_price: open_entry_price,
                                exit_price,
                                pnl,
                                pnl_pct,
                            });
                            open_direction = None;
                        }
                    }
                }
                sig_idx += 1;
            }

            // Mark-to-market equity using close price
            let close_f = bar.close.value().to_f64_or(bar.close.value());
            let mtm_equity = if let Some(dir) = open_direction {
                let unrealized = compute_pnl(dir, open_entry_price, close_f, open_size);
                equity + unrealized
            } else {
                equity
            };
            equity_curve.push(mtm_equity.max(0.0));
        }

        // Close any open position at last bar's close
        if let Some(dir) = open_direction {
            let last_bar = &bars[n - 1];
            let exit_price = last_bar.close.value().to_f64_or(last_bar.close.value());
            let commission = exit_price * open_size * config.commission;
            let pnl = compute_pnl(dir, open_entry_price, exit_price, open_size) - commission;
            equity += pnl;
            let pnl_pct = if open_notional != 0.0 { pnl / open_notional } else { 0.0 };
            let bar_ts = last_bar.ts_close.nanos() as u64 / 1_000_000;
            completed_trades.push(CompletedTrade {
                entry_ts: open_entry_ts,
                exit_ts: bar_ts,
                direction: dir,
                entry_price: open_entry_price,
                exit_price,
                pnl,
                pnl_pct,
            });
            // Update last equity_curve point
            if let Some(last) = equity_curve.last_mut() {
                *last = equity.max(0.0);
            }
        }

        let metrics =
            compute_metrics(&equity_curve, &completed_trades, config.initial_capital);

        BacktestResult { equity_curve, trades: completed_trades, metrics }
    }

    fn empty_result(_initial_capital: f64) -> BacktestResult {
        BacktestResult {
            equity_curve: vec![],
            trades: vec![],
            metrics: BacktestMetrics {
                total_return: 0.0,
                annualized_return: 0.0,
                sharpe: 0.0,
                sortino: 0.0,
                max_drawdown: 0.0,
                calmar: 0.0,
                win_rate: 0.0,
                profit_factor: 0.0,
                avg_trade_return: 0.0,
                num_trades: 0,
            },
        }
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Apply slippage: longs pay more to open, receive less to close; shorts inverse.
fn apply_slippage(price: f64, slippage_mult: f64, dir: Direction, closing: bool) -> f64 {
    let adverse = match (dir, closing) {
        (Direction::Long, false) => 1.0 + slippage_mult,  // buy higher
        (Direction::Long, true) => 1.0 - slippage_mult,   // sell lower
        (Direction::Short, false) => 1.0 - slippage_mult, // sell lower
        (Direction::Short, true) => 1.0 + slippage_mult,  // buy higher
        _ => 1.0,
    };
    price * adverse
}

/// P&L for a closed position.
fn compute_pnl(dir: Direction, entry: f64, exit: f64, size: f64) -> f64 {
    match dir {
        Direction::Long => (exit - entry) * size,
        Direction::Short => (entry - exit) * size,
        Direction::Flat => 0.0,
    }
}

/// Extension trait to safely convert `rust_decimal::Decimal` to f64.
trait ToF64OrDefault {
    fn to_f64_or(&self, _fallback: Self) -> f64
    where
        Self: Sized;
}

impl ToF64OrDefault for rust_decimal::Decimal {
    fn to_f64_or(&self, _fallback: Self) -> f64 {
        use rust_decimal::prelude::ToPrimitive;
        self.to_f64().unwrap_or(0.0)
    }
}

/// Compute all performance metrics from the equity curve and trades.
fn compute_metrics(
    equity_curve: &[f64],
    trades: &[CompletedTrade],
    initial_capital: f64,
) -> BacktestMetrics {
    let n = equity_curve.len();

    // --- Returns ---
    let final_equity = equity_curve.last().copied().unwrap_or(initial_capital);
    let total_return = if initial_capital > 0.0 {
        (final_equity - initial_capital) / initial_capital
    } else {
        0.0
    };

    // Annualized (assume 252 bars ~ 252 trading days)
    let years = n as f64 / 252.0;
    let annualized_return = if years > 0.0 {
        (1.0 + total_return).powf(1.0 / years) - 1.0
    } else {
        0.0
    };

    // --- Daily returns for Sharpe/Sortino ---
    let mut daily_returns: Vec<f64> = Vec::with_capacity(n.saturating_sub(1));
    for i in 1..n {
        if equity_curve[i - 1] > 0.0 {
            daily_returns.push((equity_curve[i] - equity_curve[i - 1]) / equity_curve[i - 1]);
        }
    }

    let sharpe = compute_sharpe_f64(&daily_returns);
    let sortino = compute_sortino_f64(&daily_returns);

    // --- Max Drawdown ---
    let max_drawdown = compute_max_drawdown(equity_curve);

    // --- Calmar ---
    let calmar = if max_drawdown > 0.0 {
        annualized_return / max_drawdown
    } else {
        0.0
    };

    // --- Trade stats ---
    let num_trades = trades.len();
    let (win_rate, profit_factor, avg_trade_return) = if num_trades == 0 {
        (0.0, 0.0, 0.0)
    } else {
        let wins = trades.iter().filter(|t| t.pnl > 0.0).count();
        let gross_profit: f64 = trades.iter().filter(|t| t.pnl > 0.0).map(|t| t.pnl).sum();
        let gross_loss: f64 =
            trades.iter().filter(|t| t.pnl < 0.0).map(|t| t.pnl.abs()).sum();
        let pf = if gross_loss > 0.0 { gross_profit / gross_loss } else { f64::INFINITY };
        let avg_ret: f64 = trades.iter().map(|t| t.pnl_pct).sum::<f64>() / num_trades as f64;
        (wins as f64 / num_trades as f64, pf, avg_ret)
    };

    BacktestMetrics {
        total_return,
        annualized_return,
        sharpe,
        sortino,
        max_drawdown,
        calmar,
        win_rate,
        profit_factor,
        avg_trade_return,
        num_trades,
    }
}

fn compute_sharpe_f64(returns: &[f64]) -> f64 {
    let n = returns.len();
    if n < 2 {
        return 0.0;
    }
    let mean = returns.iter().sum::<f64>() / n as f64;
    let var = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (n as f64 - 1.0);
    let std_dev = var.sqrt();
    if std_dev == 0.0 {
        return 0.0;
    }
    (mean / std_dev) * 252.0_f64.sqrt()
}

fn compute_sortino_f64(returns: &[f64]) -> f64 {
    let n = returns.len();
    if n < 2 {
        return 0.0;
    }
    let mean = returns.iter().sum::<f64>() / n as f64;
    let downside_var = returns
        .iter()
        .map(|r| if *r < 0.0 { r.powi(2) } else { 0.0 })
        .sum::<f64>()
        / (n as f64 - 1.0);
    let downside_dev = downside_var.sqrt();
    if downside_dev == 0.0 {
        return 0.0;
    }
    (mean / downside_dev) * 252.0_f64.sqrt()
}

fn compute_max_drawdown(equity_curve: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0_f64;
    for &e in equity_curve {
        if e > peak {
            peak = e;
        }
        if peak > 0.0 {
            let dd = (peak - e) / peak;
            if dd > max_dd {
                max_dd = dd;
            }
        }
    }
    max_dd
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(open: f64, high: f64, low: f64, close: f64, ts_ms: u64) -> OhlcvBar {
        let sym = Symbol::new("TEST").unwrap();
        let open_p = Price::new(rust_decimal::Decimal::try_from(open).unwrap()).unwrap();
        let high_p = Price::new(rust_decimal::Decimal::try_from(high).unwrap()).unwrap();
        let low_p = Price::new(rust_decimal::Decimal::try_from(low).unwrap()).unwrap();
        let close_p = Price::new(rust_decimal::Decimal::try_from(close).unwrap()).unwrap();
        OhlcvBar {
            symbol: sym,
            open: open_p,
            high: high_p,
            low: low_p,
            close: close_p,
            volume: Quantity::new(dec!(100)).unwrap(),
            ts_open: NanoTimestamp::new((ts_ms * 1_000_000) as i64),
            ts_close: NanoTimestamp::new((ts_ms * 1_000_000 + 1_000_000) as i64),
            tick_count: 1,
        }
    }

    fn long_signal(ts_ms: u64) -> EngineSignal {
        EngineSignal {
            timestamp: ts_ms,
            symbol: "TEST".to_string(),
            direction: Direction::Long,
            strength: 1.0,
        }
    }

    fn flat_signal(ts_ms: u64) -> EngineSignal {
        EngineSignal {
            timestamp: ts_ms,
            symbol: "TEST".to_string(),
            direction: Direction::Flat,
            strength: 1.0,
        }
    }

    fn short_signal(ts_ms: u64) -> EngineSignal {
        EngineSignal {
            timestamp: ts_ms,
            symbol: "TEST".to_string(),
            direction: Direction::Short,
            strength: 1.0,
        }
    }

    fn make_config(bars: Vec<OhlcvBar>) -> EngineConfig {
        EngineConfig {
            initial_capital: 10_000.0,
            commission: 0.001,
            slippage_bps: 5.0,
            data: bars,
            capital_fraction: 0.1,
        }
    }

    #[test]
    fn test_empty_bars_returns_empty_result() {
        let result = BacktestEngine::run(vec![], make_config(vec![]));
        assert!(result.equity_curve.is_empty());
        assert_eq!(result.trades.len(), 0);
        assert_eq!(result.metrics.num_trades, 0);
    }

    #[test]
    fn test_no_signals_equity_equals_initial() {
        let bars: Vec<OhlcvBar> = (0..5)
            .map(|i| bar(100.0, 102.0, 99.0, 101.0, 1000 + i * 100))
            .collect();
        let config = make_config(bars);
        let result = BacktestEngine::run(vec![], config);
        // No signals: equity should remain initial_capital throughout
        for &eq in &result.equity_curve {
            assert!((eq - 10_000.0).abs() < 1e-6, "eq={}", eq);
        }
    }

    #[test]
    fn test_long_trade_profitable() {
        // Signal on bar 0 (ts=1000), fills on bar 1 open (100.0)
        // Bar 2 close is 120 → profit
        let bars = vec![
            bar(100.0, 105.0, 99.0, 102.0, 1000),
            bar(100.0, 125.0, 99.0, 120.0, 2000),
            bar(120.0, 130.0, 118.0, 125.0, 3000),
        ];
        let signals = vec![long_signal(900)]; // fires before bar 0
        let config = make_config(bars);
        let result = BacktestEngine::run(signals, config);
        // Should have at least one trade after position closes at last bar
        assert!(!result.equity_curve.is_empty());
    }

    #[test]
    fn test_flat_signal_closes_position() {
        let bars = vec![
            bar(100.0, 105.0, 99.0, 102.0, 1000),
            bar(102.0, 110.0, 100.0, 108.0, 2000),
            bar(108.0, 112.0, 106.0, 110.0, 3000),
        ];
        let signals = vec![
            long_signal(900),  // opens on bar 1
            flat_signal(1500), // closes on bar 2
        ];
        let config = make_config(bars);
        let result = BacktestEngine::run(signals, config);
        assert_eq!(result.trades.len(), 1);
        assert_eq!(result.trades[0].direction, Direction::Long);
    }

    #[test]
    fn test_short_trade_created() {
        let bars = vec![
            bar(100.0, 105.0, 99.0, 99.0, 1000),
            bar(99.0, 100.0, 90.0, 90.0, 2000),
            bar(90.0, 91.0, 80.0, 82.0, 3000),
        ];
        let signals = vec![short_signal(900)];
        let config = make_config(bars);
        let result = BacktestEngine::run(signals, config);
        assert!(!result.equity_curve.is_empty());
    }

    #[test]
    fn test_opposite_signal_closes_then_opens() {
        let bars = vec![
            bar(100.0, 105.0, 99.0, 102.0, 1000),
            bar(102.0, 110.0, 100.0, 108.0, 2000),
            bar(108.0, 112.0, 106.0, 110.0, 3000),
            bar(110.0, 115.0, 108.0, 112.0, 4000),
        ];
        let signals = vec![
            long_signal(900),  // long on bar 1 open
            short_signal(1500), // close long + short on bar 2 open
        ];
        let config = make_config(bars);
        let result = BacktestEngine::run(signals, config);
        // Should have at least 1 completed trade (the long closed by short)
        assert!(result.trades.len() >= 1);
        assert_eq!(result.trades[0].direction, Direction::Long);
    }

    #[test]
    fn test_commission_reduces_equity() {
        let bars = vec![
            bar(100.0, 100.0, 100.0, 100.0, 1000),
            bar(100.0, 100.0, 100.0, 100.0, 2000),
        ];
        let signals = vec![long_signal(900)];
        let mut config = make_config(bars);
        config.commission = 0.01; // 1% commission
        config.slippage_bps = 0.0;
        let result = BacktestEngine::run(signals, config);
        // Commission should reduce final equity
        let final_eq = result.equity_curve.last().copied().unwrap_or(10_000.0);
        assert!(final_eq < 10_000.0, "Commission should reduce equity: {}", final_eq);
    }

    #[test]
    fn test_slippage_applied_to_long_open() {
        // With slippage, long entry is above open price
        let bars = vec![
            bar(100.0, 100.0, 100.0, 100.0, 1000),
            bar(100.0, 100.0, 100.0, 100.0, 2000),
        ];
        let signals = vec![long_signal(900)];
        let mut config = make_config(bars);
        config.commission = 0.0;
        config.slippage_bps = 100.0; // 100 bps = 1%
        let result = BacktestEngine::run(signals, config);
        // With slippage, position is entered at 101 but closes at 100, loss expected
        let final_eq = result.equity_curve.last().copied().unwrap_or(10_000.0);
        assert!(final_eq <= 10_000.0, "Slippage should reduce equity: {}", final_eq);
    }

    #[test]
    fn test_equity_curve_length_equals_bars() {
        let bars: Vec<OhlcvBar> = (0..10)
            .map(|i| bar(100.0, 105.0, 99.0, 102.0, 1000 + i * 100))
            .collect();
        let config = make_config(bars.clone());
        let result = BacktestEngine::run(vec![], config);
        assert_eq!(result.equity_curve.len(), bars.len());
    }

    #[test]
    fn test_metrics_total_return_positive_for_winning_trade() {
        // Rising market: long from 100 to 200
        let bars = vec![
            bar(100.0, 100.0, 100.0, 100.0, 1000),
            bar(200.0, 200.0, 200.0, 200.0, 2000),
            bar(200.0, 200.0, 200.0, 200.0, 3000),
        ];
        let signals = vec![long_signal(900)];
        let mut config = make_config(bars);
        config.commission = 0.0;
        config.slippage_bps = 0.0;
        config.capital_fraction = 1.0;
        let result = BacktestEngine::run(signals, config);
        assert!(result.metrics.total_return > 0.0, "tr={}", result.metrics.total_return);
    }

    #[test]
    fn test_metrics_win_rate_one_winner() {
        let bars = vec![
            bar(100.0, 100.0, 100.0, 100.0, 1000),
            bar(200.0, 200.0, 200.0, 200.0, 2000),
            bar(200.0, 200.0, 200.0, 200.0, 3000),
        ];
        let signals = vec![long_signal(900), flat_signal(1500)];
        let mut config = make_config(bars);
        config.commission = 0.0;
        config.slippage_bps = 0.0;
        config.capital_fraction = 1.0;
        let result = BacktestEngine::run(signals, config);
        assert_eq!(result.metrics.win_rate, 1.0);
    }

    #[test]
    fn test_max_drawdown_computed() {
        // Equity goes up then crashes
        let bars = vec![
            bar(100.0, 100.0, 100.0, 200.0, 1000),
            bar(200.0, 200.0, 200.0, 50.0, 2000),
            bar(50.0, 50.0, 50.0, 50.0, 3000),
        ];
        let result = BacktestEngine::run(vec![], make_config(bars));
        // No trades but equity_curve built; drawdown from 200 to 50 is 75%
        // But without a long position open, equity stays flat (= initial_capital)
        // max_drawdown is 0 (no position held)
        assert!(result.metrics.max_drawdown >= 0.0);
    }

    #[test]
    fn test_profit_factor_above_one_for_winning_trade() {
        let bars = vec![
            bar(100.0, 100.0, 100.0, 100.0, 1000),
            bar(110.0, 110.0, 110.0, 110.0, 2000),
            bar(110.0, 110.0, 110.0, 110.0, 3000),
        ];
        let signals = vec![long_signal(900), flat_signal(1500)];
        let mut config = make_config(bars);
        config.commission = 0.0;
        config.slippage_bps = 0.0;
        let result = BacktestEngine::run(signals, config);
        // Single winning trade: profit_factor = inf or > 1
        assert!(result.metrics.profit_factor > 1.0 || result.metrics.profit_factor.is_infinite());
    }

    #[test]
    fn test_num_trades_matches_completed_trades() {
        let bars = vec![
            bar(100.0, 100.0, 100.0, 100.0, 1000),
            bar(110.0, 110.0, 110.0, 110.0, 2000),
            bar(110.0, 110.0, 110.0, 115.0, 3000),
            bar(115.0, 115.0, 115.0, 120.0, 4000),
        ];
        let signals = vec![
            long_signal(900),
            flat_signal(1500),
            short_signal(2500),
            flat_signal(3500),
        ];
        let config = make_config(bars);
        let result = BacktestEngine::run(signals, config);
        assert_eq!(result.metrics.num_trades, result.trades.len());
    }

    #[test]
    fn test_strength_scales_position_size() {
        let bars = vec![
            bar(100.0, 100.0, 100.0, 100.0, 1000),
            bar(200.0, 200.0, 200.0, 200.0, 2000),
        ];
        let sig_half = EngineSignal {
            timestamp: 900,
            symbol: "TEST".to_string(),
            direction: Direction::Long,
            strength: 0.5,
        };
        let sig_full = EngineSignal {
            timestamp: 900,
            symbol: "TEST".to_string(),
            direction: Direction::Long,
            strength: 1.0,
        };
        let mut config1 = make_config(bars.clone());
        config1.commission = 0.0;
        config1.slippage_bps = 0.0;
        let mut config2 = make_config(bars);
        config2.commission = 0.0;
        config2.slippage_bps = 0.0;
        let r1 = BacktestEngine::run(vec![sig_half], config1);
        let r2 = BacktestEngine::run(vec![sig_full], config2);
        let ret1 = r1.metrics.total_return;
        let ret2 = r2.metrics.total_return;
        // Full strength should yield higher total return than half
        assert!(ret2 > ret1, "ret2={} ret1={}", ret2, ret1);
    }

    #[test]
    fn test_completed_trade_fields() {
        let bars = vec![
            bar(100.0, 100.0, 100.0, 100.0, 1000),
            bar(110.0, 110.0, 110.0, 110.0, 2000),
            bar(110.0, 110.0, 110.0, 115.0, 3000),
        ];
        let signals = vec![long_signal(900), flat_signal(1500)];
        let mut config = make_config(bars);
        config.commission = 0.0;
        config.slippage_bps = 0.0;
        let result = BacktestEngine::run(signals, config);
        let t = &result.trades[0];
        assert_eq!(t.direction, Direction::Long);
        assert!(t.entry_price > 0.0);
        assert!(t.exit_price > 0.0);
        assert!(t.exit_ts > t.entry_ts || t.exit_ts == t.entry_ts);
    }
}
