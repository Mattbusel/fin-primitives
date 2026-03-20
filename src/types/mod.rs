//! # Module: types
//!
//! ## Responsibility
//! Provides the core validated newtype wrappers used throughout fin-primitives:
//! `Symbol`, `Price`, `Quantity`, `Side`, and `NanoTimestamp`.
//!
//! ## Guarantees
//! - `Symbol`: non-empty, no whitespace; backed by `Arc<str>` for O(1) clone
//! - `Price`: strictly positive (`> 0`)
//! - `Quantity`: non-negative (`>= 0`)
//! - `NanoTimestamp`: nanosecond-resolution UTC epoch timestamp; inner field is private
//! - All types implement `Clone`, `Copy` (where applicable), `serde::{Serialize, Deserialize}`
//!
//! ## NOT Responsible For
//! - Currency conversion
//! - Tick size enforcement (exchange-specific)

use crate::error::FinError;
use chrono::{DateTime, TimeZone, Utc};
use rust_decimal::Decimal;
use std::sync::Arc;

/// A validated ticker symbol: non-empty, contains no whitespace.
///
/// Backed by `Arc<str>` so cloning is O(1).
///
/// # Example
/// ```rust
/// use fin_primitives::types::Symbol;
/// let sym = Symbol::new("AAPL").unwrap();
/// assert_eq!(sym.as_str(), "AAPL");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Symbol(Arc<str>);

impl Symbol {
    /// Construct a validated `Symbol`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidSymbol`] if the string is empty or contains whitespace.
    pub fn new(s: impl AsRef<str>) -> Result<Self, FinError> {
        let s = s.as_ref();
        if s.is_empty() || s.chars().any(char::is_whitespace) {
            return Err(FinError::InvalidSymbol(s.to_owned()));
        }
        Ok(Self(Arc::from(s)))
    }

    /// Returns the inner string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the number of bytes in the symbol string.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns `true` if the symbol string is empty.
    ///
    /// Note: construction always rejects empty strings, so this always returns `false`
    /// for any successfully constructed `Symbol`.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Display for Symbol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for Symbol {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::borrow::Borrow<str> for Symbol {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Symbol {
    type Error = FinError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Symbol::new(s)
    }
}

impl TryFrom<&str> for Symbol {
    type Error = FinError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Symbol::new(s)
    }
}

impl PartialOrd for Symbol {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Symbol {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.as_ref().cmp(other.0.as_ref())
    }
}

impl From<Symbol> for String {
    fn from(s: Symbol) -> Self {
        s.as_str().to_owned()
    }
}

impl From<Symbol> for Arc<str> {
    fn from(s: Symbol) -> Self {
        s.0.clone()
    }
}

/// A strictly positive price value backed by [`Decimal`].
///
/// # Example
/// ```rust
/// use fin_primitives::types::Price;
/// use rust_decimal_macros::dec;
/// let p = Price::new(dec!(100.50)).unwrap();
/// assert_eq!(p.value(), dec!(100.50));
/// ```
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct Price(Decimal);

impl Price {
    /// Construct a validated `Price`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPrice`] if `d <= 0`.
    pub fn new(d: Decimal) -> Result<Self, FinError> {
        if d <= Decimal::ZERO {
            return Err(FinError::InvalidPrice(d));
        }
        Ok(Self(d))
    }

    /// Returns the inner [`Decimal`] value.
    pub fn value(&self) -> Decimal {
        self.0
    }

    /// Converts to `f64` with possible precision loss.
    pub fn to_f64(&self) -> f64 {
        rust_decimal::prelude::ToPrimitive::to_f64(&self.0).unwrap_or(f64::NAN)
    }

    /// Constructs a `Price` from an `f64`. Returns `None` if `f` is not finite or `<= 0`.
    pub fn from_f64(f: f64) -> Option<Self> {
        use rust_decimal::prelude::FromPrimitive;
        let d = Decimal::from_f64(f)?;
        Self::new(d).ok()
    }

    /// Returns a `String` representation rounded to `dp` decimal places.
    ///
    /// Useful for display/logging without losing the underlying decimal precision.
    pub fn to_string_with_dp(&self, dp: u32) -> String {
        self.0.round_dp(dp).to_string()
    }
}

impl Price {
    /// Returns the percentage change from `self` to `other`: `(other - self) / self * 100`.
    ///
    /// Positive values indicate a price increase; negative values indicate a decrease.
    pub fn pct_change_to(self, other: Price) -> Decimal {
        (other.0 - self.0) / self.0 * Decimal::ONE_HUNDRED
    }

    /// Returns the midpoint between `self` and `other`: `(self + other) / 2`.
    pub fn mid(self, other: Price) -> Price {
        Price((self.0 + other.0) / Decimal::TWO)
    }
}

impl Price {
    /// Returns the absolute difference between `self` and `other`: `|self - other|`.
    pub fn abs_diff(self, other: Price) -> Decimal {
        (self.0 - other.0).abs()
    }

