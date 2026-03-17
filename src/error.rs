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
    SequenceMismatch { expected: u64, got: u64 },

    /// Not enough resting liquidity to fill the requested quantity.
    #[error("No liquidity available for requested quantity {0}")]
    InsufficientLiquidity(Decimal),

    /// OHLCV bar failed internal invariant check (high >= low, etc.).
    #[error("OHLCV bar invariant violated: {0}")]
    BarInvariant(String),

    /// A signal has not accumulated enough bars to produce a value.
    #[error("Signal '{name}' not ready (requires {required} periods, have {have})")]
    SignalNotReady {
        name: String,
        required: usize,
        have: usize,
    },

    /// Position lookup failed for the given symbol.
    #[error("Position not found for symbol '{0}'")]
    PositionNotFound(String),

    /// Ledger cash balance insufficient to cover the fill cost.
    #[error("Insufficient funds: need {need}, have {have}")]
    InsufficientFunds {
        need: Decimal,
        have: Decimal,
    },

    /// Timeframe duration was zero or negative.
    #[error("Timeframe duration must be positive")]
    InvalidTimeframe,

    /// CSV or text parse failure.
    #[error("CSV parse error: {0}")]
    CsvParse(String),

    /// A Decimal arithmetic operation overflowed.
    #[error("Arithmetic overflow in financial calculation")]
    ArithmeticOverflow,

    /// Order book ended up with an inverted spread after a delta was applied.
    #[error("Inverted spread: best_bid {best_bid} >= best_ask {best_ask}")]
    InvertedSpread { best_bid: Decimal, best_ask: Decimal },
}
