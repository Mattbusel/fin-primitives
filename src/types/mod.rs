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

    /// Returns the signed nanosecond difference `self - other`.
    ///
    /// Positive when `self` is later than `other`, negative when earlier.
    pub fn duration_since(&self, other: NanoTimestamp) -> i64 {
        self.0 - other.0
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
}
