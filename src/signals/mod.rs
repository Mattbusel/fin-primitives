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

    /// Returns the weighted close price: `(high + low + close + close) / 4`.
    ///
    /// Weights the close twice, giving it extra significance compared to the typical price.
    /// Used by some indicators (e.g. CCI variants) and charting systems as a price reference.
    pub fn weighted_close(&self) -> Decimal {
        (self.high + self.low + self.close + self.close) / Decimal::from(4u32)
    }

    /// Returns the midpoint of the bar: `(high + low) / 2`.
    pub fn midpoint(&self) -> Decimal {
        (self.high + self.low) / Decimal::from(2u32)
    }

    /// Returns the absolute body size: `|close - open|`.
    pub fn body_size(&self) -> Decimal {
        (self.close - self.open).abs()
    }

    /// Returns the upper wick length: `high - max(open, close)`.
    pub fn upper_wick(&self) -> Decimal {
        self.high - self.open.max(self.close)
    }

    /// Returns the lower wick length: `min(open, close) - low`.
    pub fn lower_wick(&self) -> Decimal {
        self.open.min(self.close) - self.low
    }

    /// Returns `true` if the bar closed higher than it opened (bullish candle).
    pub fn is_bullish(&self) -> bool {
        self.close > self.open
    }

    /// Returns `true` if the bar closed lower than it opened (bearish candle).
    pub fn is_bearish(&self) -> bool {
        self.close < self.open
    }

    /// Returns the close-to-close price change: `close - prev_close`.
    ///
    /// When `prev_close` is `None` (first bar), returns `Decimal::ZERO`.
    pub fn price_change(&self, prev_close: Option<Decimal>) -> Decimal {
        match prev_close {
            None => Decimal::ZERO,
            Some(pc) => self.close - pc,
        }
    }

    /// Returns the log return: `ln(close / prev_close)` via f64.
    ///
    /// Returns `None` when `prev_close` is `None`, zero, or negative, or when the
    /// f64 conversion fails.
    pub fn log_return(&self, prev_close: Option<Decimal>) -> Option<Decimal> {
        use rust_decimal::prelude::ToPrimitive;
        let pc = prev_close?;
        if pc <= Decimal::ZERO {
            return None;
        }
        let ratio = self.close.to_f64()? / pc.to_f64()?;
        if ratio <= 0.0 {
            return None;
        }
        Decimal::try_from(ratio.ln()).ok()
    }

    /// Returns the True Range of this bar given the previous bar's close.
    ///
    /// `TR = max(high - low, |high - prev_close|, |low - prev_close|)`
    ///
    /// When there is no previous close (first bar), `high - low` is used as the true range.
    pub fn true_range(&self, prev_close: Option<Decimal>) -> Decimal {
        let hl = self.high - self.low;
        match prev_close {
            None => hl,
            Some(pc) => {
                let hc = (self.high - pc).abs();
                let lc = (self.low - pc).abs();
                hl.max(hc).max(lc)
            }
        }
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

    /// Negates the scalar value: returns `Scalar(-x)` if `Scalar(x)`, else `Unavailable`.
    ///
    /// Useful for inverting oscillator signals (e.g. turning a sell signal into a buy signal
    /// by negating the output) without requiring an explicit `map(|x| -x)`.
    pub fn negate(self) -> SignalValue {
        match self {
            SignalValue::Scalar(d) => SignalValue::Scalar(-d),
            SignalValue::Unavailable => SignalValue::Unavailable,
        }
    }

    /// Adds `delta` to the scalar value.
    ///
    /// Returns [`SignalValue::Unavailable`] unchanged.
    pub fn offset(self, delta: rust_decimal::Decimal) -> SignalValue {
        match self {
            SignalValue::Unavailable => SignalValue::Unavailable,
            SignalValue::Scalar(v) => SignalValue::Scalar(v + delta),
        }
    }

    /// Returns the smaller of `self` and `other`.  `Unavailable` loses to any `Scalar`.
    pub fn min_with(self, other: SignalValue) -> SignalValue {
        match (self, other) {
            (SignalValue::Scalar(a), SignalValue::Scalar(b)) => SignalValue::Scalar(a.min(b)),
            (s @ SignalValue::Scalar(_), SignalValue::Unavailable) => s,
            (SignalValue::Unavailable, s @ SignalValue::Scalar(_)) => s,
            (SignalValue::Unavailable, SignalValue::Unavailable) => SignalValue::Unavailable,
        }
    }

    /// Returns the larger of `self` and `other`.  `Unavailable` loses to any `Scalar`.
    pub fn max_with(self, other: SignalValue) -> SignalValue {
        match (self, other) {
            (SignalValue::Scalar(a), SignalValue::Scalar(b)) => SignalValue::Scalar(a.max(b)),
            (s @ SignalValue::Scalar(_), SignalValue::Unavailable) => s,
            (SignalValue::Unavailable, s @ SignalValue::Scalar(_)) => s,
            (SignalValue::Unavailable, SignalValue::Unavailable) => SignalValue::Unavailable,
        }
    }

    /// Returns the absolute value of the scalar: `Scalar(|x|)` or `Unavailable`.
    ///
    /// Useful when you only care about the magnitude of a signal (e.g. absolute momentum).
    pub fn abs(self) -> SignalValue {
        match self {
            SignalValue::Scalar(d) => SignalValue::Scalar(d.abs()),
            SignalValue::Unavailable => SignalValue::Unavailable,
        }
    }

    /// Scales the scalar by `factor`: `Scalar(x) * factor = Scalar(x * factor)`.
    ///
    /// Returns `Unavailable` if the signal is `Unavailable`. Useful for weighting
    /// or inverting signals (e.g. `signal.mul(Decimal::NEGATIVE_ONE)`).
    pub fn mul(self, factor: Decimal) -> SignalValue {
        match self {
            SignalValue::Scalar(d) => SignalValue::Scalar(d * factor),
            SignalValue::Unavailable => SignalValue::Unavailable,
        }
    }

    /// Subtracts two signals: `Scalar(a) - Scalar(b) = Scalar(a - b)`.
    ///
    /// Returns `Unavailable` if either operand is `Unavailable`.
    pub fn sub(self, other: SignalValue) -> SignalValue {
        match (self, other) {
            (SignalValue::Scalar(a), SignalValue::Scalar(b)) => SignalValue::Scalar(a - b),
            _ => SignalValue::Unavailable,
        }
    }

    /// Multiplies two signals: `Scalar(a) * Scalar(b) = Scalar(a * b)`.
    ///
    /// Returns `Unavailable` if either operand is `Unavailable`.
    pub fn mul_signal(self, other: SignalValue) -> SignalValue {
        match (self, other) {
            (SignalValue::Scalar(a), SignalValue::Scalar(b)) => SignalValue::Scalar(a * b),
            _ => SignalValue::Unavailable,
        }
    }

    /// Adds two signals: `Scalar(a) + Scalar(b) = Scalar(a + b)`.
    ///
    /// Returns `Unavailable` if either operand is `Unavailable`.
    /// Useful for combining multiple signal outputs without explicit pattern matching.
    pub fn add(self, other: SignalValue) -> SignalValue {
        match (self, other) {
            (SignalValue::Scalar(a), SignalValue::Scalar(b)) => SignalValue::Scalar(a + b),
            _ => SignalValue::Unavailable,
        }
    }

    /// Clamps the scalar value to `[lo, hi]`, returning `Unavailable` if `Unavailable`.
    ///
    /// If `Scalar(v)`, returns `Scalar(v.clamp(lo, hi))`. Useful for bounding oscillators
    /// such as RSI to valid ranges after arithmetic transforms.
    ///
    /// # Example
    /// ```rust
    /// use fin_primitives::signals::SignalValue;
    /// use rust_decimal_macros::dec;
    ///
    /// let v = SignalValue::Scalar(dec!(105));
    /// assert_eq!(v.clamp(dec!(0), dec!(100)), SignalValue::Scalar(dec!(100)));
    /// ```
    pub fn clamp(self, lo: Decimal, hi: Decimal) -> SignalValue {
        match self {
            SignalValue::Scalar(d) => SignalValue::Scalar(d.clamp(lo, hi)),
            SignalValue::Unavailable => SignalValue::Unavailable,
        }
    }

    /// Divides two signals: `Scalar(a) / Scalar(b)`.
    ///
    /// Returns `Unavailable` if either operand is `Unavailable` or `b` is zero.
    pub fn div(self, other: SignalValue) -> SignalValue {
        match (self, other) {
            (SignalValue::Scalar(a), SignalValue::Scalar(b)) => {
                if b.is_zero() {
                    SignalValue::Unavailable
                } else {
                    match a.checked_div(b) {
                        Some(result) => SignalValue::Scalar(result),
                        None => SignalValue::Unavailable,
                    }
                }
            }
            _ => SignalValue::Unavailable,
        }
    }

    /// Returns `true` if the scalar value is strictly positive. `Unavailable` returns `false`.
    pub fn is_positive(&self) -> bool {
        matches!(self, SignalValue::Scalar(d) if *d > Decimal::ZERO)
    }

    /// Returns `true` if the scalar value is strictly negative. `Unavailable` returns `false`.
    pub fn is_negative(&self) -> bool {
        matches!(self, SignalValue::Scalar(d) if *d < Decimal::ZERO)
    }

    /// Returns `default` if this is `Unavailable`; otherwise returns the scalar value.
    pub fn if_unavailable(self, default: Decimal) -> Decimal {
        match self {
            SignalValue::Scalar(v) => v,
            SignalValue::Unavailable => default,
        }
    }

    /// Returns `true` if the scalar value is strictly above `threshold`.
    ///
    /// `Unavailable` always returns `false`.
    pub fn is_above(&self, threshold: Decimal) -> bool {
        matches!(self, SignalValue::Scalar(d) if *d > threshold)
    }

    /// Returns `true` if the scalar value is strictly below `threshold`.
    ///
    /// `Unavailable` always returns `false`.
    pub fn is_below(&self, threshold: Decimal) -> bool {
        matches!(self, SignalValue::Scalar(d) if *d < threshold)
    }

    /// Rounds the scalar to `dp` decimal places using banker's rounding.
    ///
    /// Returns `Unavailable` unchanged.
    pub fn round(self, dp: u32) -> SignalValue {
        match self {
            SignalValue::Scalar(d) => SignalValue::Scalar(d.round_dp(dp)),
            SignalValue::Unavailable => SignalValue::Unavailable,
        }
    }

    /// Converts to `Option<Decimal>`: `Some(d)` for `Scalar(d)`, `None` for `Unavailable`.
    pub fn to_option(self) -> Option<Decimal> {
        match self {
            SignalValue::Scalar(d) => Some(d),
            SignalValue::Unavailable => None,
        }
    }

    /// Converts to `Option<f64>`: `Some(f64)` for `Scalar`, `None` for `Unavailable`.
    ///
    /// Precision may be lost in the `Decimal → f64` conversion.
    pub fn as_f64(&self) -> Option<f64> {
        use rust_decimal::prelude::ToPrimitive;
        match self {
            SignalValue::Scalar(d) => d.to_f64(),
            SignalValue::Unavailable => None,
        }
    }

    /// Returns the element-wise maximum of two signals.
    ///
    /// `Scalar(a).max(Scalar(b)) = Scalar(max(a, b))`.
    /// Returns `Unavailable` if either operand is `Unavailable`.
    pub fn max(self, other: SignalValue) -> SignalValue {
        match (self, other) {
            (SignalValue::Scalar(a), SignalValue::Scalar(b)) => SignalValue::Scalar(a.max(b)),
            _ => SignalValue::Unavailable,
        }
    }

    /// Returns the element-wise minimum of two signals.
    ///
    /// `Scalar(a).min(Scalar(b)) = Scalar(min(a, b))`.
    /// Returns `Unavailable` if either operand is `Unavailable`.
    pub fn min(self, other: SignalValue) -> SignalValue {
        match (self, other) {
            (SignalValue::Scalar(a), SignalValue::Scalar(b)) => SignalValue::Scalar(a.min(b)),
            _ => SignalValue::Unavailable,
        }
    }

    /// Returns `Scalar(-1)`, `Scalar(0)`, or `Scalar(1)` based on the sign of the value.
    ///
    /// Returns `Unavailable` if the value is unavailable.
    pub fn signum(self) -> SignalValue {
        match self {
            SignalValue::Scalar(v) => {
                let s = if v > Decimal::ZERO {
                    Decimal::ONE
                } else if v < Decimal::ZERO {
                    -Decimal::ONE
                } else {
                    Decimal::ZERO
                };
                SignalValue::Scalar(s)
            }
            SignalValue::Unavailable => SignalValue::Unavailable,
        }
    }

    /// Returns the square root of the scalar value.
    ///
    /// Uses f64 intermediate computation. Returns `Unavailable` if the value is
    /// negative or unavailable.
    ///
    /// ```rust
    /// use fin_primitives::signals::SignalValue;
    /// use rust_decimal_macros::dec;
    ///
    /// let v = SignalValue::Scalar(dec!(4));
    /// if let SignalValue::Scalar(r) = v.sqrt() {
    ///     assert!((r - dec!(2)).abs() < dec!(0.00001));
    /// }
    /// ```
    pub fn sqrt(self) -> SignalValue {
        use rust_decimal::prelude::ToPrimitive;
        match self {
            SignalValue::Scalar(v) => {
                if v < Decimal::ZERO {
                    return SignalValue::Unavailable;
                }
                let f = v.to_f64().unwrap_or(0.0).sqrt();
                Decimal::try_from(f)
                    .map(SignalValue::Scalar)
                    .unwrap_or(SignalValue::Unavailable)
            }
            SignalValue::Unavailable => SignalValue::Unavailable,
        }
    }

    /// Raises the scalar value to an integer power.
    ///
    /// Returns `Unavailable` if the value is unavailable.
    ///
    /// ```rust
    /// use fin_primitives::signals::SignalValue;
    /// use rust_decimal_macros::dec;
    ///
    /// assert_eq!(SignalValue::Scalar(dec!(3)).pow(2), SignalValue::Scalar(dec!(9)));
    /// ```
    pub fn pow(self, exp: u32) -> SignalValue {
        match self {
            SignalValue::Scalar(v) => {
                let mut result = Decimal::ONE;
                for _ in 0..exp {
                    result *= v;
                }
                SignalValue::Scalar(result)
            }
            SignalValue::Unavailable => SignalValue::Unavailable,
        }
    }

    /// Returns the natural logarithm of the scalar value.
    ///
    /// Returns `Unavailable` if the value is ≤ 0 or unavailable.
    ///
    /// ```rust
    /// use fin_primitives::signals::SignalValue;
    /// use rust_decimal_macros::dec;
    ///
    /// let v = SignalValue::Scalar(dec!(1));
    /// assert_eq!(v.ln(), SignalValue::Scalar(dec!(0)));
    /// assert_eq!(SignalValue::Scalar(dec!(-1)).ln(), SignalValue::Unavailable);
    /// ```
    pub fn ln(self) -> SignalValue {
        use rust_decimal::prelude::ToPrimitive;
        match self {
            SignalValue::Scalar(v) => {
                if v <= Decimal::ZERO {
                    return SignalValue::Unavailable;
                }
                let f = v.to_f64().unwrap_or(0.0).ln();
                if f.is_finite() {
                    Decimal::try_from(f)
                        .map(SignalValue::Scalar)
                        .unwrap_or(SignalValue::Unavailable)
                } else {
                    SignalValue::Unavailable
                }
            }
            SignalValue::Unavailable => SignalValue::Unavailable,
        }
    }

    /// Returns `true` if this value is above `threshold` while `prev` was at or below it.
    ///
    /// Detects an upward crossing of a threshold level. Both values must be scalar.
    ///
    /// ```rust
    /// use fin_primitives::signals::SignalValue;
    /// use rust_decimal_macros::dec;
    ///
    /// let prev = SignalValue::Scalar(dec!(49));
    /// let curr = SignalValue::Scalar(dec!(51));
    /// assert!(curr.cross_above(dec!(50), prev));
    /// ```
    pub fn cross_above(self, threshold: Decimal, prev: SignalValue) -> bool {
        matches!(
            (self, prev),
            (SignalValue::Scalar(curr), SignalValue::Scalar(p))
            if curr > threshold && p <= threshold
        )
    }

    /// Returns `true` if this value is below `threshold` while `prev` was at or above it.
    ///
    /// Detects a downward crossing of a threshold level. Both values must be scalar.
    ///
    /// ```rust
    /// use fin_primitives::signals::SignalValue;
    /// use rust_decimal_macros::dec;
    ///
    /// let prev = SignalValue::Scalar(dec!(51));
    /// let curr = SignalValue::Scalar(dec!(49));
    /// assert!(curr.cross_below(dec!(50), prev));
    /// ```
    pub fn cross_below(self, threshold: Decimal, prev: SignalValue) -> bool {
        matches!(
            (self, prev),
            (SignalValue::Scalar(curr), SignalValue::Scalar(p))
            if curr < threshold && p >= threshold
        )
    }

    /// Returns this scalar as a percentage of `other`.
    ///
    /// `result = (self / other) × 100`
    ///
    /// Returns `Unavailable` if either value is unavailable or `other` is zero.
    ///
    /// ```rust
    /// use fin_primitives::signals::SignalValue;
    /// use rust_decimal_macros::dec;
    ///
    /// let v = SignalValue::Scalar(dec!(50));
    /// let base = SignalValue::Scalar(dec!(200));
    /// assert_eq!(v.pct_of(base), SignalValue::Scalar(dec!(25)));
    /// ```
    pub fn pct_of(self, other: SignalValue) -> SignalValue {
        match (self, other) {
            (SignalValue::Scalar(a), SignalValue::Scalar(b)) => {
                if b.is_zero() {
                    return SignalValue::Unavailable;
                }
                match a.checked_div(b) {
                    Some(r) => SignalValue::Scalar(r * Decimal::ONE_HUNDRED),
                    None => SignalValue::Unavailable,
                }
            }
            _ => SignalValue::Unavailable,
        }
    }

    /// Returns `-1`, `0`, or `+1` depending on how this value crosses `threshold` from `prev`.
    ///
    /// - `+1` if `prev <= threshold` and `self > threshold` (upward crossing)
    /// - `-1` if `prev >= threshold` and `self < threshold` (downward crossing)
    /// - `0` otherwise (no crossing, or either value is unavailable)
    ///
    /// ```rust
    /// use fin_primitives::signals::SignalValue;
    /// use rust_decimal_macros::dec;
    ///
    /// let prev = SignalValue::Scalar(dec!(49));
    /// let curr = SignalValue::Scalar(dec!(51));
    /// assert_eq!(curr.threshold_cross(dec!(50), prev), SignalValue::Scalar(dec!(1)));
    /// ```
    pub fn threshold_cross(self, threshold: Decimal, prev: SignalValue) -> SignalValue {
        match (self, prev) {
            (SignalValue::Scalar(curr), SignalValue::Scalar(p)) => {
                if curr > threshold && p <= threshold {
                    SignalValue::Scalar(Decimal::ONE)
                } else if curr < threshold && p >= threshold {
                    SignalValue::Scalar(Decimal::NEGATIVE_ONE)
                } else {
                    SignalValue::Scalar(Decimal::ZERO)
                }
            }
            _ => SignalValue::Scalar(Decimal::ZERO),
        }
    }

    /// Returns `e^x`. Returns `Unavailable` if the value is `Unavailable` or if `x > 700`
    /// (overflow guard — `e^709 ≈ f64::MAX`).
    pub fn exp(self) -> SignalValue {
        match self {
            SignalValue::Unavailable => SignalValue::Unavailable,
            SignalValue::Scalar(v) => {
                if v > Decimal::from(700) {
                    return SignalValue::Unavailable;
                }
                use rust_decimal::prelude::ToPrimitive;
                let f = v.to_f64().unwrap_or(f64::NAN);
                if f.is_nan() { return SignalValue::Unavailable; }
                match Decimal::try_from(f.exp()) {
                    Ok(d) => SignalValue::Scalar(d),
                    Err(_) => SignalValue::Unavailable,
                }
            }
        }
    }

    /// Returns the floor of the value (rounds toward negative infinity).
    pub fn floor(self) -> SignalValue {
        self.map(|v| v.floor())
    }

    /// Returns the ceiling of the value (rounds toward positive infinity).
    pub fn ceil(self) -> SignalValue {
        self.map(|v| v.ceil())
    }

    /// Returns `1 / self`. Returns `Unavailable` if the value is zero or `Unavailable`.
    pub fn reciprocal(self) -> SignalValue {
        match self {
            SignalValue::Unavailable => SignalValue::Unavailable,
            SignalValue::Scalar(v) => {
                if v.is_zero() {
                    SignalValue::Unavailable
                } else {
                    SignalValue::Scalar(Decimal::ONE / v)
                }
            }
        }
    }

    /// Returns `(self / total) * 100`. Returns `Unavailable` if `total` is zero or either
    /// value is `Unavailable`.
    pub fn to_percent(self, total: SignalValue) -> SignalValue {
        match (self, total) {
            (SignalValue::Scalar(v), SignalValue::Scalar(t)) => {
                if t.is_zero() {
                    SignalValue::Unavailable
                } else {
                    SignalValue::Scalar(v / t * Decimal::ONE_HUNDRED)
                }
            }
            _ => SignalValue::Unavailable,
        }
    }

    /// Returns the arctangent of the value in radians. Returns `Unavailable` if unavailable.
    pub fn atan(self) -> SignalValue {
        match self {
            SignalValue::Unavailable => SignalValue::Unavailable,
            SignalValue::Scalar(v) => {
                use rust_decimal::prelude::ToPrimitive;
                let f: f64 = v.to_f64().unwrap_or(f64::NAN);
                match Decimal::try_from(f.atan()) {
                    Ok(d) => SignalValue::Scalar(d),
                    Err(_) => SignalValue::Unavailable,
                }
            }
        }
    }

    /// Returns the hyperbolic tangent of the value. Returns `Unavailable` if unavailable.
    ///
    /// `tanh` maps any real value to `(-1, 1)` — useful for normalising unbounded signals.
    pub fn tanh(self) -> SignalValue {
        match self {
            SignalValue::Unavailable => SignalValue::Unavailable,
            SignalValue::Scalar(v) => {
                use rust_decimal::prelude::ToPrimitive;
                let f: f64 = v.to_f64().unwrap_or(f64::NAN);
                match Decimal::try_from(f.tanh()) {
                    Ok(d) => SignalValue::Scalar(d),
                    Err(_) => SignalValue::Unavailable,
                }
            }
        }
    }

    /// Returns the hyperbolic sine of the scalar value.
    ///
    /// Returns [`SignalValue::Unavailable`] if the result is non-finite.
    pub fn sinh(self) -> SignalValue {
        match self {
            SignalValue::Unavailable => SignalValue::Unavailable,
            SignalValue::Scalar(v) => {
                use rust_decimal::prelude::ToPrimitive;
                let f: f64 = v.to_f64().unwrap_or(f64::NAN);
                match Decimal::try_from(f.sinh()) {
                    Ok(d) => SignalValue::Scalar(d),
                    Err(_) => SignalValue::Unavailable,
                }
            }
        }
    }

    /// Returns the hyperbolic cosine of the scalar value.
    ///
    /// Returns [`SignalValue::Unavailable`] if the result is non-finite.
    pub fn cosh(self) -> SignalValue {
        match self {
            SignalValue::Unavailable => SignalValue::Unavailable,
            SignalValue::Scalar(v) => {
                use rust_decimal::prelude::ToPrimitive;
                let f: f64 = v.to_f64().unwrap_or(f64::NAN);
                match Decimal::try_from(f.cosh()) {
                    Ok(d) => SignalValue::Scalar(d),
                    Err(_) => SignalValue::Unavailable,
                }
            }
        }
    }

    /// Rounds the scalar to `dp` decimal places using banker's rounding.
    ///
    /// Returns [`SignalValue::Unavailable`] unchanged.
    pub fn round_to(self, dp: u32) -> SignalValue {
        match self {
            SignalValue::Unavailable => SignalValue::Unavailable,
            SignalValue::Scalar(v) => SignalValue::Scalar(v.round_dp(dp)),
        }
    }

    /// Returns `true` if this is a `Scalar` with a non-zero value.
    pub fn to_bool(&self) -> bool {
        matches!(self, SignalValue::Scalar(v) if !v.is_zero())
    }

    /// Multiplies the scalar by `factor`, returning the product as a new `SignalValue`.
    ///
    /// Returns [`SignalValue::Unavailable`] unchanged.
    pub fn scale_by(self, factor: rust_decimal::Decimal) -> SignalValue {
        match self {
            SignalValue::Unavailable => SignalValue::Unavailable,
            SignalValue::Scalar(v) => SignalValue::Scalar(v * factor),
        }
    }

    /// Returns `true` if this is `Scalar(0)`.
    pub fn is_zero(&self) -> bool {
        matches!(self, SignalValue::Scalar(v) if v.is_zero())
    }

    /// Absolute difference between two `SignalValue`s.
    ///
    /// Returns `Unavailable` if either operand is `Unavailable`.
    pub fn delta(self, other: SignalValue) -> SignalValue {
        match (self, other) {
            (SignalValue::Scalar(a), SignalValue::Scalar(b)) => SignalValue::Scalar((a - b).abs()),
            _ => SignalValue::Unavailable,
        }
    }

    /// Linear interpolation: `self * (1 - t) + other * t`.
    ///
    /// `t` is clamped to `[0, 1]`. Returns `Unavailable` if either operand is `Unavailable`.
    pub fn lerp(self, other: SignalValue, t: Decimal) -> SignalValue {
        match (self, other) {
            (SignalValue::Scalar(a), SignalValue::Scalar(b)) => {
                let t_clamped = t.max(Decimal::ZERO).min(Decimal::ONE);
                SignalValue::Scalar(a * (Decimal::ONE - t_clamped) + b * t_clamped)
            }
            _ => SignalValue::Unavailable,
        }
    }

    /// Returns `true` if `self` is a scalar strictly greater than `other`.
    ///
    /// Returns `false` if either operand is `Unavailable`.
    pub fn gt(&self, other: &SignalValue) -> bool {
        match (self, other) {
            (SignalValue::Scalar(a), SignalValue::Scalar(b)) => a > b,
            _ => false,
        }
    }

    /// Returns `true` if `self` is a scalar strictly less than `other`.
    ///
    /// Returns `false` if either operand is `Unavailable`.
    pub fn lt(&self, other: &SignalValue) -> bool {
        match (self, other) {
            (SignalValue::Scalar(a), SignalValue::Scalar(b)) => a < b,
            _ => false,
        }
    }

    /// Returns `true` if both are scalars and `|self - other| <= tolerance`.
    ///
    /// Returns `false` if either is `Unavailable`.
    pub fn eq_approx(&self, other: &SignalValue, tolerance: Decimal) -> bool {
        match (self, other) {
            (SignalValue::Scalar(a), SignalValue::Scalar(b)) => (a - b).abs() <= tolerance,
            _ => false,
        }
    }

    /// Two-argument arctangent: `atan2(self, x)` in radians.
    ///
    /// Treats `self` as the `y` argument. Returns `Unavailable` if either is `Unavailable`.
    pub fn atan2(self, x: SignalValue) -> SignalValue {
        match (self, x) {
            (SignalValue::Scalar(y), SignalValue::Scalar(xv)) => {
                use rust_decimal::prelude::ToPrimitive;
                let yf: f64 = y.to_f64().unwrap_or(f64::NAN);
                let xf: f64 = xv.to_f64().unwrap_or(f64::NAN);
                match Decimal::try_from(yf.atan2(xf)) {
                    Ok(d) => SignalValue::Scalar(d),
                    Err(_) => SignalValue::Unavailable,
                }
            }
            _ => SignalValue::Unavailable,
        }
    }

    /// Returns `true` if both scalars have the same sign (both positive or both negative).
    ///
    /// Zero is treated as positive. Returns `false` if either is `Unavailable`.
    pub fn sign_match(&self, other: &SignalValue) -> bool {
        match (self, other) {
            (SignalValue::Scalar(a), SignalValue::Scalar(b)) => {
                (a >= &Decimal::ZERO) == (b >= &Decimal::ZERO)
            }
            _ => false,
        }
    }

    /// Adds a raw `Decimal` to this scalar value.
    ///
    /// Returns `Unavailable` if `self` is `Unavailable`.
    pub fn add_scalar(self, delta: Decimal) -> SignalValue {
        match self {
            SignalValue::Scalar(v) => SignalValue::Scalar(v + delta),
            SignalValue::Unavailable => SignalValue::Unavailable,
        }
    }

    /// Maps the scalar with `f`, falling back to `default` if `Unavailable`.
    pub fn map_or(self, default: Decimal, f: impl FnOnce(Decimal) -> Decimal) -> Decimal {
        match self {
            SignalValue::Scalar(v) => f(v),
            SignalValue::Unavailable => default,
        }
    }

    /// Returns `true` if `self >= other` (both scalar). Returns `false` if either is `Unavailable`.
    pub fn gte(&self, other: &SignalValue) -> bool {
        match (self, other) {
            (SignalValue::Scalar(a), SignalValue::Scalar(b)) => a >= b,
            _ => false,
        }
    }

    /// Returns `true` if `self <= other` (both scalar). Returns `false` if either is `Unavailable`.
    pub fn lte(&self, other: &SignalValue) -> bool {
        match (self, other) {
            (SignalValue::Scalar(a), SignalValue::Scalar(b)) => a <= b,
            _ => false,
        }
    }

    /// Express this scalar as a percentage of `base`: `self / base * 100`.
    ///
    /// Returns `Unavailable` if `self` is `Unavailable` or `base` is zero.
    pub fn as_percent(self, base: Decimal) -> SignalValue {
        if base.is_zero() { return SignalValue::Unavailable; }
        match self {
            SignalValue::Scalar(v) => SignalValue::Scalar(v / base * Decimal::ONE_HUNDRED),
            SignalValue::Unavailable => SignalValue::Unavailable,
        }
    }

    /// Returns `true` if this scalar is in `[lo, hi]` (inclusive).
    ///
    /// Returns `false` if `Unavailable`.
    pub fn within_range(&self, lo: Decimal, hi: Decimal) -> bool {
        match self {
            SignalValue::Scalar(v) => v >= &lo && v <= &hi,
            SignalValue::Unavailable => false,
        }
    }

    /// Caps the scalar at `max_val`. Returns `Unavailable` if `self` is `Unavailable`.
    pub fn cap_at(self, max_val: Decimal) -> SignalValue {
        match self {
            SignalValue::Scalar(v) => SignalValue::Scalar(v.min(max_val)),
            SignalValue::Unavailable => SignalValue::Unavailable,
        }
    }

    /// Floors the scalar at `min_val`. Returns `Unavailable` if `self` is `Unavailable`.
    pub fn floor_at(self, min_val: Decimal) -> SignalValue {
        match self {
            SignalValue::Scalar(v) => SignalValue::Scalar(v.max(min_val)),
            SignalValue::Unavailable => SignalValue::Unavailable,
        }
    }

    /// Round the scalar to the nearest multiple of `step`. Returns `Unavailable` if unavailable
    /// or `step` is zero.
    pub fn quantize(self, step: Decimal) -> SignalValue {
        if step.is_zero() {
            return SignalValue::Unavailable;
        }
        match self {
            SignalValue::Scalar(v) => SignalValue::Scalar((v / step).round() * step),
            SignalValue::Unavailable => SignalValue::Unavailable,
        }
    }

    /// Absolute difference between `self` and `other`. Returns `Unavailable` if either is unavailable.
    pub fn distance_to(self, other: SignalValue) -> SignalValue {
        match (self, other) {
            (SignalValue::Scalar(a), SignalValue::Scalar(b)) => SignalValue::Scalar((a - b).abs()),
            _ => SignalValue::Unavailable,
        }
    }

    /// Weighted blend: `self * (1 - weight) + other * weight`, clamping `weight` to `[0, 1]`.
    /// Returns `Unavailable` if either operand is unavailable.
    pub fn blend(self, other: SignalValue, weight: Decimal) -> SignalValue {
        match (self, other) {
            (SignalValue::Scalar(a), SignalValue::Scalar(b)) => {
                let w = weight.max(Decimal::ZERO).min(Decimal::ONE);
                SignalValue::Scalar(a * (Decimal::ONE - w) + b * w)
            }
            _ => SignalValue::Unavailable,
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

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_signal_value_and_then_scalar_returns_value() {
        let v = SignalValue::Scalar(dec!(50));
        let result = v.and_then(|x| SignalValue::Scalar(x * dec!(2)));
        assert_eq!(result, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_signal_value_and_then_scalar_can_return_unavailable() {
        let v = SignalValue::Scalar(dec!(5));
        let result = v.and_then(|x| {
            if x > dec!(10) { SignalValue::Scalar(x) } else { SignalValue::Unavailable }
        });
        assert_eq!(result, SignalValue::Unavailable);
    }

    #[test]
    fn test_signal_value_and_then_unavailable_short_circuits() {
        let v = SignalValue::Unavailable;
        let result = v.and_then(|_| SignalValue::Scalar(dec!(999)));
        assert_eq!(result, SignalValue::Unavailable);
    }

    #[test]
    fn test_signal_value_map_scalar() {
        let v = SignalValue::Scalar(dec!(10));
        assert_eq!(v.map(|x| x + dec!(5)), SignalValue::Scalar(dec!(15)));
    }

    #[test]
    fn test_signal_value_map_unavailable() {
        assert_eq!(SignalValue::Unavailable.map(|x| x + dec!(5)), SignalValue::Unavailable);
    }

    #[test]
    fn test_signal_value_zip_with_both_scalar() {
        let a = SignalValue::Scalar(dec!(10));
        let b = SignalValue::Scalar(dec!(3));
        assert_eq!(a.zip_with(b, |x, y| x - y), SignalValue::Scalar(dec!(7)));
    }

    #[test]
    fn test_signal_value_zip_with_one_unavailable() {
        let a = SignalValue::Scalar(dec!(10));
        assert_eq!(a.zip_with(SignalValue::Unavailable, |x, y| x + y), SignalValue::Unavailable);
    }

    #[test]
    fn test_signal_value_clamp_above_hi() {
        let v = SignalValue::Scalar(dec!(105));
        assert_eq!(v.clamp(dec!(0), dec!(100)), SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_signal_value_clamp_below_lo() {
        let v = SignalValue::Scalar(dec!(-5));
        assert_eq!(v.clamp(dec!(0), dec!(100)), SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_signal_value_clamp_within_range() {
        let v = SignalValue::Scalar(dec!(50));
        assert_eq!(v.clamp(dec!(0), dec!(100)), SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_signal_value_clamp_unavailable_passthrough() {
        assert_eq!(SignalValue::Unavailable.clamp(dec!(0), dec!(100)), SignalValue::Unavailable);
    }

    #[test]
    fn test_signal_value_exp_zero() {
        // e^0 = 1
        let v = SignalValue::Scalar(dec!(0));
        if let SignalValue::Scalar(r) = v.exp() {
            let diff = (r - dec!(1)).abs();
            assert!(diff < dec!(0.0001), "e^0 should be ~1, got {r}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_signal_value_exp_overflow_guard() {
        assert_eq!(SignalValue::Scalar(dec!(701)).exp(), SignalValue::Unavailable);
    }

    #[test]
    fn test_signal_value_exp_unavailable_passthrough() {
        assert_eq!(SignalValue::Unavailable.exp(), SignalValue::Unavailable);
    }

    #[test]
    fn test_signal_value_floor_positive() {
        assert_eq!(SignalValue::Scalar(dec!(3.7)).floor(), SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_signal_value_floor_negative() {
        assert_eq!(SignalValue::Scalar(dec!(-2.3)).floor(), SignalValue::Scalar(dec!(-3)));
    }

    #[test]
    fn test_signal_value_ceil_positive() {
        assert_eq!(SignalValue::Scalar(dec!(3.2)).ceil(), SignalValue::Scalar(dec!(4)));
    }

    #[test]
    fn test_signal_value_ceil_integer() {
        assert_eq!(SignalValue::Scalar(dec!(5)).ceil(), SignalValue::Scalar(dec!(5)));
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
