//! # Module: position
//!
//! ## Responsibility
//! Tracks individual positions per symbol and a multi-position ledger with cash accounting.
//! Computes realized and unrealized P&L from fills.
//!
//! ## Guarantees
//! - `Position::apply_fill` returns realized `PnL` (non-zero only when reducing position)
//! - `PositionLedger::apply_fill` debits/credits cash correctly including commissions
//! - `Position::is_flat` is true iff `quantity == 0`
//!
//! ## NOT Responsible For
//! - Risk checks (see `risk` module)
//! - Order management

use crate::error::FinError;
use crate::types::{NanoTimestamp, Price, Quantity, Side, Symbol};
use rust_decimal::Decimal;
use std::collections::HashMap;

/// A single trade execution event.
#[derive(Debug, Clone)]
pub struct Fill {
    /// The instrument traded.
    pub symbol: Symbol,
    /// Whether this fill is a buy (Bid) or sell (Ask).
    pub side: Side,
    /// The number of units traded.
    pub quantity: Quantity,
    /// The execution price.
    pub price: Price,
    /// When the fill occurred.
    pub timestamp: NanoTimestamp,
    /// Commission charged.
    pub commission: Decimal,
}

/// A single-symbol position tracking quantity, average cost, and realized P&L.
#[derive(Debug, Clone)]
pub struct Position {
    /// The instrument.
    pub symbol: Symbol,
    /// Current net quantity (positive = long, negative = short, zero = flat).
    pub quantity: Decimal,
    /// Volume-weighted average cost of the current position.
    pub avg_cost: Decimal,
    /// Cumulative realized P&L for this position (net of commissions).
    pub realized_pnl: Decimal,
}

impl Position {
    /// Creates a new flat `Position` for `symbol`.
    pub fn new(symbol: Symbol) -> Self {
        Self {
            symbol,
            quantity: Decimal::ZERO,
            avg_cost: Decimal::ZERO,
            realized_pnl: Decimal::ZERO,
        }
    }

    /// Applies a fill, updating quantity, `avg_cost`, and `realized_pnl`.
    ///
    /// # Returns
    /// The realized P&L contributed by this fill (0 if position is increasing).
    ///
    /// # Errors
    /// Returns [`FinError::ArithmeticOverflow`] on checked arithmetic failure.
    pub fn apply_fill(&mut self, fill: &Fill) -> Result<Decimal, FinError> {
        let fill_qty = match fill.side {
            Side::Bid => fill.quantity.value(),
            Side::Ask => -fill.quantity.value(),
        };

        let realized = if self.quantity != Decimal::ZERO
            && (self.quantity > Decimal::ZERO) != (fill_qty > Decimal::ZERO)
        {
            let closed = fill_qty.abs().min(self.quantity.abs());
            if self.quantity > Decimal::ZERO {
                closed * (fill.price.value() - self.avg_cost)
            } else {
                closed * (self.avg_cost - fill.price.value())
            }
        } else {
            Decimal::ZERO
        };

        let new_qty = self.quantity + fill_qty;
        if new_qty == Decimal::ZERO {
            self.avg_cost = Decimal::ZERO;
        } else if (self.quantity >= Decimal::ZERO && fill_qty > Decimal::ZERO)
            || (self.quantity <= Decimal::ZERO && fill_qty < Decimal::ZERO)
        {
            let total_cost =
                self.avg_cost * self.quantity.abs() + fill.price.value() * fill_qty.abs();
            self.avg_cost = total_cost
                .checked_div(new_qty.abs())
                .ok_or(FinError::ArithmeticOverflow)?;
        } else if new_qty.abs() <= self.quantity.abs() {
            // Partial close: avg_cost unchanged.
        } else {
            // Position flipped.
            self.avg_cost = fill.price.value();
        }

        self.quantity = new_qty;
        let net_realized = realized - fill.commission;
        self.realized_pnl += net_realized;
        Ok(net_realized)
    }

    /// Returns unrealized P&L at `current_price`.
    pub fn unrealized_pnl(&self, current_price: Price) -> Decimal {
        self.quantity * (current_price.value() - self.avg_cost)
    }

    /// Returns the market value of this position at `current_price`.
    pub fn market_value(&self, current_price: Price) -> Decimal {
        self.quantity * current_price.value()
    }

    /// Returns `true` if the position is flat (zero quantity).
    pub fn is_flat(&self) -> bool {
        self.quantity == Decimal::ZERO
    }
}

