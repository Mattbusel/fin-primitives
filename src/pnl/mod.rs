//! # Module: pnl
//!
//! ## Responsibility
//! Streaming P&L attribution: tracks realized and unrealized P&L per trade and
//! decomposes it into direction alpha, timing alpha, slippage cost, and fee cost.
//!
//! ## Guarantees
//! - All arithmetic uses `rust_decimal::Decimal`; no floating-point drift
//! - `PnlAttributor::close_trade` emits a `PnlEvent` with fully decomposed components
//! - Slippage cost is always non-negative (it is a cost, not a gain)
//! - Fee cost is always non-negative
//!
//! ## NOT Responsible For
//! - Order routing or execution
//! - Position risk checks (see `risk` module)

use crate::error::FinError;
use crate::types::{NanoTimestamp, Price, Quantity, Side, Symbol};
use rust_decimal::Decimal;

/// An open trade leg awaiting closure.
#[derive(Debug, Clone)]
pub struct OpenTrade {
    /// Instrument traded.
    pub symbol: Symbol,
    /// Direction of the opening fill.
    pub side: Side,
    /// Size of the opening fill.
    pub quantity: Quantity,
    /// Execution price of the opening fill.
    pub entry_price: Price,
    /// Theoretical fair value at entry time (e.g. mid-price).
    ///
    /// Used to compute slippage: `(entry_price - fair_value).abs() * qty`.
    pub entry_fair_value: Price,
    /// Commission charged on entry.
    pub entry_fee: Decimal,
    /// When the trade was opened.
    pub opened_at: NanoTimestamp,
}

impl OpenTrade {
    /// Creates a new `OpenTrade`.
    pub fn new(
        symbol: Symbol,
        side: Side,
        quantity: Quantity,
        entry_price: Price,
        entry_fair_value: Price,
        entry_fee: Decimal,
        opened_at: NanoTimestamp,
    ) -> Self {
        Self {
            symbol,
            side,
            quantity,
            entry_price,
            entry_fair_value,
            entry_fee,
            opened_at,
        }
    }
}

/// Decomposed P&L event emitted when a trade is closed.
///
/// All components are signed from the perspective of the trader:
/// positive = profit, negative = loss.
///
/// ## Decomposition identity
/// `total_pnl ≈ direction_alpha + timing_alpha - slippage_cost - fee_cost`
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PnlEvent {
    /// Instrument.
    pub symbol: Symbol,
    /// Side of the opening leg.
    pub side: Side,
    /// Size closed.
    pub quantity: Decimal,
    /// Price at which the trade was entered.
    pub entry_price: Decimal,
    /// Price at which the trade was exited.
    pub exit_price: Decimal,
    /// When the trade opened.
    pub opened_at: NanoTimestamp,
    /// When the trade closed.
    pub closed_at: NanoTimestamp,

    /// **Direction alpha**: profit from the raw directional move,
    /// measured at fair values (no slippage, no fees).
    ///
    /// `long: (exit_fair - entry_fair) * qty`
    /// `short: (entry_fair - exit_fair) * qty`
    pub direction_alpha: Decimal,

    /// **Timing alpha**: additional P&L gained from entering/exiting
    /// at a favourable time relative to a passive mid strategy.
    ///
    /// Currently zero-valued (reserved for future bar-level attribution).
    pub timing_alpha: Decimal,

    /// **Slippage cost**: execution cost from crossing the spread
    /// (difference between execution price and fair value).
    ///
    /// Always `>= 0`.
    pub slippage_cost: Decimal,

    /// **Fee cost**: total commissions paid on both legs.
    ///
    /// Always `>= 0`.
    pub fee_cost: Decimal,

    /// Net realized P&L: raw directional P&L minus costs.
    ///
    /// `= (exit_price - entry_price) * qty` (for long)
    /// minus total fees.
    pub realized_pnl: Decimal,
}

