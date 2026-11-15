//! # fin-primitives
//!
//! The basic parts of trading software, done once and checked: exact price and
//! size types that refuse bad values, an order book, candles built from trades,
//! indicators, option prices and risk limits.
//!
//! ![Three bundled examples running in a terminal: an order book ladder, an option chain with Greeks, and a position ledger tripping a drawdown limit](https://gitlab.com/mattbusel/fin-primitives/-/raw/main/assets/demo.gif)
//!
//! Add it (the `dec!` macro for decimal literals needs both `rust_decimal` crates):
//!
//! ```text
//! cargo add fin-primitives rust_decimal rust_decimal_macros
//! ```
//!
//! The main types: [`Price`](types::Price), [`Quantity`](types::Quantity) and
//! [`Symbol`](types::Symbol) (validated values), [`OrderBook`](orderbook::OrderBook),
//! [`OhlcvAggregator`](ohlcv::OhlcvAggregator) (ticks to candles), the
//! [`Signal`](signals::Signal) trait and its indicators,
//! [`PositionLedger`](position::PositionLedger), [`RiskMonitor`](risk::RiskMonitor),
//! [`BlackScholes`](greeks::BlackScholes), and one error type, [`FinError`].
//!
//! ## A first look
//!
//! Build a book from sequenced deltas, read the top of book, and price a
//! market order by walking the levels:
//!
//! ```
//! use fin_primitives::orderbook::{BookDelta, DeltaAction, OrderBook};
//! use fin_primitives::types::{Price, Quantity, Side, Symbol};
//! use rust_decimal_macros::dec;
//!
//! # fn main() -> Result<(), fin_primitives::FinError> {
//! let mut book = OrderBook::new(Symbol::new("BTC-USD")?);
//! let levels = [
//!     (Side::Ask, dec!(64250.50), dec!(0.842)),
//!     (Side::Ask, dec!(64251.00), dec!(1.310)),
//!     (Side::Bid, dec!(64250.00), dec!(1.204)),
//!     (Side::Bid, dec!(64249.50), dec!(0.655)),
//! ];
//! for (seq, (side, price, qty)) in (1..).zip(levels) {
//!     book.apply_delta(BookDelta {
//!         side,
//!         price: Price::new(price)?,
//!         quantity: Quantity::new(qty)?,
//!         action: DeltaAction::Set,
//!         sequence: seq,
//!     })?;
//! }
//!
//! assert_eq!(book.spread(), Some(dec!(0.50)));
//! assert_eq!(book.mid_price(), Some(dec!(64250.25)));
//!
//! // Buying 1 BTC takes all 0.842 at the best ask and 0.158 at the next level.
//! let vwap = book.vwap_for_qty(Side::Ask, Quantity::new(dec!(1))?)?;
//! assert_eq!(vwap, dec!(64250.579));
//!
//! // A delta that would cross the book is rejected and rolled back.
//! let crossing = BookDelta {
//!     side: Side::Bid,
//!     price: Price::new(dec!(64251.00))?,
//!     quantity: Quantity::new(dec!(2))?,
//!     action: DeltaAction::Set,
//!     sequence: 5,
//! };
//! assert!(book.apply_delta(crossing).is_err());
//! assert_eq!(book.sequence(), 4);
//! # Ok(())
//! # }
//! ```
//!
//! ![Animated diagram: real ticks land in one-minute buckets, push_tick returns the finished bar when the next bucket starts, and RSI(14) returns Unavailable until bar 15](https://gitlab.com/mattbusel/fin-primitives/-/raw/main/docs/img/ticks-to-bars.svg)
//!
//! Ticks become bars, and bars feed indicators. Indicators return
//! [`SignalValue::Unavailable`](signals::SignalValue::Unavailable) until they
//! have enough history, never a NaN or a zero:
//!
//! ```
//! use fin_primitives::ohlcv::{OhlcvAggregator, Timeframe};
//! # #[cfg(feature = "indicators")]
//! use fin_primitives::signals::indicators::Sma;
//! use fin_primitives::signals::{BarInput, Signal, SignalValue};
//! use fin_primitives::tick::Tick;
//! use fin_primitives::types::{NanoTimestamp, Price, Quantity, Side, Symbol};
//! use rust_decimal_macros::dec;
//!
//! # #[cfg(feature = "indicators")]
//! # fn main() -> Result<(), fin_primitives::FinError> {
//! let sym = Symbol::new("ETH-USD")?;
//! let mut agg = OhlcvAggregator::new(sym.clone(), Timeframe::Minutes(1))?;
//! let mut sma = Sma::new("sma3", 3)?;
//!
//! let mut values = Vec::new();
//! for (minute, close) in [(0, dec!(3180)), (1, dec!(3190)), (2, dec!(3200)), (3, dec!(3230))] {
//!     let tick = Tick::new(
//!         sym.clone(),
//!         Price::new(close)?,
//!         Quantity::new(dec!(1))?,
//!         Side::Bid,
//!         NanoTimestamp::from_secs(1_767_625_200 + minute * 60),
//!     );
//!     for bar in agg.push_tick(&tick)? {
//!         values.push(sma.update(&BarInput::from(&bar))?);
//!     }
//! }
//! // Three bars have closed; the fourth is still open.
//! assert_eq!(values[0], SignalValue::Unavailable);
//! assert_eq!(values[1], SignalValue::Unavailable);
//! assert_eq!(values[2], SignalValue::Scalar(dec!(3190)));
//! # Ok(())
//! # }
//! # #[cfg(not(feature = "indicators"))]
//! # fn main() {}
//! ```
//!
//! ## Runnable examples
//!
//! The repository ships examples that print formatted, colored output:
//!
//! | Command | Shows |
//! |---------|-------|
//! | `cargo run --example order_book` | depth ladder, spread, micro-price, VWAP fill, rejected deltas |
//! | `cargo run --example candles` | ticks to 1-minute candles, EMA and RSI with warm-up |
//! | `cargo run --example position_risk` | fills, mark-to-market, drawdown and equity-floor breaches |
//! | `cargo run --example option_chain` | Black-Scholes chain with Greeks and an implied-vol round trip |
//!
//! ## Where things live
//!
//! | Module | What it provides |
//! |--------|------------------|
//! | [`types`] | [`Price`](types::Price), [`Quantity`](types::Quantity), [`Symbol`](types::Symbol), [`NanoTimestamp`](types::NanoTimestamp), [`Side`](types::Side) |
//! | [`tick`] | [`Tick`](tick::Tick), `TickFilter`, `TickReplayer` |
//! | [`orderbook`] | [`OrderBook`](orderbook::OrderBook): L2 book with sequence checks and crossed-book rollback |
//! | [`ohlcv`] | [`OhlcvBar`](ohlcv::OhlcvBar), [`OhlcvAggregator`](ohlcv::OhlcvAggregator), `OhlcvSeries` analytics |
//! | [`signals`] | the [`Signal`](signals::Signal) trait, `SignalPipeline`, and several hundred indicators in [`signals::indicators`] |
//! | [`position`] | [`Fill`](position::Fill), [`Position`](position::Position), [`PositionLedger`](position::PositionLedger), Kelly sizing |
//! | [`risk`] | `DrawdownTracker`, the [`RiskRule`](risk::RiskRule) trait, [`RiskMonitor`](risk::RiskMonitor), VaR and stress tools |
//! | [`greeks`] | [`BlackScholes`](greeks::BlackScholes) pricing, Greeks, implied volatility, multi-leg spreads |
//! | [`backtest`] | bar-by-bar `Backtester`, `Strategy` trait, `WalkForwardOptimizer` |
//! | `async_signals` | Tokio-based `StreamingSignalPipeline` (feature `async`) |
//! | [`regime`] | Hurst exponent, GARCH(1,1), correlation-breakdown regime detection |
//!
//! Further modules cover portfolio optimization, factor models, yield curves,
//! fixed income, credit, derivatives, execution cost, microstructure, Monte
//! Carlo, pairs trading, tax lots and more; see the module list below.
//!
//! ## Feature flags
//!
//! Since 3.0 every domain module sits behind its own feature. All of them are on by
//! default except `async`, so `cargo add fin-primitives` gets everything but no
//! async runtime. With `default-features = false` you get only the core: [`types`],
//! [`tick`], [`orderbook`], [`ohlcv`], the [`Signal`](signals::Signal) trait and its
//! pipeline and composition tools, [`normal`], [`interop`] and [`FinError`], built on
//! chrono, rust_decimal, thiserror and libm.
//!
//! ```toml
//! # Only the core types, order book and candles:
//! fin-primitives = { version = "3", default-features = false }
//! # Core plus the indicator library and serde:
//! fin-primitives = { version = "3", default-features = false, features = ["serde", "indicators"] }
//! # Everything, including the Tokio pipeline (3.0 moved `async` out of default):
//! fin-primitives = { version = "3", features = ["async"] }
//! ```
//!
//! | feature | default | adds |
//! |---|---|---|
//! | `serde` | yes | `Serialize`/`Deserialize` on the data types; deserializing re-runs validation |
//! | `indicators` | yes | `signals::indicators` (700+ indicators), `technical`, `regime`, `correlation`, `volatility` |
//! | `options` | yes | `greeks` (Black-Scholes, Greeks, implied vol, spreads) and `options` |
//! | `portfolio` | yes | `portfolio`, `position`, `pnl`, `attribution`, `factor`, `rebalancing`, `performance` |
//! | `risk` | yes | `risk`, `scenario`, `cross_asset`, `liquidity` (turns on `portfolio`) |
//! | `backtest` | yes | `backtest`, `execution`, `execution_cost`, `impact`, `latency`, `microstructure` (turns on `portfolio`) |
//! | `ml` | yes | `ml`, `ml_features`, `alternative_data`, `pairs_trading`, `events` |
//! | `crypto` | yes | `crypto`, `funding`, `arbitrage` |
//! | `fixed-income`, `yield-curve`, `derivatives`, `credit` | yes | `fixed_income`, `yield_curve`, `derivatives`, `credit` |
//! | `tax`, `clustering`, `montecarlo` | yes | `tax`, `clustering`, `montecarlo` |
//! | `async` | **no** (yes before 3.0) | `async_signals`: a Tokio task that runs a signal pipeline over a channel |
//! | `full` | no | every feature above, including `async` |
//! | `ta`, `yata`, `wickra` | no | [`interop`] with those crates: their indicators read fin-primitives bars, their bars convert to [`BarInput`](signals::BarInput) |
//! | `arrow` | no | `arrow`: bars to and from Apache Arrow with exact `Decimal128` columns |
//! | `python` | no | PyO3 bindings for the `fin-primitives` Python package (all indicators, exact `Decimal` or float output, numpy / pandas / polars batches), built with maturin from the repository's `python/` folder |
//!
//! The doc examples on this page use the default features.
//!
//! [`normal`] has the standard normal CDF, PDF and inverse used by every pricer and
//! VaR model here (accurate to about 1e-15 relative, including the tails).
//!
//! ## Design
//!
//! - **Validated at construction.** `Price::new` rejects zero and negative
//!   values; `Quantity::new` rejects negatives; `Symbol::new` rejects empty or
//!   whitespace strings. Code that holds one of these types can trust it.
//! - **Decimal where money is.** Prices, quantities, P&L and order-book math
//!   use [`rust_decimal::Decimal`]. Statistical models (GARCH, optimizers,
//!   Black-Scholes internals) compute in `f64` and convert at the boundary.
//! - **Typed errors.** Fallible operations return `Result<_, FinError>`, and
//!   the crate's Clippy config warns on `unwrap`, `expect` and `panic`.
//! - **Traits at the seams.** [`risk::RiskRule`], [`signals::Signal`] and
//!   [`tick::TickFilter`] are traits, so your own rules and indicators plug in
//!   next to the built-in ones.
//! - **No unsafe code.** The crate is `#![forbid(unsafe_code)]`.
//!
//! Sister crate: [fin-stream](https://gitlab.com/mattbusel/fin-stream) handles
//! real-time market data ingestion on top of these types.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]
// The crate and module docs link to feature-gated modules (`position`, `risk`,
// `greeks`, `signals::indicators`). Those links resolve in the default and docs.rs
// builds; with some of those features off they have no target, which is expected.
#![cfg_attr(
    not(all(feature = "indicators", feature = "options", feature = "portfolio", feature = "risk")),
    allow(rustdoc::broken_intra_doc_links)
)]