/// A multi-symbol ledger tracking positions and a cash balance.
pub struct PositionLedger {
    positions: HashMap<Symbol, Position>,
    cash: Decimal,
}

impl PositionLedger {
    /// Creates a new `PositionLedger` with the given initial cash balance.
    pub fn new(initial_cash: Decimal) -> Self {
        Self {
            positions: HashMap::new(),
            cash: initial_cash,
        }
    }

    /// Applies a fill to the appropriate position and updates cash.
    ///
    /// # Errors
    /// Returns [`FinError::InsufficientFunds`] if a buy would require more cash than available.
    #[allow(clippy::needless_pass_by_value)]
    pub fn apply_fill(&mut self, fill: Fill) -> Result<(), FinError> {
        let cost = match fill.side {
            Side::Bid => -(fill.quantity.value() * fill.price.value() + fill.commission),
            Side::Ask => fill.quantity.value() * fill.price.value() - fill.commission,
        };
        if fill.side == Side::Bid && self.cash + cost < Decimal::ZERO {
            return Err(FinError::InsufficientFunds {
                need: fill.quantity.value() * fill.price.value() + fill.commission,
                have: self.cash,
            });
        }
        self.cash += cost;
        let pos = self
            .positions
            .entry(fill.symbol.clone())
            .or_insert_with(|| Position::new(fill.symbol.clone()));
        pos.apply_fill(&fill)?;
        Ok(())
    }

    /// Returns the position for `symbol`, or `None` if no position exists.
    pub fn position(&self, symbol: &Symbol) -> Option<&Position> {
        self.positions.get(symbol)
    }

    /// Returns the current cash balance.
    pub fn cash(&self) -> Decimal {
        self.cash
    }

    /// Returns the total realized P&L across all positions.
    pub fn realized_pnl_total(&self) -> Decimal {
        self.positions.values().map(|p| p.realized_pnl).sum()
    }

    /// Returns the total unrealized P&L given a map of current prices.
    ///
    /// # Errors
    /// Returns [`FinError::PositionNotFound`] if a non-flat position has no price in `prices`.
    pub fn unrealized_pnl_total(
        &self,
        prices: &HashMap<String, Price>,
    ) -> Result<Decimal, FinError> {
        let mut total = Decimal::ZERO;
        for (sym, pos) in &self.positions {
            if pos.quantity == Decimal::ZERO {
                continue;
            }
            let price = prices
                .get(sym.as_str())
                .ok_or_else(|| FinError::PositionNotFound(sym.as_str().to_owned()))?;
            total += pos.unrealized_pnl(*price);
        }
        Ok(total)
    }