    /// Rounds this price to the nearest multiple of `tick_size`.
    ///
    /// Returns `None` if `tick_size <= 0` or if the rounded value is not a valid
    /// `Price` (i.e. the result is zero or negative).
    ///
    /// # Example
    /// ```rust
    /// use fin_primitives::types::Price;
    /// use rust_decimal_macros::dec;
    ///
    /// let p = Price::new(dec!(10.3)).unwrap();
    /// let snapped = p.snap_to_tick(dec!(0.5)).unwrap();
    /// assert_eq!(snapped.value(), dec!(10.5));
    /// ```
    pub fn snap_to_tick(self, tick_size: Decimal) -> Option<Price> {
        if tick_size <= Decimal::ZERO {
            return None;
        }
        let rounded = (self.0 / tick_size).round() * tick_size;
        Price::new(rounded).ok()
    }

    /// Clamps this price to the inclusive range `[lo, hi]`.
    ///
    /// Returns `lo` if `self < lo`, `hi` if `self > hi`, otherwise `self`.
    pub fn clamp(self, lo: Price, hi: Price) -> Price {
        if self.0 < lo.0 {
            lo
        } else if self.0 > hi.0 {
            hi
        } else {
            self
        }
    }
}

impl Price {
    /// Rounds this price to `dp` decimal places using banker's rounding.
    ///
    /// Returns `None` if the rounded value is zero or negative (invalid price).
    pub fn round_to(self, dp: u32) -> Option<Price> {
        let rounded = self.0.round_dp(dp);
        Price::new(rounded).ok()
    }
}

impl Price {
    /// Adds `other` to `self`, returning the result as a `Price`, or `None` on overflow.
    ///
    /// Useful when combining two price levels and needing a validated result.
    pub fn checked_add(self, other: Price) -> Option<Price> {
        let sum = self.0.checked_add(other.0)?;
        Price::new(sum).ok()
    }
}

impl Price {
    /// Multiplies this price by `qty`, returning `None` if the result overflows.
    ///
    /// Prefer this over the `*` operator when overflow is a concern (e.g., large
    /// notional values with many decimal digits).
    pub fn checked_mul(self, qty: Quantity) -> Option<Decimal> {
        self.0.checked_mul(qty.0)
    }
}

impl Price {
    /// Returns the midpoint between `bid` and `ask`: `(bid + ask) / 2`.
    ///
    /// Useful for computing the theoretical fair value between two prices.
    pub fn midpoint(bid: Price, ask: Price) -> Decimal {
        (bid.0 + ask.0) / Decimal::TWO
    }

    /// Linearly interpolates between `self` and `other` by factor `t` in `[0, 1]`.
    ///
    /// Returns `self + (other - self) * t`. Returns `None` if `t` is outside `[0, 1]`
    /// or if the result is not a valid price (i.e., not strictly positive).
    pub fn lerp(self, other: Price, t: Decimal) -> Option<Price> {
        if t < Decimal::ZERO || t > Decimal::ONE {
            return None;
        }
        let result = self.0 + (other.0 - self.0) * t;
        Price::new(result).ok()
    }
}

impl std::fmt::Display for Price {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// `Price + Price` yields a raw `Decimal` (sum is not necessarily a valid price in all contexts).
impl std::ops::Add<Price> for Price {
    type Output = Decimal;
    fn add(self, rhs: Price) -> Decimal {
        self.0 + rhs.0
    }
}

/// `Price - Price` yields a raw `Decimal` (difference may be zero or negative).
impl std::ops::Sub<Price> for Price {
    type Output = Decimal;
    fn sub(self, rhs: Price) -> Decimal {
        self.0 - rhs.0
    }
}

/// `Price * Quantity` yields the notional value as `Decimal`.
impl std::ops::Mul<Quantity> for Price {
    type Output = Decimal;
    fn mul(self, rhs: Quantity) -> Decimal {
        self.0 * rhs.0
    }
}

/// `Price * Decimal` scales a price; returns `None` if the result is not a valid `Price`.
impl std::ops::Mul<Decimal> for Price {
    type Output = Option<Price>;
    fn mul(self, rhs: Decimal) -> Option<Price> {
        Price::new(self.0 * rhs).ok()
    }
}

/// A non-negative quantity backed by [`Decimal`].
///
/// # Example
/// ```rust
/// use fin_primitives::types::Quantity;
/// use rust_decimal_macros::dec;
/// let q = Quantity::zero();
/// assert_eq!(q.value(), dec!(0));
/// ```
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct Quantity(Decimal);

impl Quantity {
    /// Construct a validated `Quantity`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidQuantity`] if `d < 0`.
    pub fn new(d: Decimal) -> Result<Self, FinError> {
        if d < Decimal::ZERO {
            return Err(FinError::InvalidQuantity(d));
        }
        Ok(Self(d))
    }

    /// Returns a zero quantity without allocation.
    pub fn zero() -> Self {
        Self(Decimal::ZERO)
    }

    /// Returns `true` if this quantity is zero.
    pub fn is_zero(&self) -> bool {
        self.0 == Decimal::ZERO
    }

    /// Returns the inner [`Decimal`] value.
    pub fn value(&self) -> Decimal {
        self.0
    }

    /// Converts to `f64` with possible precision loss.
    pub fn to_f64(&self) -> f64 {
        rust_decimal::prelude::ToPrimitive::to_f64(&self.0).unwrap_or(f64::NAN)
    }