// Core: always compiled. These modules depend only on chrono, rust_decimal,
// thiserror and libm (plus serde when that feature is on).
pub mod error;
pub mod interop;
pub mod normal;
pub mod ohlcv;
pub mod orderbook;
pub mod signals;
pub mod tick;
pub mod types;

// Optional integrations.
#[cfg(feature = "async")]
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
pub mod async_signals;
#[cfg(feature = "arrow")]
#[cfg_attr(docsrs, doc(cfg(feature = "arrow")))]
pub mod arrow;
#[cfg(feature = "python")]
pub mod python;

// Domain modules, each behind a Cargo feature (all on by default).

// feature = "indicators"
#[cfg(feature = "indicators")]
#[cfg_attr(docsrs, doc(cfg(feature = "indicators")))]
pub mod correlation;
#[cfg(feature = "indicators")]
#[cfg_attr(docsrs, doc(cfg(feature = "indicators")))]
pub mod regime;
#[cfg(feature = "indicators")]
#[cfg_attr(docsrs, doc(cfg(feature = "indicators")))]
pub mod technical;
#[cfg(feature = "indicators")]
#[cfg_attr(docsrs, doc(cfg(feature = "indicators")))]
pub mod volatility;

// feature = "options"
#[cfg(feature = "options")]
#[cfg_attr(docsrs, doc(cfg(feature = "options")))]
pub mod greeks;
#[cfg(feature = "options")]
#[cfg_attr(docsrs, doc(cfg(feature = "options")))]
pub mod options;

