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

pub mod error;
pub mod types;
pub mod tick;
pub mod orderbook;
pub mod ohlcv;
pub mod signals;
pub mod position;
pub mod risk;

pub use error::FinError;