    /// Constructs a `Quantity` from an `f64`. Returns `None` if `f` is not finite or `< 0`.
    pub fn from_f64(f: f64) -> Option<Self> {
        use rust_decimal::prelude::FromPrimitive;
        let d = Decimal::from_f64(f)?;
        Self::new(d).ok()
    }
}

impl Quantity {
    /// Adds `other` to this quantity, returning `None` if the result overflows.
    pub fn checked_add(self, other: Quantity) -> Option<Quantity> {
        self.0.checked_add(other.0).map(Quantity)
    }

    /// Subtracts `other` from `self`, returning `None` if the result would be negative or overflow.
    pub fn checked_sub(self, other: Quantity) -> Option<Quantity> {
        let result = self.0.checked_sub(other.0)?;
        if result < Decimal::ZERO {
            None
        } else {
            Some(Quantity(result))
        }
    }

    /// Returns the absolute value of this quantity's underlying decimal.
    ///
    /// `Quantity` values are normally non-negative, but this is useful when
    /// working with raw `Decimal` fields (e.g. from `sub` operations that yield
    /// negative `Decimal`s wrapped in `Quantity(d)` via internal code paths).
    pub fn abs(self) -> Quantity {
        Quantity(self.0.abs())
    }

    /// Multiplies this quantity by `factor`, returning `None` if the result is negative.
    ///
    /// Useful for scaling position sizes by a fraction (e.g. `0.5` for half-position).
    /// Returns `None` if `factor` is negative (which would produce an invalid quantity).
    pub fn scale(self, factor: Decimal) -> Option<Quantity> {
        if factor < Decimal::ZERO {
            return None;
        }
        Some(Quantity(self.0 * factor))
    }
}

impl std::fmt::Display for Quantity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// `Quantity + Quantity` always yields a valid `Quantity` (sum of non-negatives is non-negative).
impl std::ops::Add<Quantity> for Quantity {
    type Output = Quantity;
    fn add(self, rhs: Quantity) -> Quantity {
        Quantity(self.0 + rhs.0)
    }
}

/// `Quantity - Quantity` yields a raw `Decimal` (result may be negative).
impl std::ops::Sub<Quantity> for Quantity {
    type Output = Decimal;
    fn sub(self, rhs: Quantity) -> Decimal {
        self.0 - rhs.0
    }
}

/// `Quantity * Decimal` scales a quantity; yields raw `Decimal`.
impl std::ops::Mul<Decimal> for Quantity {
    type Output = Decimal;
    fn mul(self, rhs: Decimal) -> Decimal {
        self.0 * rhs
    }
}

/// The side of a market order or book level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Side {
    /// Buy side (bids).
    Bid,
    /// Sell side (asks).
    Ask,
}

impl Side {
    /// Returns the opposite side: `Bid` → `Ask`, `Ask` → `Bid`.
    pub fn opposite(self) -> Side {
        match self {
            Side::Bid => Side::Ask,
            Side::Ask => Side::Bid,
        }
    }
}

impl std::fmt::Display for Side {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Side::Bid => f.write_str("Bid"),
            Side::Ask => f.write_str("Ask"),
        }
    }
}

/// Exchange-epoch timestamp with nanosecond resolution.
///
/// Stores nanoseconds since the Unix epoch (UTC). The inner field is private;
/// use [`NanoTimestamp::new`] to construct and [`NanoTimestamp::nanos`] to read.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct NanoTimestamp(i64);

impl NanoTimestamp {
    /// Smallest representable timestamp (earliest possible time).
    pub const MIN: NanoTimestamp = NanoTimestamp(i64::MIN);

    /// Largest representable timestamp (latest possible time).
    pub const MAX: NanoTimestamp = NanoTimestamp(i64::MAX);
    /// Constructs a `NanoTimestamp` from a raw nanosecond integer.
    pub fn new(nanos: i64) -> Self {
        Self(nanos)
    }

    /// Returns the raw nanosecond value.
    pub fn nanos(&self) -> i64 {
        self.0
    }

    /// Returns the current UTC time as a `NanoTimestamp`.
    ///
    /// Falls back to `0` if the system clock overflows nanosecond range (extremely unlikely).
    pub fn now() -> Self {
        Self(Utc::now().timestamp_nanos_opt().unwrap_or(0))
    }

    /// Returns the nanoseconds elapsed since `self` (i.e. `NanoTimestamp::now() - self`).
    ///
    /// Positive when `self` is in the past, negative when `self` is in the future.
    pub fn elapsed(&self) -> i64 {
        NanoTimestamp::now().0 - self.0
    }

    /// Returns the signed nanosecond difference `self - other`.
    ///
    /// Positive when `self` is later than `other`, negative when earlier.
    pub fn duration_since(&self, other: NanoTimestamp) -> i64 {
        self.0 - other.0
    }

    /// Returns the signed millisecond difference `self - other`.
    ///
    /// Positive when `self` is later than `other`. Rounds toward zero (truncates
    /// sub-millisecond nanoseconds).
    pub fn diff_millis(&self, other: NanoTimestamp) -> i64 {
        (self.0 - other.0) / 1_000_000
    }

    /// Returns `Some(nanos)` if `self >= other` (non-negative elapsed time), otherwise `None`.
    ///
    /// Use this when you want to measure forward elapsed time and treat a negative
    /// difference as "not yet elapsed" rather than a negative value.
    pub fn elapsed_nanos_since(&self, other: NanoTimestamp) -> Option<i64> {
        let diff = self.0 - other.0;
        if diff >= 0 {
            Some(diff)
        } else {
            None
        }
    }