// feature = "portfolio"
#[cfg(feature = "portfolio")]
#[cfg_attr(docsrs, doc(cfg(feature = "portfolio")))]
pub mod attribution;
#[cfg(feature = "portfolio")]
#[cfg_attr(docsrs, doc(cfg(feature = "portfolio")))]
pub mod factor;
#[cfg(feature = "portfolio")]
#[cfg_attr(docsrs, doc(cfg(feature = "portfolio")))]
pub mod performance;
#[cfg(feature = "portfolio")]
#[cfg_attr(docsrs, doc(cfg(feature = "portfolio")))]
pub mod pnl;
#[cfg(feature = "portfolio")]
#[cfg_attr(docsrs, doc(cfg(feature = "portfolio")))]
pub mod portfolio;
#[cfg(feature = "portfolio")]
#[cfg_attr(docsrs, doc(cfg(feature = "portfolio")))]
pub mod position;
#[cfg(feature = "portfolio")]
#[cfg_attr(docsrs, doc(cfg(feature = "portfolio")))]
pub mod rebalancing;

// feature = "risk"
#[cfg(feature = "risk")]
#[cfg_attr(docsrs, doc(cfg(feature = "risk")))]
pub mod cross_asset;
#[cfg(feature = "risk")]
#[cfg_attr(docsrs, doc(cfg(feature = "risk")))]
pub mod liquidity;
#[cfg(feature = "risk")]
#[cfg_attr(docsrs, doc(cfg(feature = "risk")))]
pub mod risk;
#[cfg(feature = "risk")]
#[cfg_attr(docsrs, doc(cfg(feature = "risk")))]
pub mod scenario;

