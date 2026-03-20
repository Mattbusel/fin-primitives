//! Built-in technical indicators implementing the [`crate::signals::Signal`] trait.
//!
//! All indicators return [`crate::signals::SignalValue::Unavailable`] until they have
//! accumulated enough bars to produce a meaningful value.

pub mod atr;
pub mod bollinger;
pub mod cci;
pub mod ema;
pub mod macd;
pub mod rsi;
pub mod sma;
pub mod stochastic;
pub mod wma;

pub use atr::Atr;
pub use bollinger::BollingerB;
pub use cci::Cci;
pub use ema::Ema;
pub use macd::Macd;
pub use rsi::Rsi;
pub use sma::Sma;
pub use stochastic::StochasticK;
pub use wma::Wma;
