//! Built-in technical indicators implementing the [`crate::signals::Signal`] trait.
//!
//! All indicators return [`crate::signals::SignalValue::Unavailable`] until they have
//! accumulated enough bars to produce a meaningful value.

pub mod apo;
pub mod atr;
pub mod obv;
pub mod ppo;
pub mod bollinger;
pub mod cci;
pub mod chandelier;
pub mod dema;
pub mod donchian;
pub mod ema;
pub mod keltner;
pub mod hullma;
pub mod macd;
pub mod momentum;
pub mod roc;
pub mod rsi;
pub mod sma;
pub mod stddev;
pub mod stochastic;
pub mod stochastic_d;
pub mod tema;
pub mod vwap;
pub mod williams_r;
pub mod wma;

pub use apo::Apo;
pub use atr::Atr;
pub use obv::Obv;
pub use ppo::Ppo;
pub use bollinger::BollingerB;
pub use cci::Cci;
pub use chandelier::ChandelierExit;
pub use dema::Dema;
pub use donchian::DonchianMidpoint;
pub use ema::Ema;
pub use keltner::KeltnerChannel;
pub use hullma::HullMa;
pub use macd::Macd;
pub use momentum::Momentum;
pub use roc::Roc;
pub use rsi::Rsi;
pub use sma::Sma;
pub use stddev::StdDev;
pub use stochastic::StochasticK;
pub use stochastic_d::StochasticD;
pub use tema::Tema;
pub use vwap::Vwap;
pub use williams_r::WilliamsR;
pub use wma::Wma;
