//! Error types for the fin-primitives crate.
//!
//! All errors are named, typed, and propagatable via `thiserror`.
//! Every variant has at least one test that triggers it.

use rust_decimal::Decimal;

/// All errors that can occur in fin-primitives operations.
#[derive(Debug, thiserror::Error)]
pub enum FinError {
    /// Symbol string was empty or contained whitespace.
    #[error("Symbol '{0}' is invalid (empty or contains whitespace)")]
    InvalidSymbol(String),

    /// Price value was zero or negative.
    #[error("Price must be positive, got {0}")]
    InvalidPrice(Decimal),

    /// Quantity value was negative.
    #[error("Quantity must be non-negative, got {0}")]
    InvalidQuantity(Decimal),

    /// Order book delta arrived out of sequence.
    #[error("Order book sequence mismatch: expected {expected}, got {got}")]
    SequenceMismatch {
        /// The next sequence number the book expected.
        expected: u64,
        /// The sequence number that was actually received.
        got: u64,
    },

    /// Not enough resting liquidity to fill the requested quantity.
    #[error("No liquidity available for requested quantity {0}")]
    InsufficientLiquidity(Decimal),

    /// OHLCV bar failed internal invariant check (high >= low, etc.).
    #[error("OHLCV bar invariant violated: {0}")]
    BarInvariant(String),

    /// A signal has not accumulated enough bars to produce a value.
    #[error("Signal '{name}' not ready (requires {required} periods, have {have})")]
    SignalNotReady {
        /// Name of the signal that is not ready.
        name: String,
        /// Number of bars required before the signal produces a value.
        required: usize,
        /// Number of bars seen so far.
        have: usize,
    },

    /// Position lookup failed for the given symbol.
    #[error("Position not found for symbol '{0}'")]
    PositionNotFound(String),

    /// Ledger cash balance insufficient to cover the fill cost.
    #[error("Insufficient funds: need {need}, have {have}")]
    InsufficientFunds {
        /// Amount of cash required for the fill (cost + commission).
        need: Decimal,
        /// Current cash balance in the ledger.
        have: Decimal,
    },

    /// Timeframe duration was zero or negative.
    #[error("Timeframe duration must be positive")]
    InvalidTimeframe,

    /// A Decimal arithmetic operation overflowed.
    #[error("Arithmetic overflow in financial calculation")]
    ArithmeticOverflow,

    /// Order book ended up with an inverted spread after a delta was applied.
    #[error("Inverted spread: best_bid {best_bid} >= best_ask {best_ask}")]
    InvertedSpread {
        /// Best bid price at the time the spread inversion was detected.
        best_bid: Decimal,
        /// Best ask price at the time the spread inversion was detected.
        best_ask: Decimal,
    },

    /// Indicator or aggregator period was zero; must be at least 1.
    #[error("Period must be at least 1, got {0}")]
    InvalidPeriod(usize),

    /// General-purpose validation error for invalid inputs.
    #[error("Invalid input: {0}")]
    InvalidInput(String),
}

impl FinError {
    /// Returns `true` if this error is a period validation error.
    pub fn is_period_error(&self) -> bool {
        matches!(self, FinError::InvalidPeriod(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_period_error_true_for_invalid_period() {
        let e = FinError::InvalidPeriod(0);
        assert!(e.is_period_error());
    }

    #[test]
    fn test_is_period_error_false_for_other_errors() {
        let e = FinError::InvalidSymbol("".to_owned());
        assert!(!e.is_period_error());
    }

    #[test]
    fn test_invalid_input_error_message() {
        let e = FinError::InvalidInput("bad value".to_owned());
        assert!(e.to_string().contains("bad value"));
    }
}
