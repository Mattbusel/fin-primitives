//! # fin-primitives
//!
//! Financial data primitives: validated types, order book, OHLCV aggregation,
//! technical indicators, position tracking, and risk monitoring.
//!
//! All prices and quantities use [`rust_decimal::Decimal`] — never `f64`.

pub mod error;
pub mod types;
pub mod tick;
pub mod orderbook;
pub mod ohlcv;
pub mod signals;
pub mod position;
pub mod risk;

pub use error::FinError;