// feature = "backtest"
#[cfg(feature = "backtest")]
#[cfg_attr(docsrs, doc(cfg(feature = "backtest")))]
pub mod backtest;
#[cfg(feature = "backtest")]
#[cfg_attr(docsrs, doc(cfg(feature = "backtest")))]
pub mod execution;
#[cfg(feature = "backtest")]
#[cfg_attr(docsrs, doc(cfg(feature = "backtest")))]
pub mod execution_cost;
#[cfg(feature = "backtest")]
#[cfg_attr(docsrs, doc(cfg(feature = "backtest")))]
pub mod impact;
#[cfg(feature = "backtest")]
#[cfg_attr(docsrs, doc(cfg(feature = "backtest")))]
pub mod latency;
#[cfg(feature = "backtest")]
#[cfg_attr(docsrs, doc(cfg(feature = "backtest")))]
pub mod microstructure;

// feature = "ml"
#[cfg(feature = "ml")]
#[cfg_attr(docsrs, doc(cfg(feature = "ml")))]
pub mod alternative_data;
#[cfg(feature = "ml")]
#[cfg_attr(docsrs, doc(cfg(feature = "ml")))]
pub mod events;
#[cfg(feature = "ml")]
#[cfg_attr(docsrs, doc(cfg(feature = "ml")))]
pub mod ml;
#[cfg(feature = "ml")]
#[cfg_attr(docsrs, doc(cfg(feature = "ml")))]
pub mod ml_features;
#[cfg(feature = "ml")]
#[cfg_attr(docsrs, doc(cfg(feature = "ml")))]
pub mod pairs_trading;