    /// Returns a new `NanoTimestamp` offset by `nanos` (positive = forward in time).
    pub fn add_nanos(&self, nanos: i64) -> NanoTimestamp {
        NanoTimestamp(self.0 + nanos)
    }

    /// Returns a new `NanoTimestamp` offset by `ms` milliseconds.
    pub fn add_millis(&self, ms: i64) -> NanoTimestamp {
        NanoTimestamp(self.0 + ms * 1_000_000)
    }

    /// Returns a new `NanoTimestamp` offset by `secs` seconds.
    pub fn add_seconds(&self, secs: i64) -> NanoTimestamp {
        NanoTimestamp(self.0 + secs * 1_000_000_000)
    }

    /// Returns `true` if `self` is strictly earlier than `other`.
    pub fn is_before(&self, other: NanoTimestamp) -> bool {
        self.0 < other.0
    }

    /// Returns `true` if `self` is strictly later than `other`.
    pub fn is_after(&self, other: NanoTimestamp) -> bool {
        self.0 > other.0
    }

    /// Returns `true` if `self` and `other` fall within the same calendar second.
    ///
    /// Two timestamps are in the same second when
    /// `floor(self / 1_000_000_000) == floor(other / 1_000_000_000)`.
    pub fn is_same_second(&self, other: NanoTimestamp) -> bool {
        self.0.div_euclid(1_000_000_000) == other.0.div_euclid(1_000_000_000)
    }

    /// Returns `true` if `self` and `other` fall within the same calendar minute.
    ///
    /// Two timestamps are in the same minute when
    /// `floor(self / 60_000_000_000) == floor(other / 60_000_000_000)`.
    pub fn is_same_minute(&self, other: NanoTimestamp) -> bool {
        self.0.div_euclid(60_000_000_000) == other.0.div_euclid(60_000_000_000)
    }

    /// Constructs a `NanoTimestamp` from milliseconds since the Unix epoch.
    pub fn from_millis(ms: i64) -> Self {
        Self(ms * 1_000_000)
    }

    /// Returns milliseconds since the Unix epoch (truncates sub-millisecond precision).
    pub fn to_millis(&self) -> i64 {
        self.0 / 1_000_000
    }

    /// Constructs a `NanoTimestamp` from whole seconds since the Unix epoch.
    pub fn from_secs(secs: i64) -> Self {
        Self(secs * 1_000_000_000)
    }

    /// Returns whole seconds since the Unix epoch (truncates sub-second precision).
    pub fn to_secs(&self) -> i64 {
        self.0 / 1_000_000_000
    }

    /// Constructs a `NanoTimestamp` from a [`DateTime<Utc>`].
    ///
    /// Falls back to `0` if the datetime is outside the representable nanosecond range.
    pub fn from_datetime(dt: DateTime<Utc>) -> Self {
        Self(dt.timestamp_nanos_opt().unwrap_or(0))
    }

    /// Converts this timestamp to a [`DateTime<Utc>`].
    pub fn to_datetime(&self) -> DateTime<Utc> {
        let secs = self.0 / 1_000_000_000;
        #[allow(clippy::cast_sign_loss)]
        let nanos = (self.0 % 1_000_000_000) as u32;
        Utc.timestamp_opt(secs, nanos).single().unwrap_or_else(|| {
            Utc.timestamp_opt(0, 0)
                .single()
                .unwrap_or(DateTime::<Utc>::MIN_UTC)
        })
    }

    /// Returns the earlier of `self` and `other`.
    pub fn min(self, other: NanoTimestamp) -> NanoTimestamp {
        if self.0 <= other.0 { self } else { other }
    }

    /// Returns the later of `self` and `other`.
    pub fn max(self, other: NanoTimestamp) -> NanoTimestamp {
        if self.0 >= other.0 { self } else { other }
    }

    /// Returns the signed nanosecond difference `self - earlier`.
    ///
    /// Positive when `self` is after `earlier`, negative when before.
    /// Use for computing durations between two timestamps without assuming ordering.
    pub fn elapsed_since(self, earlier: NanoTimestamp) -> i64 {
        self.0 - earlier.0
    }

    /// Snaps this timestamp down to the nearest multiple of `period_nanos`.
    ///
    /// For example, rounding `ts=1_500_000_000` down to `period_nanos=1_000_000_000`
    /// yields `1_000_000_000`. Useful for bar-boundary calculations.
    ///
    /// Returns `self` unchanged when `period_nanos == 0`.
    pub fn round_down_to(&self, period_nanos: i64) -> NanoTimestamp {
        if period_nanos == 0 {
            return *self;
        }
        NanoTimestamp(self.0 - self.0.rem_euclid(period_nanos))
    }
}

/// `NanoTimestamp + i64` shifts the timestamp forward by `nanos` nanoseconds.
impl std::ops::Add<i64> for NanoTimestamp {
    type Output = NanoTimestamp;
    fn add(self, rhs: i64) -> NanoTimestamp {
        NanoTimestamp(self.0 + rhs)
    }
}