/// Streaming P&L attributor.
///
/// Open a trade with [`PnlAttributor::open_trade`], close it with
/// [`PnlAttributor::close_trade`] and receive a [`PnlEvent`] with
/// fully decomposed attribution.
///
/// # Example
/// ```rust
/// use fin_primitives::pnl::{PnlAttributor, OpenTrade};
/// use fin_primitives::types::{Symbol, Side, Price, Quantity, NanoTimestamp};
/// use rust_decimal_macros::dec;
///
/// let mut attr = PnlAttributor::new();
/// let sym = Symbol::new("AAPL").unwrap();
/// let ts = NanoTimestamp::new(1_000_000_000);
/// let trade = OpenTrade::new(
///     sym.clone(), Side::Bid,
///     Quantity::new(dec!(10)).unwrap(),
///     Price::new(dec!(100)).unwrap(),
///     Price::new(dec!(100.05)).unwrap(),  // fair value slightly above
///     dec!(0.10),
///     ts,
/// );
/// attr.open_trade("t1", trade);
/// let event = attr.close_trade(
///     "t1",
///     Price::new(dec!(105)).unwrap(),
///     Price::new(dec!(104.95)).unwrap(),
///     dec!(0.10),
///     NanoTimestamp::new(2_000_000_000),
/// ).unwrap();
/// assert!(event.realized_pnl > dec!(0));
/// ```
#[derive(Debug, Default)]
pub struct PnlAttributor {
    open_trades: std::collections::HashMap<String, OpenTrade>,
    /// Cumulative realized P&L across all closed trades.
    pub total_realized_pnl: Decimal,
    /// Count of closed trades.
    pub closed_trade_count: usize,
}

impl PnlAttributor {
    /// Creates an empty `PnlAttributor`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an open trade under `trade_id`.
    ///
    /// If a trade with the same id already exists it is silently replaced.
    pub fn open_trade(&mut self, trade_id: impl Into<String>, trade: OpenTrade) {
        self.open_trades.insert(trade_id.into(), trade);
    }

    /// Closes the trade identified by `trade_id` and returns a [`PnlEvent`].
    ///
    /// # Errors
    /// - [`FinError::InvalidInput`] if `trade_id` is not found.
    /// - [`FinError::ArithmeticOverflow`] on checked-arithmetic failure (extremely unlikely
    ///   with normal price magnitudes).
    pub fn close_trade(
        &mut self,
        trade_id: &str,
        exit_price: Price,
        exit_fair_value: Price,
        exit_fee: Decimal,
        closed_at: NanoTimestamp,
    ) -> Result<PnlEvent, FinError> {
        let trade = self
            .open_trades
            .remove(trade_id)
            .ok_or_else(|| FinError::InvalidInput(format!("trade '{trade_id}' not found")))?;

        let qty = trade.quantity.value();
        let entry_p = trade.entry_price.value();
        let exit_p = exit_price.value();
        let entry_fair = trade.entry_fair_value.value();
        let exit_fair = exit_fair_value.value();

        // Raw realized P&L (signed, before fees)
        let raw_pnl = match trade.side {
            Side::Bid => (exit_p - entry_p) * qty,
            Side::Ask => (entry_p - exit_p) * qty,
        };

        // Direction alpha: fair-value move in the direction of the trade
        let direction_alpha = match trade.side {
            Side::Bid => (exit_fair - entry_fair) * qty,
            Side::Ask => (entry_fair - exit_fair) * qty,
        };

        // Slippage: cost of crossing the spread on entry and exit
        // entry slippage: long paid above fair, short sold below fair
        let entry_slip = match trade.side {
            Side::Bid => (entry_p - entry_fair) * qty,
            Side::Ask => (entry_fair - entry_p) * qty,
        };
        let exit_slip = match trade.side {
            Side::Bid => (exit_fair - exit_p) * qty,
            Side::Ask => (exit_p - exit_fair) * qty,
        };
        // Slippage cost is the total execution disadvantage vs fair value
        let slippage_cost = (entry_slip + exit_slip).max(Decimal::ZERO);

        let fee_cost = (trade.entry_fee + exit_fee).max(Decimal::ZERO);
        let realized_pnl = raw_pnl - fee_cost;

        // Timing alpha is currently zero; reserved for bar-level decomposition
        let timing_alpha = Decimal::ZERO;

        self.total_realized_pnl += realized_pnl;
        self.closed_trade_count += 1;

        Ok(PnlEvent {
            symbol: trade.symbol,
            side: trade.side,
            quantity: qty,
            entry_price: entry_p,
            exit_price: exit_p,
            opened_at: trade.opened_at,
            closed_at,
            direction_alpha,
            timing_alpha,
            slippage_cost,
            fee_cost,
            realized_pnl,
        })
    }

    /// Returns unrealized P&L for a trade given the current fair value.
    ///
    /// Returns `None` if `trade_id` is not found.
    pub fn unrealized_pnl(&self, trade_id: &str, current_price: Decimal) -> Option<Decimal> {
        let trade = self.open_trades.get(trade_id)?;
        let qty = trade.quantity.value();
        let entry_p = trade.entry_price.value();
        let upnl = match trade.side {
            Side::Bid => (current_price - entry_p) * qty,
            Side::Ask => (entry_p - current_price) * qty,
        };
        Some(upnl - trade.entry_fee)
    }

