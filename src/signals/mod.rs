//! # Module: signals
//!
//! ## Responsibility
//! Provides the `Signal` trait, `SignalValue` enum, and a `SignalPipeline` that
//! applies multiple signals to each OHLCV bar in sequence.
//!
//! ## Guarantees
//! - `SignalValue::Unavailable` is returned until a signal has accumulated `period` bars
//! - `SignalPipeline::update` always returns `Ok(SignalMap)` even when individual signals are not ready
//!
//! ## NOT Responsible For
//! - Persistence
//! - Real-time streaming (use `OhlcvAggregator` upstream)

pub mod indicators;
pub mod pipeline;

use crate::error::FinError;
use crate::ohlcv::OhlcvBar;
use rust_decimal::Decimal;

/// The output value of a signal computation.
#[derive(Debug, Clone)]
pub enum SignalValue {
    /// A computed scalar value.
    Scalar(Decimal),
    /// The signal does not yet have enough data to produce a value.
    Unavailable,
}

/// A stateful indicator that updates on each new OHLCV bar.
///
/// # Implementors
/// - [`indicators::Sma`]: simple moving average
/// - [`indicators::Ema`]: exponential moving average
/// - [`indicators::Rsi`]: relative strength index
pub trait Signal: Send {
    /// Returns the name of this signal (unique within a pipeline).
    fn name(&self) -> &str;

    /// Updates the signal with a new bar and returns the current value.
    ///
    /// # Returns
    /// - `Ok(SignalValue::Scalar(v))` if enough bars have been accumulated
    /// - `Ok(SignalValue::Unavailable)` if fewer than `period` bars have been seen
    ///
    /// # Errors
    /// Returns [`FinError`] on arithmetic failure.
    fn update(&mut self, bar: &OhlcvBar) -> Result<SignalValue, FinError>;

    /// Returns `true` if the signal has accumulated enough bars to produce a value.
    fn is_ready(&self) -> bool;

    /// Returns the number of bars required before the signal produces a value.
    fn period(&self) -> usize;
}
