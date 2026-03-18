//! Built-in technical indicators implementing the [`Signal`] trait.
//!
//! All indicators return [`crate::signals::SignalValue::Unavailable`] until they have
//! accumulated enough bars to produce a meaningful value.

pub mod sma;
pub mod ema;
pub mod rsi;

pub use sma::Sma;
pub use ema::Ema;
pub use rsi::Rsi;
