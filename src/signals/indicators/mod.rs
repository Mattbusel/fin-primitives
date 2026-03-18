//! Built-in technical indicators implementing the [`Signal`] trait.
//!
//! All indicators return [`crate::signals::SignalValue::Unavailable`] until they have
//! accumulated enough bars to produce a meaningful value.

pub mod ema;
pub mod rsi;
pub mod sma;

pub use ema::Ema;
pub use rsi::Rsi;
pub use sma::Sma;