    /// Returns the number of currently open trades.
    pub fn open_trade_count(&self) -> usize {
        self.open_trades.len()
    }

    /// Returns `true` if `trade_id` is an open trade.
    pub fn has_open_trade(&self, trade_id: &str) -> bool {
        self.open_trades.contains_key(trade_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn sym() -> Symbol {
        Symbol::new("AAPL").unwrap()
    }
    fn ts(n: i64) -> NanoTimestamp {
        NanoTimestamp::new(n)
    }

    #[test]
    fn test_long_profitable_trade() {
        let mut attr = PnlAttributor::new();
        let trade = OpenTrade::new(
            sym(),
            Side::Bid,
            Quantity::new(dec!(10)).unwrap(),
            Price::new(dec!(100)).unwrap(),
            Price::new(dec!(100)).unwrap(),
            dec!(0.50),
            ts(1000),
        );
        attr.open_trade("t1", trade);
        assert!(attr.has_open_trade("t1"));
        let event = attr
            .close_trade(
                "t1",
                Price::new(dec!(110)).unwrap(),
                Price::new(dec!(110)).unwrap(),
                dec!(0.50),
                ts(2000),
            )
            .unwrap();
        // raw = (110-100)*10 = 100, fee = 1.00, net = 99
        assert_eq!(event.realized_pnl, dec!(99));
        assert_eq!(event.fee_cost, dec!(1.00));
        assert_eq!(event.direction_alpha, dec!(100));
        assert!(!attr.has_open_trade("t1"));
        assert_eq!(attr.closed_trade_count, 1);
    }

    #[test]
    fn test_short_profitable_trade() {
        let mut attr = PnlAttributor::new();
        let trade = OpenTrade::new(
            sym(),
            Side::Ask,
            Quantity::new(dec!(5)).unwrap(),
            Price::new(dec!(200)).unwrap(),
            Price::new(dec!(200)).unwrap(),
            dec!(0.25),
            ts(1000),
        );
        attr.open_trade("t2", trade);
        let event = attr
            .close_trade(
                "t2",
                Price::new(dec!(190)).unwrap(),
                Price::new(dec!(190)).unwrap(),
                dec!(0.25),
                ts(3000),
            )
            .unwrap();
        // raw = (200-190)*5 = 50, fees=0.50, net=49.50
        assert_eq!(event.realized_pnl, dec!(49.50));
        assert_eq!(event.direction_alpha, dec!(50));
    }

    #[test]
    fn test_slippage_computed() {
        let mut attr = PnlAttributor::new();
        let trade = OpenTrade::new(
            sym(),
            Side::Bid,
            Quantity::new(dec!(1)).unwrap(),
            Price::new(dec!(100.10)).unwrap(), // paid 0.10 above fair
            Price::new(dec!(100)).unwrap(),
            Decimal::ZERO,
            ts(1000),
        );
        attr.open_trade("t3", trade);
        let event = attr
            .close_trade(
                "t3",
                Price::new(dec!(105)).unwrap(),
                Price::new(dec!(105.05)).unwrap(), // exited 0.05 below fair
                Decimal::ZERO,
                ts(2000),
            )
            .unwrap();
        // entry slip = (100.10-100)*1 = 0.10
        // exit slip = (105.05-105)*1 = 0.05
        assert_eq!(event.slippage_cost, dec!(0.15));
    }

    #[test]
    fn test_unrealized_pnl() {
        let mut attr = PnlAttributor::new();
        let trade = OpenTrade::new(
            sym(),
            Side::Bid,
            Quantity::new(dec!(10)).unwrap(),
            Price::new(dec!(50)).unwrap(),
            Price::new(dec!(50)).unwrap(),
            dec!(1.00),
            ts(1000),
        );
        attr.open_trade("t4", trade);
        let upnl = attr.unrealized_pnl("t4", dec!(55)).unwrap();
        // (55-50)*10 - 1.00 = 49
        assert_eq!(upnl, dec!(49));
    }

    #[test]
    fn test_close_unknown_trade_errors() {
        let mut attr = PnlAttributor::new();
        let err = attr
            .close_trade(
                "nonexistent",
                Price::new(dec!(100)).unwrap(),
                Price::new(dec!(100)).unwrap(),
                Decimal::ZERO,
                ts(1000),
            )
            .unwrap_err();
        assert!(matches!(err, FinError::InvalidInput(_)));
    }
}
