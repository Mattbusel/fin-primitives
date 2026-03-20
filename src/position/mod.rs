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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

impl Fill {
    /// Constructs a `Fill` without commission (zero commission).
    pub fn new(
        symbol: Symbol,
        side: Side,
        quantity: Quantity,
        price: Price,
        timestamp: NanoTimestamp,
    ) -> Self {
        Self {
            symbol,
            side,
            quantity,
            price,
            timestamp,
            commission: Decimal::ZERO,
        }
    }

    /// Constructs a `Fill` with the specified commission.
    pub fn with_commission(
        symbol: Symbol,
        side: Side,
        quantity: Quantity,
        price: Price,
        timestamp: NanoTimestamp,
        commission: Decimal,
    ) -> Self {
        Self {
            symbol,
            side,
            quantity,
            price,
            timestamp,
            commission,
        }
    }

    /// Returns the gross notional value of this fill: `price × quantity`.
    ///
    /// Does not subtract commission. Useful for computing total capital deployed
    /// per fill and aggregate turnover statistics.
    pub fn notional(&self) -> Decimal {
        self.price.value() * self.quantity.value()
    }
}

/// Direction of an open position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PositionDirection {
    /// Net quantity is positive.
    Long,
    /// Net quantity is negative.
    Short,
    /// Net quantity is zero.
    Flat,
}

/// A single-symbol position tracking quantity, average cost, and realized P&L.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

    /// Returns unrealized P&L at `current_price`, returning `Err` on arithmetic overflow.
    pub fn checked_unrealized_pnl(&self, current_price: Price) -> Result<Decimal, FinError> {
        let diff = current_price.value() - self.avg_cost;
        self.quantity
            .checked_mul(diff)
            .ok_or(FinError::ArithmeticOverflow)
    }

    /// Returns the market value of this position at `current_price`.
    pub fn market_value(&self, current_price: Price) -> Decimal {
        self.quantity * current_price.value()
    }

    /// Returns `true` if the position is flat (zero quantity).
    pub fn is_flat(&self) -> bool {
        self.quantity == Decimal::ZERO
    }

    /// Returns `true` if the position is long (positive quantity).
    pub fn is_long(&self) -> bool {
        self.quantity > Decimal::ZERO
    }

    /// Returns `true` if the position is short (negative quantity).
    pub fn is_short(&self) -> bool {
        self.quantity < Decimal::ZERO
    }

    /// Returns the direction of the position.
    pub fn direction(&self) -> PositionDirection {
        if self.quantity > Decimal::ZERO {
            PositionDirection::Long
        } else if self.quantity < Decimal::ZERO {
            PositionDirection::Short
        } else {
            PositionDirection::Flat
        }
    }

    /// Returns total P&L: `realized_pnl + unrealized_pnl(current_price)`.
    pub fn total_pnl(&self, current_price: Price) -> Decimal {
        self.realized_pnl + self.unrealized_pnl(current_price)
    }

    /// Returns the absolute magnitude of the current quantity.
    pub fn quantity_abs(&self) -> Decimal {
        self.quantity.abs()
    }

    /// Returns the cost basis of the current position: `avg_cost * |quantity|`.
    ///
    /// Represents total capital deployed, excluding any realized P&L.
    /// Returns `0` when the position is flat.
    pub fn cost_basis(&self) -> Decimal {
        self.avg_cost * self.quantity.abs()
    }

    /// Returns unrealized P&L as a percentage of cost basis.
    ///
    /// Returns `None` when the position is flat (avg_cost is zero).
    pub fn unrealized_pnl_pct(&self, current_price: Price) -> Option<Decimal> {
        if self.avg_cost == Decimal::ZERO {
            return None;
        }
        let pnl = self.unrealized_pnl(current_price);
        let cost_basis = (self.avg_cost * self.quantity.abs()).abs();
        Some(pnl / cost_basis * Decimal::ONE_HUNDRED)
    }

    /// Returns `true` if unrealized PnL at `current_price` is strictly positive.
    pub fn is_profitable(&self, current_price: Price) -> bool {
        self.unrealized_pnl(current_price) > Decimal::ZERO
    }

    /// Returns the average entry price as a `Price`, or `None` if the position is flat.
    ///
    /// This is `avg_cost` expressed as a validated `Price`. Returns `None` when
    /// `avg_cost == 0` (no open position).
    pub fn avg_entry_price(&self) -> Option<Price> {
        Price::new(self.avg_cost).ok()
    }

    /// Returns the stop-loss price at `stop_pct` percent below (long) or above (short) entry.
    ///
    /// - Long: `stop = avg_cost × (1 - stop_pct / 100)`
    /// - Short: `stop = avg_cost × (1 + stop_pct / 100)`
    ///
    /// Returns `None` when the position is flat or `avg_cost` is zero.
    ///
    /// # Example
    /// ```rust,ignore
    /// // A 2% stop loss on a long position at avg_cost=100 → stop at 98
    /// position.stop_loss_price(dec!(2)).unwrap() == Price::new(dec!(98)).unwrap()
    /// ```
    pub fn stop_loss_price(&self, stop_pct: Decimal) -> Option<Price> {
        if self.is_flat() || self.avg_cost.is_zero() {
            return None;
        }
        let factor = stop_pct / Decimal::ONE_HUNDRED;
        let stop = if self.is_long() {
            self.avg_cost * (Decimal::ONE - factor)
        } else {
            self.avg_cost * (Decimal::ONE + factor)
        };
        Price::new(stop).ok()
    }

    /// Returns the take-profit price for the current position at `tp_pct` percent gain.
    ///
    /// Returns `None` when the position is flat or `avg_cost` is zero.
    /// For a long position, the take-profit price is `avg_cost * (1 + tp_pct / 100)`.
    /// For a short position, the take-profit price is `avg_cost * (1 - tp_pct / 100)`.
    pub fn take_profit_price(&self, tp_pct: Decimal) -> Option<Price> {
        if self.is_flat() || self.avg_cost.is_zero() {
            return None;
        }
        let factor = tp_pct / Decimal::ONE_HUNDRED;
        let tp = if self.is_long() {
            self.avg_cost * (Decimal::ONE + factor)
        } else {
            self.avg_cost * (Decimal::ONE - factor)
        };
        Price::new(tp).ok()
    }
}

