//! # Module: signals
//!
//! ## Responsibility
//! Provides the `Signal` trait, `SignalValue` enum, `BarInput` thin input type, and a
//! `SignalPipeline` that applies multiple signals to each OHLCV bar in sequence.
//!
//! ## Guarantees
//! - `SignalValue::Unavailable` is returned until a signal has accumulated `period` bars
//! - `SignalPipeline::update` always returns a `SignalMap`; per-signal errors are collected
//!   rather than aborting the whole pipeline
//!
//! ## NOT Responsible For
//! - Persistence
//! - Real-time streaming (use `OhlcvAggregator` upstream)

pub mod indicators;
pub mod pipeline;

use crate::error::FinError;
use crate::ohlcv::OhlcvBar;
use rust_decimal::Decimal;

/// Thin input type for signal computation, decoupled from `OhlcvBar`.
///
/// Carrying all four price fields and volume allows future indicators (e.g. MACD on
/// high-low, OBV on volume) without forcing a dependency on `OhlcvBar`.
#[derive(Debug, Clone, Copy)]
pub struct BarInput {
    /// Closing price (used by most indicators).
    pub close: Decimal,
    /// High price of the bar.
    pub high: Decimal,
    /// Low price of the bar.
    pub low: Decimal,
    /// Opening price of the bar.
    pub open: Decimal,
    /// Total traded volume during the bar.
    pub volume: Decimal,
}

impl BarInput {
    /// Constructs a `BarInput` with all fields explicitly specified.
    pub fn new(close: Decimal, high: Decimal, low: Decimal, open: Decimal, volume: Decimal) -> Self {
        Self { close, high, low, open, volume }
    }

    /// Constructs a `BarInput` from a single close price, setting all OHLC fields to `close`
    /// and volume to zero. Useful in tests and for close-only indicators (SMA/EMA/RSI).
    pub fn from_close(close: Decimal) -> Self {
        Self { close, high: close, low: close, open: close, volume: Decimal::ZERO }
    }

    /// Returns the typical price of this bar: `(high + low + close) / 3`.
    pub fn typical_price(&self) -> Decimal {
        (self.high + self.low + self.close) / Decimal::from(3u32)
    }
}

impl From<&OhlcvBar> for BarInput {
    fn from(bar: &OhlcvBar) -> Self {
        Self {
            close: bar.close.value(),
            high: bar.high.value(),
            low: bar.low.value(),
            open: bar.open.value(),
            volume: bar.volume.value(),
        }
    }
}

/// The output value of a signal computation.
#[derive(Debug, Clone, PartialEq)]
pub enum SignalValue {
    /// A computed scalar value.
    Scalar(Decimal),
    /// The signal does not yet have enough data to produce a value.
    Unavailable,
}

impl SignalValue {
    /// Returns the inner `Decimal` if this is `Scalar`, or `None` if `Unavailable`.
    ///
    /// Eliminates `match` boilerplate at call sites.
    pub fn as_decimal(&self) -> Option<Decimal> {
        match self {
            SignalValue::Scalar(d) => Some(*d),
            SignalValue::Unavailable => None,
        }
    }

    /// Returns `true` if this value is `Scalar`.
    pub fn is_scalar(&self) -> bool {
        matches!(self, SignalValue::Scalar(_))
    }

    /// Returns `true` if this value is `Unavailable`.
    pub fn is_unavailable(&self) -> bool {
        matches!(self, SignalValue::Unavailable)
    }

    /// Returns the inner `Decimal` if `Scalar`, otherwise returns `default`.
    pub fn scalar_or(&self, default: Decimal) -> Decimal {
        match self {
            SignalValue::Scalar(d) => *d,
            SignalValue::Unavailable => default,
        }
    }

    /// Combine two `SignalValue`s with `f`, returning `Unavailable` if either is unavailable.
    ///
    /// Mirrors `Option::zip` combined with `map`. Useful for computing derived values
    /// that require two ready signals (e.g. a spread = signal_a - signal_b).
    ///
    /// # Example
    /// ```rust
    /// use fin_primitives::signals::SignalValue;
    /// use rust_decimal_macros::dec;
    ///
    /// let a = SignalValue::Scalar(dec!(10));
    /// let b = SignalValue::Scalar(dec!(3));
    /// let diff = a.zip_with(b, |x, y| x - y);
    /// assert_eq!(diff, SignalValue::Scalar(dec!(7)));
    /// ```
    pub fn zip_with(
        self,
        other: SignalValue,
        f: impl FnOnce(Decimal, Decimal) -> Decimal,
    ) -> SignalValue {
        match (self, other) {
            (SignalValue::Scalar(a), SignalValue::Scalar(b)) => SignalValue::Scalar(f(a, b)),
            _ => SignalValue::Unavailable,
        }
    }