// feature = "crypto"
#[cfg(feature = "crypto")]
#[cfg_attr(docsrs, doc(cfg(feature = "crypto")))]
pub mod arbitrage;
#[cfg(feature = "crypto")]
#[cfg_attr(docsrs, doc(cfg(feature = "crypto")))]
pub mod crypto;
#[cfg(feature = "crypto")]
#[cfg_attr(docsrs, doc(cfg(feature = "crypto")))]
pub mod funding;

// feature = "fixed-income"
#[cfg(feature = "fixed-income")]
#[cfg_attr(docsrs, doc(cfg(feature = "fixed-income")))]
pub mod fixed_income;

// feature = "derivatives"
#[cfg(feature = "derivatives")]
#[cfg_attr(docsrs, doc(cfg(feature = "derivatives")))]
pub mod derivatives;

// feature = "credit"
#[cfg(feature = "credit")]
#[cfg_attr(docsrs, doc(cfg(feature = "credit")))]
pub mod credit;

// feature = "tax"
#[cfg(feature = "tax")]
#[cfg_attr(docsrs, doc(cfg(feature = "tax")))]
pub mod tax;

// feature = "yield-curve"
#[cfg(feature = "yield-curve")]
#[cfg_attr(docsrs, doc(cfg(feature = "yield-curve")))]
pub mod yield_curve;

// feature = "clustering"
#[cfg(feature = "clustering")]
#[cfg_attr(docsrs, doc(cfg(feature = "clustering")))]
pub mod clustering;

// feature = "montecarlo"
#[cfg(feature = "montecarlo")]
#[cfg_attr(docsrs, doc(cfg(feature = "montecarlo")))]
pub mod montecarlo;

pub use error::FinError;

/// Compiles and runs the Rust code blocks in README.md as doc tests, so the README
/// cannot drift from the API.
#[cfg(all(doctest, feature = "indicators"))]
#[doc = include_str!("../README.md")]
pub struct ReadmeDoctests;
