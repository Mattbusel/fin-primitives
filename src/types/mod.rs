//! # Module: types
//!
//! ## Responsibility
//! Provides the core validated newtype wrappers used throughout fin-primitives:
//! `Symbol`, `Price`, `Quantity`, `Side`, and `NanoTimestamp`.
//!
//! ## Guarantees
//! - `Symbol`: non-empty, no whitespace
//! - `Price`: strictly positive (`> 0`)
//! - `Quantity`: non-negative (`>= 0`)
//! - `NanoTimestamp`: nanosecond-resolution UTC epoch timestamp
//! - All types implement `Clone`, `Copy` (where applicable), `serde::{Serialize, Deserialize}`
//!
//! ## NOT Responsible For
//! - Currency conversion
//! - Tick size enforcement (exchange-specific)

use crate::error::FinError;
use chrono::{DateTime, TimeZone, Utc};
use rust_decimal::Decimal;

/// A validated ticker symbol: non-empty, contains no whitespace.
///
/// # Example
/// ```rust
/// use fin_primitives::types::Symbol;
/// let sym = Symbol::new("AAPL").unwrap();
/// assert_eq!(sym.as_str(), "AAPL");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Symbol(String);

impl Symbol {
    /// Construct a validated `Symbol`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidSymbol`] if the string is empty or contains whitespace.
    pub fn new(s: impl Into<String>) -> Result<Self, FinError> {
        let s = s.into();
        if s.is_empty() || s.chars().any(|c| c.is_whitespace()) {
            return Err(FinError::InvalidSymbol(s));
        }
        Ok(Self(s))
    }

    /// Returns the inner string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Symbol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
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

/// A non-negative quantity backed by [`Decimal`].
///
/// # Example
/// ```rust
/// use fin_primitives::types::Quantity;
/// use rust_decimal_macros::dec;
/// let q = Quantity::zero();
/// assert_eq!(q.value(), dec!(0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
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

    /// Returns the inner [`Decimal`] value.
    pub fn value(&self) -> Decimal {
        self.0
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

/// Exchange-epoch timestamp with nanosecond resolution.
///
/// Stores nanoseconds since the Unix epoch (UTC).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct NanoTimestamp(pub i64);

impl NanoTimestamp {
    /// Returns the current UTC time as a `NanoTimestamp`.
    ///
    /// Falls back to `0` if the system clock overflows nanosecond range (extremely unlikely).
    pub fn now() -> Self {
        Self(Utc::now().timestamp_nanos_opt().unwrap_or(0))
    }

    /// Converts this timestamp to a [`DateTime<Utc>`].
    pub fn to_datetime(&self) -> DateTime<Utc> {
        let secs = self.0 / 1_000_000_000;
        let nanos = (self.0 % 1_000_000_000) as u32;
        Utc.timestamp_opt(secs, nanos)
            .single()
            .unwrap_or_else(|| Utc.timestamp_opt(0, 0).single().unwrap_or(DateTime::<Utc>::MIN_UTC))
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

    // --- NanoTimestamp ---

    #[test]
    fn test_nano_timestamp_now_positive() {
        let ts = NanoTimestamp::now();
        assert!(ts.0 > 0);
    }

    #[test]
    fn test_nano_timestamp_ordering() {
        let ts1 = NanoTimestamp(1_000_000_000);
        let ts2 = NanoTimestamp(2_000_000_000);
        assert!(ts1 < ts2);
    }

    #[test]
    fn test_nano_timestamp_to_datetime_epoch() {
        let ts = NanoTimestamp(0);
        let dt = ts.to_datetime();
        assert_eq!(dt.timestamp(), 0);
    }

    #[test]
    fn test_nano_timestamp_to_datetime_roundtrip() {
        let ts = NanoTimestamp(1_700_000_000_000_000_000_i64);
        let dt = ts.to_datetime();
        assert_eq!(dt.timestamp_nanos_opt().unwrap_or(0), 1_700_000_000_000_000_000_i64);
    }
}