    /// Apply `f` to the inner value if `Scalar`, returning a new `SignalValue`.
    ///
    /// If `Unavailable`, returns `Unavailable` without calling `f`. This mirrors
    /// `Option::map` and enables functional chaining without explicit `match`.
    ///
    /// # Example
    /// ```rust
    /// use fin_primitives::signals::SignalValue;
    /// use rust_decimal_macros::dec;
    ///
    /// let v = SignalValue::Scalar(dec!(100));
    /// let scaled = v.map(|x| x * dec!(2));
    /// assert_eq!(scaled, SignalValue::Scalar(dec!(200)));
    /// ```
    pub fn map(self, f: impl FnOnce(Decimal) -> Decimal) -> SignalValue {
        match self {
            SignalValue::Scalar(d) => SignalValue::Scalar(f(d)),
            SignalValue::Unavailable => SignalValue::Unavailable,
        }
    }

    /// Applies `f` to the inner value if `Scalar`, where `f` returns a `SignalValue`.
    ///
    /// If `Unavailable`, returns `Unavailable` without calling `f`. This mirrors
    /// `Option::and_then` and enables chaining operations that may themselves produce
    /// `Unavailable` (e.g., clamping, conditional transforms).
    ///
    /// # Example
    /// ```rust
    /// use fin_primitives::signals::SignalValue;
    /// use rust_decimal_macros::dec;
    ///
    /// let v = SignalValue::Scalar(dec!(50));
    /// // Only return a value if it's above 30.
    /// let r = v.and_then(|x| if x > dec!(30) { SignalValue::Scalar(x) } else { SignalValue::Unavailable });
    /// assert_eq!(r, SignalValue::Scalar(dec!(50)));
    /// ```
    pub fn and_then(self, f: impl FnOnce(Decimal) -> SignalValue) -> SignalValue {
        match self {
            SignalValue::Scalar(d) => f(d),
            SignalValue::Unavailable => SignalValue::Unavailable,
        }
    }
}

impl From<Decimal> for SignalValue {
    fn from(d: Decimal) -> Self {
        SignalValue::Scalar(d)
    }
}

impl std::fmt::Display for SignalValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SignalValue::Scalar(d) => write!(f, "{d}"),
            SignalValue::Unavailable => write!(f, "Unavailable"),
        }
    }
}

/// A stateful indicator that updates on each new bar input.
///
/// # Implementors
/// - [`indicators::Sma`]: simple moving average
/// - [`indicators::Ema`]: exponential moving average
/// - [`indicators::Rsi`]: relative strength index
pub trait Signal: Send {
    /// Returns the name of this signal (unique within a pipeline).
    fn name(&self) -> &str;

    /// Updates the signal with a [`BarInput`] and returns the current value.
    ///
    /// Accepting `BarInput` rather than `&OhlcvBar` lets signals be used on any
    /// price stream, not just OHLCV data.
    ///
    /// # Returns
    /// - `Ok(SignalValue::Scalar(v))` if enough bars have been accumulated
    /// - `Ok(SignalValue::Unavailable)` if fewer than `period` bars have been seen
    ///
    /// # Errors
    /// Returns [`FinError`] on arithmetic failure.
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError>;

    /// Convenience wrapper: converts `bar` to [`BarInput`] and calls [`Self::update`].
    fn update_bar(&mut self, bar: &OhlcvBar) -> Result<SignalValue, FinError> {
        self.update(&BarInput::from(bar))
    }

    /// Returns `true` if the signal has accumulated enough bars to produce a value.
    fn is_ready(&self) -> bool;

    /// Returns the number of bars required before the signal produces a value.
    fn period(&self) -> usize;

    /// Resets the signal to its initial state as if no bars had been seen.
    ///
    /// After calling `reset()`, `is_ready()` returns `false` and the next `period`
    /// bars will warm up the indicator again. Useful for walk-forward backtesting
    /// without creating a new indicator instance.
    fn reset(&mut self);

    /// Feed a slice of historical bars to prime the indicator in one call.
    ///
    /// Equivalent to calling [`update`](Self::update) for each bar in sequence.
    /// Returns the value after the final bar, or `Ok(SignalValue::Unavailable)`
    /// if `bars` is empty.
    ///
    /// # Errors
    /// Propagates the first [`FinError`] returned by [`update`](Self::update).
    fn warm_up(&mut self, bars: &[BarInput]) -> Result<SignalValue, FinError> {
        let mut last = SignalValue::Unavailable;
        for bar in bars {
            last = self.update(bar)?;
        }
        Ok(last)
    }
}
