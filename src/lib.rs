//! # fin-primitives
//!
//! A zero-panic, decimal-precise foundation for high-frequency trading and
//! quantitative systems in Rust.
//!
//! ## Features
//!
//! | Module | What it provides | Key guarantee |
//! |--------|-----------------|---------------|
//! | [`types`] | `Price`, `Quantity`, `Symbol`, `NanoTimestamp`, `Side` newtypes | Validation at construction; no invalid value can exist at runtime |
//! | [`tick`] | `Tick`, `TickFilter`, `TickReplayer` | Filter is pure; replayer yields ticks in ascending timestamp order |
//! | [`orderbook`] | L2 `OrderBook` with `apply_delta`, spread, mid-price, VWAP, top-N levels | Sequence validation; inverted spreads are detected and rolled back |
//! | [`ohlcv`] | `OhlcvBar`, `Timeframe`, `OhlcvAggregator`, `OhlcvSeries` | Bar invariants (`high >= low`, etc.) enforced on every push |
//! | [`signals`] | `Signal` trait, `SignalPipeline`, `Sma`, `Ema`, `Rsi` | Returns `Unavailable` until warm-up period is satisfied; no silent NaN |
//! | [`position`] | `Position`, `Fill`, `PositionLedger` | VWAP average cost; realized and unrealized P&L net of commissions |
//! | [`risk`] | `DrawdownTracker`, `RiskRule` trait, `MaxDrawdownRule`, `MinEquityRule`, `RiskMonitor` | All breaches returned as a typed `Vec<RiskBreach>`; never silently swallowed |
//! | [`greeks`] | `BlackScholes`, `OptionGreeks`, `OptionSpec`, `SpreadGreeks` | All math returns `Result<T, FinError>`; no panics on edge-case inputs |
//! | [`backtest`] | `Backtester`, `Strategy`, `BacktestResult`, `WalkForwardOptimizer`, `WfPeriod`, `ParamRange` | Bar-by-bar; no look-ahead; grid-search walk-forward with OOS stability score |
//! | [`async_signals`] | `StreamingSignalPipeline`, `SignalUpdate`, `spawn_signal_stream` | Tokio MPSC streaming; pre-allocated output buffers on the hot path |
//! | [`regime`] | `RegimeDetector`, `MarketRegime`, `Garch11`, `CorrelationBreakdownDetector`, `RegimeConditionalSignal`, `RegimeHistory` | Hurst + GARCH(1,1) + cross-asset correlation breakdown; regime-conditional RSI adaptation |
//!
//! ## Design Principles
//!
//! - **Zero panics.** Every fallible operation returns `Result<_, FinError>`.
//!   No `unwrap` or `expect` in production code paths.
//! - **Decimal precision.** All prices and quantities use [`rust_decimal::Decimal`].
//!   Floating-point drift is structurally impossible.
//! - **Nanosecond timestamps.** [`types::NanoTimestamp`] is a newtype over `i64`
//!   nanoseconds since Unix epoch, suitable for microsecond-accurate event ordering.
//! - **Composable by design.** [`risk::RiskRule`], [`signals::Signal`], and
//!   [`tick::TickFilter`] are traits; plug in your own implementations without forking.
//!
//! ## Errors
//!
//! All error variants live in [`FinError`] (re-exported at the crate root).
//! Every public fallible function documents which variant it may return.
//!
//! All prices and quantities use [`rust_decimal::Decimal`]; never `f64`.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod async_signals;
pub mod backtest;
pub mod error;
pub mod greeks;
pub mod ohlcv;
pub mod orderbook;
pub mod position;
pub mod risk;
pub mod signals;
pub mod tick;
pub mod types;

/// Streaming P&L attribution: decomposes realized P&L into alpha and cost components.
pub mod pnl;

/// Streaming Pearson correlation matrix for indicator redundancy detection.
pub mod correlation;

/// Order latency tracking: measures submit→ack, ack→fill, fill→book-update phases.
pub mod latency;

/// Risk scenario backtesting: replays historical bars through risk rules.
pub mod scenario;

/// Tick-level microstructure metrics: bid-ask spread, Amihud illiquidity, Kyle's lambda, Roll implied spread.
pub mod microstructure;

/// ML feature vector builder: snapshot N indicator outputs, normalize, and serialize for ML pipelines.
pub mod ml;

/// Market regime engine: Hurst exponent, GARCH(1,1), cross-asset correlation breakdown,
/// `RegimeConditionalSignal` (regime-adaptive RSI), and full `RegimeHistory` audit trail.
pub mod regime;

/// Cross-asset rolling correlations and PCA-based dimensionality reduction.
pub mod cross_asset;

/// Portfolio performance attribution: Brinson-Hood-Beebower decomposition, multi-factor
/// attribution, marginal risk contribution, and comprehensive performance tearsheet.
pub mod attribution;

/// Black-Scholes options pricing engine with Greeks and implied volatility solver.
pub mod options;

/// Realised volatility estimators: Close-to-Close, Parkinson, Garman-Klass, Rogers-Satchell, Yang-Zhang.
pub mod volatility;

/// Almgren-Chriss optimal order execution and market impact model.
pub mod impact;

/// PyO3 Python bindings (enabled by the `python` feature).
#[cfg(feature = "python")]
pub mod python;

pub use error::FinError;