/// `NanoTimestamp - i64` shifts the timestamp backward by `nanos` nanoseconds.
impl std::ops::Sub<i64> for NanoTimestamp {
    type Output = NanoTimestamp;
    fn sub(self, rhs: i64) -> NanoTimestamp {
        NanoTimestamp(self.0 - rhs)
    }
}

/// `NanoTimestamp - NanoTimestamp` returns the signed nanosecond difference.
impl std::ops::Sub<NanoTimestamp> for NanoTimestamp {
    type Output = i64;
    fn sub(self, rhs: NanoTimestamp) -> i64 {
        self.0 - rhs.0
    }
}

impl std::fmt::Display for NanoTimestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    // --- Symbol ---

    #[test]
    fn test_symbol_new_valid_ok() {
        let sym = Symbol::new("AAPL").unwrap();
        assert_eq!(sym.as_str(), "AAPL");
    }

    #[test]
    fn test_symbol_new_empty_fails() {
        let result = Symbol::new("");
        assert!(matches!(result, Err(FinError::InvalidSymbol(_))));
    }

    #[test]
    fn test_symbol_new_whitespace_fails() {
        let result = Symbol::new("AA PL");
        assert!(matches!(result, Err(FinError::InvalidSymbol(_))));
    }

    #[test]
    fn test_symbol_new_leading_whitespace_fails() {
        let result = Symbol::new(" AAPL");
        assert!(matches!(result, Err(FinError::InvalidSymbol(_))));
    }

    #[test]
    fn test_symbol_display() {
        let sym = Symbol::new("TSLA").unwrap();
        assert_eq!(format!("{sym}"), "TSLA");
    }

    #[test]
    fn test_symbol_clone_equality() {
        let a = Symbol::new("BTC").unwrap();
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn test_symbol_arc_clone_is_cheap() {
        let a = Symbol::new("ETH").unwrap();
        let b = a.clone();
        assert_eq!(a.as_str().as_ptr(), b.as_str().as_ptr());
    }

    // --- Price ---

    #[test]
    fn test_price_new_positive_ok() {
        let p = Price::new(dec!(100.5)).unwrap();
        assert_eq!(p.value(), dec!(100.5));
    }

    #[test]
    fn test_price_new_zero_fails() {
        let result = Price::new(dec!(0));
        assert!(matches!(result, Err(FinError::InvalidPrice(_))));
    }

    #[test]
    fn test_price_new_negative_fails() {
        let result = Price::new(dec!(-1));
        assert!(matches!(result, Err(FinError::InvalidPrice(_))));
    }

    #[test]
    fn test_price_ordering() {
        let p1 = Price::new(dec!(1)).unwrap();
        let p2 = Price::new(dec!(2)).unwrap();
        assert!(p1 < p2);
    }

    #[test]
    fn test_price_add() {
        let a = Price::new(dec!(10)).unwrap();
        let b = Price::new(dec!(5)).unwrap();
        assert_eq!(a + b, dec!(15));
    }

    #[test]
    fn test_price_sub() {
        let a = Price::new(dec!(10)).unwrap();
        let b = Price::new(dec!(3)).unwrap();
        assert_eq!(a - b, dec!(7));
    }

    #[test]
    fn test_price_mul_quantity() {
        let p = Price::new(dec!(10)).unwrap();
        let q = Quantity::new(dec!(5)).unwrap();
        assert_eq!(p * q, dec!(50));
    }

    #[test]
    fn test_price_mul_decimal_valid() {
        let p = Price::new(dec!(10)).unwrap();
        assert_eq!((p * dec!(2)).unwrap().value(), dec!(20));
    }

    #[test]
    fn test_price_mul_decimal_zero_returns_none() {
        let p = Price::new(dec!(10)).unwrap();
        assert!((p * dec!(0)).is_none());
    }

    // --- Quantity ---

    #[test]
    fn test_quantity_new_zero_ok() {
        let q = Quantity::new(dec!(0)).unwrap();
        assert_eq!(q.value(), dec!(0));
    }

    #[test]
    fn test_quantity_new_positive_ok() {
        let q = Quantity::new(dec!(5.5)).unwrap();
        assert_eq!(q.value(), dec!(5.5));
    }

    #[test]
    fn test_quantity_new_negative_fails() {
        let result = Quantity::new(dec!(-0.01));
        assert!(matches!(result, Err(FinError::InvalidQuantity(_))));
    }

    #[test]
    fn test_quantity_zero_constructor() {
        let q = Quantity::zero();
        assert_eq!(q.value(), Decimal::ZERO);
    }

    #[test]
    fn test_quantity_add() {
        let a = Quantity::new(dec!(3)).unwrap();
        let b = Quantity::new(dec!(4)).unwrap();
        assert_eq!((a + b).value(), dec!(7));
    }

    #[test]
    fn test_quantity_sub_positive() {
        let a = Quantity::new(dec!(10)).unwrap();
        let b = Quantity::new(dec!(3)).unwrap();
        assert_eq!(a - b, dec!(7));
    }

    #[test]
    fn test_quantity_sub_negative() {
        let a = Quantity::new(dec!(3)).unwrap();
        let b = Quantity::new(dec!(10)).unwrap();
        assert_eq!(a - b, dec!(-7));
    }

    #[test]
    fn test_quantity_mul_decimal() {
        let q = Quantity::new(dec!(5)).unwrap();
        assert_eq!(q * dec!(3), dec!(15));
    }

    #[test]
    fn test_quantity_is_zero() {
        assert!(Quantity::zero().is_zero());
        assert!(!Quantity::new(dec!(1)).unwrap().is_zero());
    }

    // --- Side ---

    #[test]
    fn test_side_display_bid() {
        assert_eq!(format!("{}", Side::Bid), "Bid");
    }

    #[test]
    fn test_side_display_ask() {
        assert_eq!(format!("{}", Side::Ask), "Ask");
    }

    // --- NanoTimestamp ---

    #[test]
    fn test_nano_timestamp_now_positive() {
        let ts = NanoTimestamp::now();
        assert!(ts.nanos() > 0);
    }

    #[test]
    fn test_nano_timestamp_ordering() {
        let ts1 = NanoTimestamp::new(1_000_000_000);
        let ts2 = NanoTimestamp::new(2_000_000_000);
        assert!(ts1 < ts2);
    }

    #[test]
    fn test_nano_timestamp_to_datetime_epoch() {
        let ts = NanoTimestamp::new(0);
        let dt = ts.to_datetime();
        assert_eq!(dt.timestamp(), 0);
    }

    #[test]
    fn test_nano_timestamp_to_datetime_roundtrip() {
        let ts = NanoTimestamp::new(1_700_000_000_000_000_000_i64);
        let dt = ts.to_datetime();
        assert_eq!(
            dt.timestamp_nanos_opt().unwrap_or(0),
            1_700_000_000_000_000_000_i64
        );
    }

    #[test]
    fn test_nano_timestamp_nanos_roundtrip() {
        let ts = NanoTimestamp::new(42_000_000);
        assert_eq!(ts.nanos(), 42_000_000);
    }

    #[test]
    fn test_nano_timestamp_duration_since_positive() {
        let a = NanoTimestamp::new(1_000);
        let b = NanoTimestamp::new(600);
        assert_eq!(a.duration_since(b), 400);
    }

    #[test]
    fn test_nano_timestamp_duration_since_negative() {
        let a = NanoTimestamp::new(500);
        let b = NanoTimestamp::new(1_000);
        assert_eq!(a.duration_since(b), -500);
    }

    #[test]
    fn test_symbol_len() {
        let sym = Symbol::new("AAPL").unwrap();
        assert_eq!(sym.len(), 4);
    }

    #[test]
    fn test_symbol_is_empty_always_false() {
        let sym = Symbol::new("X").unwrap();
        assert!(!sym.is_empty());
    }

    #[test]
    fn test_symbol_try_from_string_valid() {
        let sym = Symbol::try_from("AAPL".to_owned()).unwrap();
        assert_eq!(sym.as_str(), "AAPL");
    }

    #[test]
    fn test_symbol_try_from_str_valid() {
        let sym = Symbol::try_from("ETH").unwrap();
        assert_eq!(sym.as_str(), "ETH");
    }

    #[test]
    fn test_symbol_try_from_empty_fails() {
        assert!(Symbol::try_from("").is_err());
    }

    #[test]
    fn test_symbol_try_from_whitespace_fails() {
        assert!(Symbol::try_from("BTC USD").is_err());
    }

    #[test]
    fn test_nano_timestamp_from_datetime_roundtrip() {
        let original = NanoTimestamp::new(1_700_000_000_000_000_000_i64);
        let dt = original.to_datetime();
        let recovered = NanoTimestamp::from_datetime(dt);
        assert_eq!(recovered.nanos(), original.nanos());
    }

    #[test]
    fn test_nano_timestamp_from_datetime_epoch() {
        use chrono::Utc;
        let epoch = Utc.timestamp_opt(0, 0).single().unwrap();
        let ts = NanoTimestamp::from_datetime(epoch);
        assert_eq!(ts.nanos(), 0);
    }

    #[test]
    fn test_price_to_f64() {
        let p = Price::new(dec!(123.45)).unwrap();
        let f = p.to_f64();
        assert!((f - 123.45_f64).abs() < 1e-6);
    }

    #[test]
    fn test_quantity_to_f64() {
        let q = Quantity::new(dec!(42)).unwrap();
        assert!((q.to_f64() - 42.0_f64).abs() < 1e-10);
    }

    #[test]
    fn test_price_from_f64_valid() {
        let p = Price::from_f64(42.5).unwrap();
        assert!((p.to_f64() - 42.5).abs() < 1e-6);
    }

    #[test]
    fn test_price_from_f64_zero_returns_none() {
        assert!(Price::from_f64(0.0).is_none());
    }

    #[test]
    fn test_price_from_f64_negative_returns_none() {
        assert!(Price::from_f64(-1.0).is_none());
    }

    #[test]
    fn test_quantity_from_f64_valid() {
        let q = Quantity::from_f64(10.0).unwrap();
        assert!((q.to_f64() - 10.0).abs() < 1e-10);
    }

    #[test]
    fn test_quantity_from_f64_zero_valid() {
        let q = Quantity::from_f64(0.0).unwrap();
        assert!(q.is_zero());
    }

    #[test]
    fn test_quantity_from_f64_negative_returns_none() {
        assert!(Quantity::from_f64(-1.0).is_none());
    }

    #[test]
    fn test_nano_timestamp_add_millis() {
        let ts = NanoTimestamp::new(0);
        assert_eq!(ts.add_millis(1).nanos(), 1_000_000);
    }

    #[test]
    fn test_nano_timestamp_add_seconds() {
        let ts = NanoTimestamp::new(0);
        assert_eq!(ts.add_seconds(2).nanos(), 2_000_000_000);
    }

    #[test]
    fn test_nano_timestamp_is_before_after() {
        let a = NanoTimestamp::new(1_000);
        let b = NanoTimestamp::new(2_000);
        assert!(a.is_before(b));
        assert!(b.is_after(a));
        assert!(!a.is_after(b));
        assert!(!b.is_before(a));
    }

    #[test]
    fn test_nano_timestamp_from_secs_roundtrip() {
        let ts = NanoTimestamp::from_secs(1_700_000_000);
        assert_eq!(ts.to_secs(), 1_700_000_000);
    }

    #[test]
    fn test_nano_timestamp_from_secs_truncates_sub_second() {
        let ts = NanoTimestamp::new(1_700_000_000_999_999_999);
        assert_eq!(ts.to_secs(), 1_700_000_000);
    }

    #[test]
    fn test_symbol_ord_lexicographic() {
        let a = Symbol::new("AAPL").unwrap();
        let b = Symbol::new("MSFT").unwrap();
        let c = Symbol::new("AAPL").unwrap();
        assert!(a < b);
        assert!(b > a);
        assert_eq!(a.cmp(&c), std::cmp::Ordering::Equal);
    }

    #[test]
    fn test_symbol_ord_usable_in_btreemap() {
        use std::collections::BTreeMap;
        let mut m: BTreeMap<Symbol, i32> = BTreeMap::new();
        m.insert(Symbol::new("Z").unwrap(), 3);
        m.insert(Symbol::new("A").unwrap(), 1);
        m.insert(Symbol::new("M").unwrap(), 2);
        let keys: Vec<_> = m.keys().map(|s| s.as_str()).collect();
        assert_eq!(keys, ["A", "M", "Z"]);
    }

    #[test]
    fn test_price_pct_change_positive() {
        let p1 = Price::new(dec!(100)).unwrap();
        let p2 = Price::new(dec!(110)).unwrap();
        assert_eq!(p1.pct_change_to(p2), dec!(10));
    }

    #[test]
    fn test_price_pct_change_negative() {
        let p1 = Price::new(dec!(100)).unwrap();
        let p2 = Price::new(dec!(90)).unwrap();
        assert_eq!(p1.pct_change_to(p2), dec!(-10));
    }

    #[test]
    fn test_price_pct_change_zero() {
        let p = Price::new(dec!(100)).unwrap();
        assert_eq!(p.pct_change_to(p), dec!(0));
    }

    #[test]
    fn test_nano_timestamp_elapsed_is_non_negative_for_past() {
        let past = NanoTimestamp::new(0); // epoch — definitely in the past
        assert!(past.elapsed() > 0);
    }

    #[test]
    fn test_price_checked_mul_some() {
        let p = Price::new(dec!(100)).unwrap();
        let q = Quantity::new(dec!(5)).unwrap();
        assert_eq!(p.checked_mul(q), Some(dec!(500)));
    }

    #[test]
    fn test_price_checked_mul_with_zero_qty() {
        let p = Price::new(dec!(100)).unwrap();
        let q = Quantity::zero();
        assert_eq!(p.checked_mul(q), Some(dec!(0)));
    }

    #[test]
    fn test_quantity_checked_add() {
        let a = Quantity::new(dec!(10)).unwrap();
        let b = Quantity::new(dec!(5)).unwrap();
        assert_eq!(a.checked_add(b).map(|q| q.value()), Some(dec!(15)));
    }

    #[test]
    fn test_nano_timestamp_min_less_than_max() {
        assert!(NanoTimestamp::MIN < NanoTimestamp::MAX);
        assert!(NanoTimestamp::MIN < NanoTimestamp::new(0));
        assert!(NanoTimestamp::new(0) < NanoTimestamp::MAX);
    }

    #[test]
    fn test_price_midpoint() {
        let bid = Price::new(dec!(99)).unwrap();
        let ask = Price::new(dec!(101)).unwrap();
        assert_eq!(Price::midpoint(bid, ask), dec!(100));
    }

    #[test]
    fn test_price_midpoint_same_price() {
        let p = Price::new(dec!(100)).unwrap();
        assert_eq!(Price::midpoint(p, p), dec!(100));
    }

    #[test]
    fn test_price_mid_method() {
        let bid = Price::new(dec!(100)).unwrap();
        let ask = Price::new(dec!(102)).unwrap();
        let mid = bid.mid(ask);
        assert_eq!(mid.value(), dec!(101));
    }

    #[test]
    fn test_price_mid_method_same_price() {
        let p = Price::new(dec!(100)).unwrap();
        assert_eq!(p.mid(p).value(), dec!(100));
    }

    #[test]
    fn test_price_abs_diff_positive() {
        let a = Price::new(dec!(105)).unwrap();
        let b = Price::new(dec!(100)).unwrap();
        assert_eq!(a.abs_diff(b), dec!(5));
        assert_eq!(b.abs_diff(a), dec!(5));
    }

    #[test]
    fn test_price_abs_diff_same() {
        let p = Price::new(dec!(100)).unwrap();
        assert_eq!(p.abs_diff(p), dec!(0));
    }

    #[test]
    fn test_quantity_checked_sub_valid() {
        let a = Quantity::new(dec!(10)).unwrap();
        let b = Quantity::new(dec!(3)).unwrap();
        assert_eq!(a.checked_sub(b).unwrap().value(), dec!(7));
    }

    #[test]
    fn test_quantity_checked_sub_exact_zero() {
        let a = Quantity::new(dec!(5)).unwrap();
        let b = Quantity::new(dec!(5)).unwrap();
        assert_eq!(a.checked_sub(b).unwrap().value(), dec!(0));
    }

    #[test]
    fn test_quantity_checked_sub_negative_returns_none() {
        let a = Quantity::new(dec!(3)).unwrap();
        let b = Quantity::new(dec!(5)).unwrap();
        assert!(a.checked_sub(b).is_none());
    }

    #[test]
    fn test_nano_timestamp_min_returns_earlier() {
        let t1 = NanoTimestamp::new(100);
        let t2 = NanoTimestamp::new(200);
        assert_eq!(t1.min(t2), t1);
        assert_eq!(t2.min(t1), t1);
    }

    #[test]
    fn test_nano_timestamp_max_returns_later() {
        let t1 = NanoTimestamp::new(100);
        let t2 = NanoTimestamp::new(200);
        assert_eq!(t1.max(t2), t2);
        assert_eq!(t2.max(t1), t2);
    }

    #[test]
    fn test_nano_timestamp_min_max_same() {
        let t = NanoTimestamp::new(500);
        assert_eq!(t.min(t), t);
        assert_eq!(t.max(t), t);
    }

    #[test]
    fn test_side_opposite_bid() {
        assert_eq!(Side::Bid.opposite(), Side::Ask);
    }

    #[test]
    fn test_side_opposite_ask() {
        assert_eq!(Side::Ask.opposite(), Side::Bid);
    }

    #[test]
    fn test_side_opposite_involution() {
        assert_eq!(Side::Bid.opposite().opposite(), Side::Bid);
    }

    #[test]
    fn test_price_checked_add_valid() {
        let a = Price::new(dec!(100)).unwrap();
        let b = Price::new(dec!(50)).unwrap();
        assert_eq!(a.checked_add(b).unwrap().value(), dec!(150));
    }

    #[test]
    fn test_price_checked_add_result_validated() {
        // Sum of two valid prices is always positive → always Some
        let a = Price::new(dec!(1)).unwrap();
        let b = Price::new(dec!(2)).unwrap();
        assert!(a.checked_add(b).is_some());
    }

    #[test]
    fn test_price_lerp_midpoint() {
        let a = Price::new(dec!(100)).unwrap();
        let b = Price::new(dec!(200)).unwrap();
        let mid = a.lerp(b, dec!(0.5)).unwrap();
        assert_eq!(mid.value(), dec!(150));
    }

    #[test]
    fn test_price_lerp_at_zero_returns_self() {
        let a = Price::new(dec!(100)).unwrap();
        let b = Price::new(dec!(200)).unwrap();
        assert_eq!(a.lerp(b, Decimal::ZERO).unwrap().value(), dec!(100));
    }

    #[test]
    fn test_price_lerp_at_one_returns_other() {
        let a = Price::new(dec!(100)).unwrap();
        let b = Price::new(dec!(200)).unwrap();
        assert_eq!(a.lerp(b, Decimal::ONE).unwrap().value(), dec!(200));
    }

    #[test]
    fn test_price_lerp_out_of_range_returns_none() {
        let a = Price::new(dec!(100)).unwrap();
        let b = Price::new(dec!(200)).unwrap();
        assert!(a.lerp(b, dec!(1.5)).is_none());
        assert!(a.lerp(b, dec!(-0.1)).is_none());
    }

    #[test]
    fn test_quantity_scale_half() {
        let q = Quantity::new(dec!(100)).unwrap();
        let result = q.scale(dec!(0.5)).unwrap();
        assert_eq!(result.value(), dec!(50));
    }

    #[test]
    fn test_quantity_scale_zero_factor() {
        let q = Quantity::new(dec!(100)).unwrap();
        let result = q.scale(Decimal::ZERO).unwrap();
        assert_eq!(result.value(), dec!(0));
    }

    #[test]
    fn test_quantity_scale_negative_factor_returns_none() {
        let q = Quantity::new(dec!(100)).unwrap();
        assert!(q.scale(dec!(-1)).is_none());
    }

    #[test]
    fn test_nano_timestamp_elapsed_since_positive() {
        let earlier = NanoTimestamp::new(1000);
        let later = NanoTimestamp::new(3000);
        assert_eq!(later.elapsed_since(earlier), 2000);
    }

    #[test]
    fn test_nano_timestamp_elapsed_since_negative() {
        let earlier = NanoTimestamp::new(1000);
        let later = NanoTimestamp::new(3000);
        // reversed order gives negative result
        assert_eq!(earlier.elapsed_since(later), -2000);
    }

    #[test]
    fn test_nano_timestamp_elapsed_since_same_is_zero() {
        let ts = NanoTimestamp::new(5000);
        assert_eq!(ts.elapsed_since(ts), 0);
    }
}