    /// Returns total equity: `cash + sum(unrealized P&L of open positions)`.
    ///
    /// # Errors
    /// Returns [`FinError::PositionNotFound`] if a position has no price in `prices`.
    pub fn equity(&self, prices: &HashMap<String, Price>) -> Result<Decimal, FinError> {
        Ok(self.cash + self.unrealized_pnl_total(prices)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn sym(s: &str) -> Symbol {
        Symbol::new(s).unwrap()
    }

    fn make_fill(symbol: &str, side: Side, qty: &str, p: &str, commission: &str) -> Fill {
        Fill {
            symbol: sym(symbol),
            side,
            quantity: Quantity::new(qty.parse().unwrap()).unwrap(),
            price: Price::new(p.parse().unwrap()).unwrap(),
            timestamp: NanoTimestamp::new(0),
            commission: commission.parse().unwrap(),
        }
    }

    #[test]
    fn test_position_apply_fill_long() {
        let mut pos = Position::new(sym("AAPL"));
        pos.apply_fill(&make_fill("AAPL", Side::Bid, "10", "100", "0"))
            .unwrap();
        assert_eq!(pos.quantity, dec!(10));
        assert_eq!(pos.avg_cost, dec!(100));
    }

    #[test]
    fn test_position_apply_fill_reduces_position() {
        let mut pos = Position::new(sym("AAPL"));
        pos.apply_fill(&make_fill("AAPL", Side::Bid, "10", "100", "0"))
            .unwrap();
        pos.apply_fill(&make_fill("AAPL", Side::Ask, "5", "110", "0"))
            .unwrap();
        assert_eq!(pos.quantity, dec!(5));
    }

    #[test]
    fn test_position_realized_pnl_on_close() {
        let mut pos = Position::new(sym("AAPL"));
        pos.apply_fill(&make_fill("AAPL", Side::Bid, "10", "100", "0"))
            .unwrap();
        let pnl = pos
            .apply_fill(&make_fill("AAPL", Side::Ask, "10", "110", "0"))
            .unwrap();
        assert_eq!(pnl, dec!(100));
        assert!(pos.is_flat());
    }

    #[test]
    fn test_position_commission_reduces_realized_pnl() {
        let mut pos = Position::new(sym("AAPL"));
        pos.apply_fill(&make_fill("AAPL", Side::Bid, "10", "100", "0"))
            .unwrap();
        let pnl = pos
            .apply_fill(&make_fill("AAPL", Side::Ask, "10", "110", "5"))
            .unwrap();
        assert_eq!(pnl, dec!(95));
    }

    #[test]
    fn test_position_unrealized_pnl() {
        let mut pos = Position::new(sym("AAPL"));
        pos.apply_fill(&make_fill("AAPL", Side::Bid, "10", "100", "0"))
            .unwrap();
        let upnl = pos.unrealized_pnl(Price::new(dec!(115)).unwrap());
        assert_eq!(upnl, dec!(150));
    }

    #[test]
    fn test_position_market_value() {
        let mut pos = Position::new(sym("AAPL"));
        pos.apply_fill(&make_fill("AAPL", Side::Bid, "10", "100", "0"))
            .unwrap();
        assert_eq!(pos.market_value(Price::new(dec!(120)).unwrap()), dec!(1200));
    }

    #[test]
    fn test_position_is_flat_initially() {
        let pos = Position::new(sym("X"));
        assert!(pos.is_flat());
    }

    #[test]
    fn test_position_is_flat_after_full_close() {
        let mut pos = Position::new(sym("AAPL"));
        pos.apply_fill(&make_fill("AAPL", Side::Bid, "10", "100", "0"))
            .unwrap();
        pos.apply_fill(&make_fill("AAPL", Side::Ask, "10", "110", "0"))
            .unwrap();
        assert!(pos.is_flat());
    }

    #[test]
    fn test_position_avg_cost_weighted_after_two_buys() {
        let mut pos = Position::new(sym("X"));
        pos.apply_fill(&make_fill("X", Side::Bid, "10", "100", "0"))
            .unwrap();
        pos.apply_fill(&make_fill("X", Side::Bid, "10", "120", "0"))
            .unwrap();
        assert_eq!(pos.avg_cost, dec!(110));
    }

    #[test]
    fn test_position_ledger_apply_fill_updates_cash() {
        let mut ledger = PositionLedger::new(dec!(10000));
        ledger
            .apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "1"))
            .unwrap();
        assert_eq!(ledger.cash(), dec!(8999));
    }

    #[test]
    fn test_position_ledger_insufficient_funds() {
        let mut ledger = PositionLedger::new(dec!(100));
        let result = ledger.apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0"));
        assert!(matches!(result, Err(FinError::InsufficientFunds { .. })));
    }

    #[test]
    fn test_position_ledger_equity_calculation() {
        let mut ledger = PositionLedger::new(dec!(10000));
        ledger
            .apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0"))
            .unwrap();
        let mut prices = HashMap::new();
        prices.insert("AAPL".to_owned(), Price::new(dec!(110)).unwrap());
        // equity = cash + unrealized = 9000 + (110-100)*10 = 9100
        let equity = ledger.equity(&prices).unwrap();
        assert_eq!(equity, dec!(9100));
    }

    #[test]
    fn test_position_ledger_sell_increases_cash() {
        let mut ledger = PositionLedger::new(dec!(10000));
        ledger
            .apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0"))
            .unwrap();
        ledger
            .apply_fill(make_fill("AAPL", Side::Ask, "10", "110", "0"))
            .unwrap();
        assert_eq!(ledger.cash(), dec!(10100));
    }

    #[test]
    fn test_position_ledger_unrealized_pnl_total() {
        let mut ledger = PositionLedger::new(dec!(10000));
        ledger
            .apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0"))
            .unwrap();
        let mut prices = HashMap::new();
        prices.insert("AAPL".to_owned(), Price::new(dec!(105)).unwrap());
        let upnl = ledger.unrealized_pnl_total(&prices).unwrap();
        assert_eq!(upnl, dec!(50));
    }
}