/// A multi-symbol ledger tracking positions and a cash balance.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

    /// Returns `true` if the ledger is tracking `symbol` (even if flat).
    pub fn has_position(&self, symbol: &Symbol) -> bool {
        self.positions.contains_key(symbol)
    }

    /// Returns an iterator over all tracked positions (including flat ones).
    pub fn positions(&self) -> impl Iterator<Item = &Position> {
        self.positions.values()
    }

    /// Returns an iterator over positions with non-zero quantity.
    pub fn open_positions(&self) -> impl Iterator<Item = &Position> {
        self.positions.values().filter(|p| !p.is_flat())
    }

    /// Returns an iterator over flat (zero-quantity) positions.
    pub fn flat_positions(&self) -> impl Iterator<Item = &Position> {
        self.positions.values().filter(|p| p.is_flat())
    }

    /// Returns an iterator over long (positive-quantity) positions.
    pub fn long_positions(&self) -> impl Iterator<Item = &Position> {
        self.positions.values().filter(|p| p.is_long())
    }

    /// Returns an iterator over short (negative-quantity) positions.
    pub fn short_positions(&self) -> impl Iterator<Item = &Position> {
        self.positions.values().filter(|p| p.is_short())
    }

    /// Returns an iterator over the symbols being tracked by this ledger.
    pub fn symbols(&self) -> impl Iterator<Item = &Symbol> {
        self.positions.keys()
    }

    /// Returns an iterator over symbols that have a non-flat (open) position.
    pub fn open_symbols(&self) -> impl Iterator<Item = &Symbol> {
        self.positions
            .iter()
            .filter(|(_, p)| !p.is_flat())
            .map(|(s, _)| s)
    }

    /// Returns a sorted `Vec` of all tracked symbols in lexicographic order.
    ///
    /// Useful when deterministic output ordering is required (e.g. reports, snapshots).
    pub fn symbols_sorted(&self) -> Vec<&Symbol> {
        let mut syms: Vec<&Symbol> = self.positions.keys().collect();
        syms.sort();
        syms
    }

    /// Returns the total number of symbols tracked by this ledger (open and flat).
    pub fn position_count(&self) -> usize {
        self.positions.len()
    }

    /// Deposits `amount` into the cash balance (increases cash).
    ///
    /// # Panics
    /// Does not panic; accepts any `Decimal` including negative (use `withdraw` for cleaner API).
    pub fn deposit(&mut self, amount: Decimal) {
        self.cash += amount;
    }

    /// Withdraws `amount` from the cash balance.
    ///
    /// # Errors
    /// Returns [`FinError::InsufficientFunds`] if `amount > self.cash`.
    pub fn withdraw(&mut self, amount: Decimal) -> Result<(), FinError> {
        if amount > self.cash {
            return Err(FinError::InsufficientFunds {
                need: amount,
                have: self.cash,
            });
        }
        self.cash -= amount;
        Ok(())
    }

    /// Returns the number of non-flat (open) positions.
    pub fn open_position_count(&self) -> usize {
        self.positions.values().filter(|p| !p.is_flat()).count()
    }

    /// Returns the number of long (positive quantity) open positions.
    pub fn long_count(&self) -> usize {
        self.positions.values().filter(|p| p.quantity > Decimal::ZERO).count()
    }

    /// Returns the number of short (negative quantity) open positions.
    pub fn short_count(&self) -> usize {
        self.positions.values().filter(|p| p.quantity < Decimal::ZERO).count()
    }

    /// Returns the net signed quantity exposure across all positions.
    ///
    /// Long positions contribute positive values; short positions contribute negative values.
    /// A result near zero indicates a roughly delta-neutral portfolio.
    pub fn net_exposure(&self) -> Decimal {
        self.positions.values().map(|p| p.quantity).sum()
    }

    /// Returns the gross (absolute) quantity exposure across all positions.
    ///
    /// Sums `|quantity|` for every position regardless of direction.
    pub fn gross_exposure(&self) -> Decimal {
        self.positions.values().map(|p| p.quantity.abs()).sum()
    }

    /// Returns a reference to the open position with the largest absolute quantity.
    ///
    /// Returns `None` when there are no open (non-flat) positions.
    pub fn largest_position(&self) -> Option<&Position> {
        self.positions
            .values()
            .filter(|p| !p.is_flat())
            .max_by(|a, b| a.quantity.abs().partial_cmp(&b.quantity.abs()).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// Returns the total market value of all open positions given a price map.
    ///
    /// # Errors
    /// Returns [`FinError::PositionNotFound`] if a non-flat position has no price in `prices`.
    pub fn total_market_value(
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
            total += pos.market_value(*price);
        }
        Ok(total)
    }

    /// Returns the current cash balance.
    pub fn cash(&self) -> Decimal {
        self.cash
    }

    /// Returns each open position's market value as a fraction of total market value.
    ///
    /// Returns a `Vec<(Symbol, Decimal)>` where the second element is `[0, 1]`.
    /// Flat positions are excluded. Returns an empty vec if total market value is zero
    /// or if `prices` lacks an entry for an open position (graceful skip).
    pub fn position_weights(&self, prices: &HashMap<String, Price>) -> Vec<(Symbol, Decimal)> {
        let mut mv_pairs: Vec<(Symbol, Decimal)> = self
            .positions
            .iter()
            .filter(|(_, p)| !p.is_flat())
            .filter_map(|(sym, pos)| {
                let price = prices.get(sym.as_str())?;
                Some((sym.clone(), pos.market_value(*price).abs()))
            })
            .collect();
        let total: Decimal = mv_pairs.iter().map(|(_, v)| *v).sum();
        if total.is_zero() {
            return vec![];
        }
        mv_pairs.iter_mut().for_each(|(_, v)| *v /= total);
        mv_pairs
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

    /// Returns the realized P&L for `symbol`, or `None` if the symbol is not tracked.
    pub fn realized_pnl(&self, symbol: &Symbol) -> Option<Decimal> {
        self.positions.get(symbol).map(|p| p.realized_pnl)
    }

    /// Returns total net P&L: `realized_pnl_total + unrealized_pnl_total(prices)`.
    ///
    /// # Errors
    /// Returns [`FinError::PositionNotFound`] if a non-flat position has no price in `prices`.
    pub fn net_pnl(&self, prices: &HashMap<String, Price>) -> Result<Decimal, FinError> {
        Ok(self.realized_pnl_total() + self.unrealized_pnl_total(prices)?)
    }

    /// Returns total equity: `cash + sum(unrealized P&L of open positions)`.
    ///
    /// # Errors
    /// Returns [`FinError::PositionNotFound`] if a position has no price in `prices`.
    pub fn equity(&self, prices: &HashMap<String, Price>) -> Result<Decimal, FinError> {
        Ok(self.cash + self.unrealized_pnl_total(prices)?)
    }

    /// Returns the net liquidation value: `cash + sum(market_value of each open position)`.
    ///
    /// Market value of a position = `quantity × current_price`. This differs from
    /// `equity` which adds unrealized P&L rather than raw market value.
    ///
    /// # Errors
    /// Returns [`FinError::PositionNotFound`] if a position has no price in `prices`.
    pub fn net_liquidation_value(&self, prices: &HashMap<String, Price>) -> Result<Decimal, FinError> {
        let mut total = self.cash;
        for (symbol, pos) in &self.positions {
            if pos.quantity == Decimal::ZERO {
                continue;
            }
            let price = prices
                .get(symbol.as_str())
                .ok_or_else(|| FinError::PositionNotFound(symbol.to_string()))?;
            total += pos.quantity * price.value();
        }
        Ok(total)
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
    fn test_position_ledger_net_liquidation_value() {
        // buy 10 AAPL @ 100 → cash = 10000 - 1000 = 9000
        let mut ledger = PositionLedger::new(dec!(10000));
        ledger
            .apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0"))
            .unwrap();
        let mut prices = HashMap::new();
        prices.insert("AAPL".to_owned(), Price::new(dec!(110)).unwrap());
        // NLV = cash(9000) + 10×110 = 9000 + 1100 = 10100
        let nlv = ledger.net_liquidation_value(&prices).unwrap();
        assert_eq!(nlv, dec!(10100));
    }

    #[test]
    fn test_position_ledger_net_liquidation_missing_price() {
        let mut ledger = PositionLedger::new(dec!(10000));
        ledger
            .apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0"))
            .unwrap();
        let prices: HashMap<String, Price> = HashMap::new();
        assert!(ledger.net_liquidation_value(&prices).is_err());
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
    fn test_position_checked_unrealized_pnl_matches() {
        let mut pos = Position::new(sym("AAPL"));
        pos.apply_fill(&make_fill("AAPL", Side::Bid, "10", "100", "0"))
            .unwrap();
        let price = Price::new(dec!(115)).unwrap();
        let checked = pos.checked_unrealized_pnl(price).unwrap();
        let unchecked = pos.unrealized_pnl(price);
        assert_eq!(checked, unchecked);
        assert_eq!(checked, dec!(150));
    }

    #[test]
    fn test_position_checked_unrealized_pnl_flat_position() {
        let pos = Position::new(sym("X"));
        let price = Price::new(dec!(100)).unwrap();
        assert_eq!(pos.checked_unrealized_pnl(price).unwrap(), dec!(0));
    }

    #[test]
    fn test_position_direction_flat() {
        let pos = Position::new(sym("X"));
        assert_eq!(pos.direction(), PositionDirection::Flat);
    }

    #[test]
    fn test_position_direction_long() {
        let mut pos = Position::new(sym("X"));
        pos.apply_fill(&make_fill("X", Side::Bid, "5", "100", "0"))
            .unwrap();
        assert_eq!(pos.direction(), PositionDirection::Long);
    }

    #[test]
    fn test_position_direction_short() {
        let mut pos = Position::new(sym("X"));
        // Short: sell without prior long (negative quantity via negative fill)
        pos.apply_fill(&make_fill("X", Side::Ask, "5", "100", "0"))
            .unwrap();
        assert_eq!(pos.direction(), PositionDirection::Short);
    }

    #[test]
    fn test_position_ledger_positions_iterator() {
        let mut ledger = PositionLedger::new(dec!(10000));
        ledger
            .apply_fill(make_fill("AAPL", Side::Bid, "1", "100", "0"))
            .unwrap();
        ledger
            .apply_fill(make_fill("MSFT", Side::Bid, "1", "200", "0"))
            .unwrap();
        let count = ledger.positions().count();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_position_ledger_total_market_value() {
        let mut ledger = PositionLedger::new(dec!(10000));
        ledger
            .apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0"))
            .unwrap();
        ledger
            .apply_fill(make_fill("MSFT", Side::Bid, "5", "200", "0"))
            .unwrap();
        let mut prices = HashMap::new();
        prices.insert("AAPL".to_owned(), Price::new(dec!(110)).unwrap());
        prices.insert("MSFT".to_owned(), Price::new(dec!(210)).unwrap());
        // 10*110 + 5*210 = 1100 + 1050 = 2150
        let mv = ledger.total_market_value(&prices).unwrap();
        assert_eq!(mv, dec!(2150));
    }

    #[test]
    fn test_position_ledger_total_market_value_missing_price() {
        let mut ledger = PositionLedger::new(dec!(10000));
        ledger
            .apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0"))
            .unwrap();
        let prices: HashMap<String, Price> = HashMap::new();
        assert!(matches!(
            ledger.total_market_value(&prices),
            Err(FinError::PositionNotFound(_))
        ));
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

    #[test]
    fn test_position_ledger_position_count_includes_flat() {
        let mut ledger = PositionLedger::new(dec!(10000));
        // open AAPL long then close it
        ledger
            .apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0"))
            .unwrap();
        ledger
            .apply_fill(make_fill("AAPL", Side::Ask, "10", "100", "0"))
            .unwrap();
        // open MSFT long (stays open)
        ledger
            .apply_fill(make_fill("MSFT", Side::Bid, "5", "200", "0"))
            .unwrap();
        assert_eq!(ledger.position_count(), 2, "both symbols tracked");
        assert_eq!(ledger.open_position_count(), 1, "only MSFT open");
    }

    #[test]
    fn test_position_ledger_position_count_zero_on_empty() {
        let ledger = PositionLedger::new(dec!(10000));
        assert_eq!(ledger.position_count(), 0);
    }

    #[test]
    fn test_position_unrealized_pnl_pct_long_gain() {
        let mut pos = Position::new(sym("AAPL"));
        pos.apply_fill(&make_fill("AAPL", Side::Bid, "10", "100", "0"))
            .unwrap();
        let current = Price::new(dec!(110)).unwrap();
        let pct = pos.unrealized_pnl_pct(current).unwrap();
        assert_eq!(pct, dec!(10));
    }

    #[test]
    fn test_position_unrealized_pnl_pct_flat_returns_none() {
        let pos = Position::new(sym("AAPL"));
        let current = Price::new(dec!(110)).unwrap();
        assert!(pos.unrealized_pnl_pct(current).is_none());
    }

    #[test]
    fn test_position_unrealized_pnl_pct_loss() {
        let mut pos = Position::new(sym("AAPL"));
        pos.apply_fill(&make_fill("AAPL", Side::Bid, "10", "100", "0"))
            .unwrap();
        let current = Price::new(dec!(90)).unwrap();
        let pct = pos.unrealized_pnl_pct(current).unwrap();
        assert_eq!(pct, dec!(-10));
    }

    #[test]
    fn test_position_ledger_open_positions_excludes_flat() {
        let mut ledger = PositionLedger::new(dec!(10000));
        ledger
            .apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0"))
            .unwrap();
        ledger
            .apply_fill(make_fill("AAPL", Side::Ask, "10", "100", "0"))
            .unwrap();
        ledger
            .apply_fill(make_fill("MSFT", Side::Bid, "5", "200", "0"))
            .unwrap();
        let open: Vec<_> = ledger.open_positions().collect();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].symbol.as_str(), "MSFT");
    }

    #[test]
    fn test_position_ledger_open_positions_empty_when_all_flat() {
        let mut ledger = PositionLedger::new(dec!(10000));
        ledger
            .apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0"))
            .unwrap();
        ledger
            .apply_fill(make_fill("AAPL", Side::Ask, "10", "100", "0"))
            .unwrap();
        let open: Vec<_> = ledger.open_positions().collect();
        assert!(open.is_empty());
    }

    #[test]
    fn test_position_is_long() {
        let mut pos = Position::new(sym("AAPL"));
        pos.apply_fill(&make_fill("AAPL", Side::Bid, "10", "100", "0"))
            .unwrap();
        assert!(pos.is_long());
        assert!(!pos.is_short());
        assert!(!pos.is_flat());
    }

    #[test]
    fn test_position_is_short() {
        let mut pos = Position::new(sym("AAPL"));
        pos.apply_fill(&make_fill("AAPL", Side::Ask, "10", "100", "0"))
            .unwrap();
        assert!(pos.is_short());
        assert!(!pos.is_long());
        assert!(!pos.is_flat());
    }

    #[test]
    fn test_position_is_flat_after_close() {
        let mut pos = Position::new(sym("AAPL"));
        pos.apply_fill(&make_fill("AAPL", Side::Bid, "10", "100", "0"))
            .unwrap();
        pos.apply_fill(&make_fill("AAPL", Side::Ask, "10", "100", "0"))
            .unwrap();
        assert!(pos.is_flat());
        assert!(!pos.is_long());
        assert!(!pos.is_short());
    }

    #[test]
    fn test_position_ledger_flat_positions() {
        let mut ledger = PositionLedger::new(dec!(10000));
        // open AAPL, then close it
        ledger.apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0")).unwrap();
        ledger.apply_fill(make_fill("AAPL", Side::Ask, "10", "100", "0")).unwrap();
        // leave MSFT open
        ledger.apply_fill(make_fill("MSFT", Side::Bid, "5", "200", "0")).unwrap();
        let flat: Vec<_> = ledger.flat_positions().collect();
        assert_eq!(flat.len(), 1);
        assert_eq!(flat[0].symbol, sym("AAPL"));
    }

    #[test]
    fn test_position_ledger_flat_positions_empty_when_all_open() {
        let mut ledger = PositionLedger::new(dec!(10000));
        ledger.apply_fill(make_fill("AAPL", Side::Bid, "1", "100", "0")).unwrap();
        assert_eq!(ledger.flat_positions().count(), 0);
    }

    #[test]
    fn test_position_ledger_deposit_increases_cash() {
        let mut ledger = PositionLedger::new(dec!(1000));
        ledger.deposit(dec!(500));
        assert_eq!(ledger.cash(), dec!(1500));
    }

    #[test]
    fn test_position_ledger_withdraw_decreases_cash() {
        let mut ledger = PositionLedger::new(dec!(1000));
        ledger.withdraw(dec!(300)).unwrap();
        assert_eq!(ledger.cash(), dec!(700));
    }

    #[test]
    fn test_position_ledger_withdraw_insufficient_fails() {
        let mut ledger = PositionLedger::new(dec!(100));
        assert!(matches!(
            ledger.withdraw(dec!(200)),
            Err(FinError::InsufficientFunds { .. })
        ));
        assert_eq!(ledger.cash(), dec!(100), "cash unchanged on failure");
    }

    #[test]
    fn test_position_is_profitable_true() {
        let mut pos = Position::new(sym("AAPL"));
        pos.apply_fill(&make_fill("AAPL", Side::Bid, "10", "100", "0"))
            .unwrap();
        let current = Price::new(dec!(110)).unwrap();
        assert!(pos.is_profitable(current));
    }

    #[test]
    fn test_position_is_profitable_false_when_at_loss() {
        let mut pos = Position::new(sym("AAPL"));
        pos.apply_fill(&make_fill("AAPL", Side::Bid, "10", "100", "0"))
            .unwrap();
        let current = Price::new(dec!(90)).unwrap();
        assert!(!pos.is_profitable(current));
    }

    #[test]
    fn test_position_ledger_long_positions() {
        let mut ledger = PositionLedger::new(dec!(10000));
        ledger
            .apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0"))
            .unwrap();
        let longs: Vec<_> = ledger.long_positions().collect();
        assert_eq!(longs.len(), 1);
        assert_eq!(longs[0].symbol.as_str(), "AAPL");
    }

    #[test]
    fn test_position_ledger_short_positions_empty_for_long_only() {
        let mut ledger = PositionLedger::new(dec!(10000));
        ledger
            .apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0"))
            .unwrap();
        let shorts: Vec<_> = ledger.short_positions().collect();
        assert!(shorts.is_empty());
    }

    #[test]
    fn test_position_ledger_realized_pnl_after_close() {
        let mut ledger = PositionLedger::new(dec!(10000));
        ledger.apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0")).unwrap();
        ledger.apply_fill(make_fill("AAPL", Side::Ask, "10", "110", "0")).unwrap();
        assert_eq!(ledger.realized_pnl(&sym("AAPL")), Some(dec!(100)));
    }

    #[test]
    fn test_position_ledger_realized_pnl_unknown_symbol_returns_none() {
        let ledger = PositionLedger::new(dec!(10000));
        assert!(ledger.realized_pnl(&sym("AAPL")).is_none());
    }

    #[test]
    fn test_position_ledger_realized_pnl_zero_before_close() {
        let mut ledger = PositionLedger::new(dec!(10000));
        ledger.apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0")).unwrap();
        assert_eq!(ledger.realized_pnl(&sym("AAPL")), Some(dec!(0)));
    }

    #[test]
    fn test_position_ledger_symbols_sorted_order() {
        let mut ledger = PositionLedger::new(dec!(10000));
        ledger.apply_fill(make_fill("MSFT", Side::Bid, "1", "100", "0")).unwrap();
        ledger.apply_fill(make_fill("AAPL", Side::Bid, "1", "100", "0")).unwrap();
        ledger.apply_fill(make_fill("GOOG", Side::Bid, "1", "100", "0")).unwrap();
        let sorted = ledger.symbols_sorted();
        let names: Vec<&str> = sorted.iter().map(|s| s.as_str()).collect();
        assert_eq!(names, vec!["AAPL", "GOOG", "MSFT"]);
    }

    #[test]
    fn test_position_ledger_symbols_sorted_empty() {
        let ledger = PositionLedger::new(dec!(10000));
        assert!(ledger.symbols_sorted().is_empty());
    }

    #[test]
    fn test_position_avg_entry_price_long() {
        let sym = Symbol::new("AAPL").unwrap();
        let mut pos = Position::new(sym.clone());
        let fill = Fill::new(
            sym,
            Side::Bid,
            Quantity::new(dec!(10)).unwrap(),
            Price::new(dec!(150)).unwrap(),
            NanoTimestamp::new(0),
        );
        pos.apply_fill(&fill).unwrap();
        assert_eq!(pos.avg_entry_price().unwrap().value(), dec!(150));
    }

    #[test]
    fn test_position_avg_entry_price_flat_returns_none() {
        let sym = Symbol::new("AAPL").unwrap();
        let pos = Position::new(sym);
        assert!(pos.avg_entry_price().is_none());
    }

    #[test]
    fn test_position_avg_entry_price_after_partial_close() {
        let sym = Symbol::new("X").unwrap();
        let mut pos = Position::new(sym.clone());
        pos.apply_fill(&Fill::new(sym.clone(), Side::Bid,
            Quantity::new(dec!(10)).unwrap(), Price::new(dec!(100)).unwrap(),
            NanoTimestamp::new(0))).unwrap();
        pos.apply_fill(&Fill::new(sym.clone(), Side::Ask,
            Quantity::new(dec!(5)).unwrap(), Price::new(dec!(100)).unwrap(),
            NanoTimestamp::new(1))).unwrap();
        // Still long 5 at avg_cost = 100
        assert_eq!(pos.avg_entry_price().unwrap().value(), dec!(100));
    }

    #[test]
    fn test_position_ledger_has_position_true_after_fill() {
        let mut ledger = PositionLedger::new(dec!(10000));
        ledger.apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0")).unwrap();
        assert!(ledger.has_position(&sym("AAPL")));
    }

    #[test]
    fn test_position_ledger_has_position_false_for_unknown() {
        let ledger = PositionLedger::new(dec!(10000));
        assert!(!ledger.has_position(&sym("AAPL")));
    }

    #[test]
    fn test_position_ledger_has_position_true_even_when_flat() {
        let mut ledger = PositionLedger::new(dec!(10000));
        ledger.apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0")).unwrap();
        ledger.apply_fill(make_fill("AAPL", Side::Ask, "10", "100", "0")).unwrap();
        // position is flat but still tracked
        assert!(ledger.has_position(&sym("AAPL")));
    }

    #[test]
    fn test_position_ledger_open_symbols_returns_non_flat() {
        let mut ledger = PositionLedger::new(dec!(10000));
        ledger.apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0")).unwrap();
        ledger.apply_fill(make_fill("MSFT", Side::Bid, "5", "200", "1")).unwrap();
        let symbols: Vec<_> = ledger.open_symbols().collect();
        assert_eq!(symbols.len(), 2);
    }

    #[test]
    fn test_position_ledger_open_symbols_excludes_flat() {
        let mut ledger = PositionLedger::new(dec!(10000));
        ledger.apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0")).unwrap();
        ledger.apply_fill(make_fill("AAPL", Side::Ask, "10", "100", "1")).unwrap(); // flat
        ledger.apply_fill(make_fill("MSFT", Side::Bid, "5", "200", "2")).unwrap();
        let symbols: Vec<_> = ledger.open_symbols().collect();
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].as_str(), "MSFT");
    }

    #[test]
    fn test_position_ledger_open_symbols_empty_when_all_flat() {
        let mut ledger = PositionLedger::new(dec!(10000));
        ledger.apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0")).unwrap();
        ledger.apply_fill(make_fill("AAPL", Side::Ask, "10", "100", "1")).unwrap();
        let symbols: Vec<_> = ledger.open_symbols().collect();
        assert!(symbols.is_empty());
    }
}
