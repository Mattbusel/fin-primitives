//! # fin-primitives
//!
//! Validated, decimal-precise building blocks for trading and quantitative
//! systems: price and quantity types that cannot hold invalid values, a
//! sequence-checked level-2 order book, tick-to-OHLCV aggregation, streaming
//! indicators with an explicit warm-up contract, a position ledger, risk rules,
//! Black-Scholes Greeks and a walk-forward backtester. One error type,
//! [`FinError`], covers all of it.
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
//! Ticks become bars, and bars feed indicators. Indicators return
//! [`SignalValue::Unavailable`](signals::SignalValue::Unavailable) until they
//! have enough history, never a NaN or a zero:
//!
//! ```
//! use fin_primitives::ohlcv::{OhlcvAggregator, Timeframe};
//! use fin_primitives::signals::indicators::Sma;
//! use fin_primitives::signals::{BarInput, Signal, SignalValue};
//! use fin_primitives::tick::Tick;
//! use fin_primitives::types::{NanoTimestamp, Price, Quantity, Side, Symbol};
//! use rust_decimal_macros::dec;
//!
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
//! | [`async_signals`] | Tokio-based `StreamingSignalPipeline` |
//! | [`regime`] | Hurst exponent, GARCH(1,1), correlation-breakdown regime detection |
//!
//! Further modules cover portfolio optimization, factor models, yield curves,
//! fixed income, credit, derivatives, execution cost, microstructure, Monte
//! Carlo, pairs trading, tax lots and more; see the module list below.
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
//! Sister crate: [fin-stream](https://github.com/Mattbusel/fin-stream) handles
//! real-time market data ingestion on top of these types.

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
pub mod pnl;
pub mod correlation;
pub mod latency;
pub mod scenario;
pub mod microstructure;
pub mod ml;
pub mod regime;
pub mod cross_asset;
pub mod attribution;
pub mod options;
pub mod volatility;
pub mod impact;
pub mod portfolio;

#[cfg(feature = "python")]
pub mod python;
pub mod factor;
pub mod execution;
pub mod montecarlo;
pub mod yield_curve;
pub mod events;
pub mod crypto;
pub mod derivatives;
pub mod technical;
pub mod fixed_income;
pub mod liquidity;
pub mod pairs_trading;
pub mod ml_features;
pub mod performance;
pub mod clustering;
pub mod tax;
pub mod rebalancing;
pub mod funding;
pub mod alternative_data;
pub mod arbitrage;
pub mod execution_cost;
pub mod credit;

pub use error::FinError;
