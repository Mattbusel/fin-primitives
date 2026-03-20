//! Built-in technical indicators implementing the [`crate::signals::Signal`] trait.
//!
//! All indicators return [`crate::signals::SignalValue::Unavailable`] until they have
//! accumulated enough bars to produce a meaningful value.

pub mod apo;
pub mod atr;
pub mod bollinger;
pub mod cci;
pub mod chandelier;
pub mod dema;
pub mod ema;
pub mod hullma;
pub mod macd;
pub mod momentum;
pub mod roc;
pub mod rsi;
pub mod sma;
pub mod stddev;
pub mod stochastic;
pub mod tema;
pub mod williams_r;
pub mod wma;

pub use apo::Apo;
pub use atr::Atr;
pub use bollinger::BollingerB;
pub use cci::Cci;
pub use chandelier::ChandelierExit;
pub use dema::Dema;
pub use ema::Ema;
pub use hullma::HullMa;
pub use macd::Macd;
pub use momentum::Momentum;
pub use roc::Roc;
pub use rsi::Rsi;
pub use sma::Sma;
pub use stddev::StdDev;
pub use stochastic::StochasticK;
pub use tema::Tema;
pub use williams_r::WilliamsR;
pub use wma::Wma;
