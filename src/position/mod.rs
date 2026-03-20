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
    /// Bar index at which the current position leg was opened. Set via [`Position::set_open_bar`].
    #[serde(default)]
    pub open_bar: usize,
}

impl Position {
    /// Creates a new flat `Position` for `symbol`.
    pub fn new(symbol: Symbol) -> Self {
        Self {
            symbol,
            quantity: Decimal::ZERO,
            avg_cost: Decimal::ZERO,
            realized_pnl: Decimal::ZERO,
            open_bar: 0,
        }
    }

    /// Records the bar index at which the current position leg was opened.
    ///
    /// Call this whenever transitioning from flat to a new position.
    pub fn set_open_bar(&mut self, bar: usize) {
        self.open_bar = bar;
    }

    /// Returns how many bars the current position has been open.
    ///
    /// `age = current_bar - self.open_bar` (saturating at 0).
    pub fn position_age_bars(&self, current_bar: usize) -> usize {
        current_bar.saturating_sub(self.open_bar)
    }

    /// Maximum favorable excursion (MFE): the best unrealized P&L seen across `prices`.
    ///
    /// For a long position, this is `max(price - avg_cost) * quantity`.
    /// For a short position, this is `max(avg_cost - price) * |quantity|`.
    ///
    /// Returns `None` when the position is flat, `avg_cost` is zero, or `prices` is empty.
    pub fn max_favorable_excursion(&self, prices: &[Price]) -> Option<Decimal> {
        if self.is_flat() || self.avg_cost.is_zero() || prices.is_empty() {
            return None;
        }
        let best = if self.is_long() {
            prices
                .iter()
                .map(|p| (p.value() - self.avg_cost) * self.quantity)
                .fold(Decimal::MIN, Decimal::max)
        } else {
            prices
                .iter()
                .map(|p| (self.avg_cost - p.value()) * self.quantity.abs())
                .fold(Decimal::MIN, Decimal::max)
        };
        if best < Decimal::ZERO {
            Some(Decimal::ZERO)
        } else {
            Some(best)
        }
    }

    /// Kelly fraction: optimal bet size as a fraction of capital.
    ///
    /// `Kelly = win_rate - (1 - win_rate) / (avg_win / avg_loss)`
    ///
    /// Returns `None` when `avg_loss` or `avg_win` is zero.
    /// The result is clamped to `[0, 1]` — never bet more than 100% or go short via Kelly.
    pub fn kelly_fraction(
        win_rate: Decimal,
        avg_win: Decimal,
        avg_loss: Decimal,
    ) -> Option<Decimal> {
        if avg_loss.is_zero() || avg_win.is_zero() {
            return None;
        }
        let odds = avg_win / avg_loss;
        let kelly = win_rate - (Decimal::ONE - win_rate) / odds;
        Some(kelly.max(Decimal::ZERO).min(Decimal::ONE))
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

    /// Returns unrealized P&L as a percentage of cost basis at `current_price`.
    ///
    /// `pct = unrealized_pnl / (|quantity| × avg_cost) × 100`.
    /// Returns `None` if the position is flat or `avg_cost` is zero.
    pub fn unrealized_pnl_pct(&self, current_price: Price) -> Option<Decimal> {
        if self.is_flat() || self.avg_cost.is_zero() {
            return None;
        }
        let cost_basis = self.quantity.abs() * self.avg_cost;
        if cost_basis.is_zero() {
            return None;
        }
        let upnl = self.unrealized_pnl(current_price);
        upnl.checked_div(cost_basis).map(|r| r * Decimal::from(100u32))
    }

    /// Returns the total cost basis: `|quantity| * avg_cost`.
    ///
    /// Represents the total capital committed to this position.
    /// Returns zero for flat positions.
    pub fn total_cost_basis(&self) -> Decimal {
        self.quantity.abs() * self.avg_cost
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

    /// Returns the position's current market value as a percentage of `total_portfolio_value`.
    ///
    /// `exposure_pct = |quantity × current_price| / total_portfolio_value × 100`
    ///
    /// Returns `None` when `total_portfolio_value` is zero, the position is flat, or
    /// `current_price` is zero.
    pub fn exposure_pct(&self, current_price: Price, total_portfolio_value: Decimal) -> Option<Decimal> {
        if total_portfolio_value.is_zero() || self.is_flat() {
            return None;
        }
        let market_value = (self.quantity * current_price.value()).abs();
        Some(market_value / total_portfolio_value * Decimal::ONE_HUNDRED)
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

    /// Returns the margin requirement for the current position: `|net_quantity| × avg_cost × margin_pct / 100`.
    ///
    /// Returns `None` if the position is flat or `avg_cost` is zero.
    pub fn margin_requirement(&self, margin_pct: Decimal) -> Option<Decimal> {
        if self.is_flat() || self.avg_cost.is_zero() {
            return None;
        }
        let notional = self.quantity.abs() * self.avg_cost;
        Some(notional * margin_pct / Decimal::ONE_HUNDRED)
    }

    /// Returns the risk/reward ratio: `target_pct / stop_pct`.
    ///
    /// This is a pure calculation and does not depend on position state.
    /// Returns `None` if `stop_pct` is zero or negative.
    pub fn risk_reward_ratio(stop_pct: Decimal, target_pct: Decimal) -> Option<f64> {
        use rust_decimal::prelude::ToPrimitive;
        if stop_pct <= Decimal::ZERO {
            return None;
        }
        (target_pct / stop_pct).to_f64()
    }

    /// Leverage: `|quantity × avg_cost| / portfolio_value`.
    ///
    /// Returns `None` if the position is flat, `avg_cost` is zero, or `portfolio_value` is zero.
    pub fn leverage(&self, portfolio_value: Decimal) -> Option<Decimal> {
        if self.is_flat() || self.avg_cost.is_zero() || portfolio_value.is_zero() {
            return None;
        }
        let notional = self.quantity.abs() * self.avg_cost;
        Some(notional / portfolio_value)
    }
}

/// A multi-symbol ledger tracking positions and a cash balance.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PositionLedger {
    positions: HashMap<Symbol, Position>,
    cash: Decimal,
    total_commission_paid: Decimal,
}

impl PositionLedger {
    /// Creates a new `PositionLedger` with the given initial cash balance.
    pub fn new(initial_cash: Decimal) -> Self {
        Self {
            positions: HashMap::new(),
            cash: initial_cash,
            total_commission_paid: Decimal::ZERO,
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
        self.total_commission_paid += fill.commission;
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

    /// Returns the sum of `|quantity| × avg_cost` for all long (positive quantity) positions.
    ///
    /// Represents the notional value invested on the long side.
    pub fn total_long_exposure(&self) -> Decimal {
        self.positions
            .values()
            .filter(|p| p.is_long())
            .map(|p| p.quantity.abs() * p.avg_cost)
            .sum()
    }

    /// Returns the sum of `|quantity| × avg_cost` for all short (negative quantity) positions.
    ///
    /// Represents the notional value of the short exposure.
    pub fn total_short_exposure(&self) -> Decimal {
        self.positions
            .values()
            .filter(|p| p.is_short())
            .map(|p| p.quantity.abs() * p.avg_cost)
            .sum()
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

    /// Net market exposure using current prices: sum of (quantity × price) across all positions.
    ///
    /// Long positions contribute positive values; short positions contribute negative values.
    /// Prices missing from `prices` are skipped.
    /// Returns `None` if no open positions have prices available.
    pub fn net_market_exposure(&self, prices: &std::collections::HashMap<String, Price>) -> Option<Decimal> {
        let mut found = false;
        let mut net = Decimal::ZERO;
        for pos in self.positions.values() {
            if pos.quantity.is_zero() { continue; }
            if let Some(&price) = prices.get(pos.symbol.as_str()) {
                found = true;
                net += pos.quantity * price.value();
            }
        }
        if found { Some(net) } else { None }
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
    /// Returns the number of positions with non-zero quantity.
    pub fn open_count(&self) -> usize {
        self.positions.values().filter(|p| !p.is_flat()).count()
    }

    /// Returns a reference to the open position with the largest absolute quantity.
    ///
    /// Returns `None` if there are no open (non-flat) positions.
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

    /// Returns the gross exposure: sum of `|quantity × price|` across all open positions.
    ///
    /// Returns unrealized P&L per symbol as a `HashMap`.
    ///
    /// # Errors
    /// Returns [`FinError::PositionNotFound`] if a non-flat position has no price in `prices`.
    pub fn pnl_by_symbol(&self, prices: &HashMap<String, Price>) -> Result<HashMap<Symbol, Decimal>, FinError> {
        let mut map = HashMap::new();
        for (symbol, pos) in &self.positions {
            if pos.quantity == Decimal::ZERO {
                continue;
            }
            let price = prices
                .get(symbol.as_str())
                .ok_or_else(|| FinError::PositionNotFound(symbol.to_string()))?;
            map.insert(symbol.clone(), pos.unrealized_pnl(*price));
        }
        Ok(map)
    }

    /// Returns `true` if the portfolio is approximately delta-neutral.
    ///
    /// Delta-neutral: `|net_exposure| / gross_exposure < 0.01` (within 1%).
    /// Returns `true` when there are no open positions.
    ///
    /// # Errors
    /// Returns [`FinError::PositionNotFound`] if a non-flat position has no price in `prices`.
    pub fn delta_neutral_check(&self, prices: &HashMap<String, Price>) -> Result<bool, FinError> {
        let mut net = Decimal::ZERO;
        let mut gross = Decimal::ZERO;
        for (symbol, pos) in &self.positions {
            if pos.quantity == Decimal::ZERO {
                continue;
            }
            let price = prices
                .get(symbol.as_str())
                .ok_or_else(|| FinError::PositionNotFound(symbol.to_string()))?;
            let exposure = pos.quantity * price.value();
            net += exposure;
            gross += exposure.abs();
        }
        if gross == Decimal::ZERO {
            return Ok(true);
        }
        Ok((net / gross).abs() < Decimal::new(1, 2)) // < 0.01
    }

    /// Returns the allocation percentage of a symbol within the total portfolio value.
    ///
    /// `allocation = |qty * price| / total_market_value * 100`.
    /// Returns `None` if the symbol has no open position, the price is not provided,
    /// or total market value is zero.
    ///
    /// # Errors
    /// Returns [`crate::error::FinError::PositionNotFound`] if `symbol` is unknown.
    pub fn allocation_pct(
        &self,
        symbol: &Symbol,
        prices: &HashMap<String, Price>,
    ) -> Result<Option<Decimal>, crate::error::FinError> {
        let pos = self
            .positions
            .get(symbol)
            .ok_or_else(|| crate::error::FinError::PositionNotFound(symbol.to_string()))?;
        if pos.quantity == Decimal::ZERO {
            return Ok(None);
        }
        let price = match prices.get(symbol.as_str()) {
            Some(p) => *p,
            None => return Ok(None),
        };
        let notional = (pos.quantity * price.value()).abs();
        let total = self.total_market_value(prices)?;
        if total.is_zero() {
            return Ok(None);
        }
        Ok(Some(notional / total * Decimal::ONE_HUNDRED))
    }

    /// Returns open positions sorted descending by unrealized PnL.
    ///
    /// Positions not in `prices` are assigned a PnL of zero for sorting purposes.
    pub fn positions_sorted_by_pnl(&self, prices: &HashMap<String, Price>) -> Vec<&Position> {
        let mut open: Vec<&Position> = self
            .positions
            .values()
            .filter(|p| p.quantity != Decimal::ZERO)
            .collect();
        open.sort_by(|a, b| {
            let pnl_a = prices
                .get(a.symbol.as_str())
                .map_or(Decimal::ZERO, |&p| a.unrealized_pnl(p));
            let pnl_b = prices
                .get(b.symbol.as_str())
                .map_or(Decimal::ZERO, |&p| b.unrealized_pnl(p));
            pnl_b.cmp(&pnl_a)
        });
        open
    }

    /// Returns the top `n` open positions sorted by absolute market value descending.
    ///
    /// Positions missing from `prices` are assigned market value of zero and sink to the bottom.
    pub fn top_n_positions<'a>(&'a self, n: usize, prices: &HashMap<String, Price>) -> Vec<&'a Position> {
        let mut open: Vec<&Position> = self.positions.values().filter(|p| !p.is_flat()).collect();
        open.sort_by(|a, b| {
            let mv_a = prices.get(a.symbol.as_str())
                .map_or(Decimal::ZERO, |p| (a.quantity * p.value()).abs());
            let mv_b = prices.get(b.symbol.as_str())
                .map_or(Decimal::ZERO, |p| (b.quantity * p.value()).abs());
            mv_b.cmp(&mv_a)
        });
        open.into_iter().take(n).collect()
    }

    /// Returns the Herfindahl-Hirschman Index of position weights (0–1).
    ///
    /// `HHI = Σ(weight_i²)` where `weight_i = |mv_i| / gross_exposure`.
    ///
    /// Values near 1 indicate high concentration (single dominant position);
    /// near `1/n` indicate equal distribution. Returns `None` when no open positions.
    ///
    /// # Errors
    /// Returns [`FinError::PositionNotFound`] if a non-flat position has no price in `prices`.
    pub fn concentration(&self, prices: &HashMap<String, Price>) -> Result<Option<Decimal>, FinError> {
        let gross = self.gross_exposure();
        if gross == Decimal::ZERO {
            return Ok(None);
        }
        let mut hhi = Decimal::ZERO;
        for (symbol, pos) in &self.positions {
            if pos.quantity == Decimal::ZERO {
                continue;
            }
            let price = prices
                .get(symbol.as_str())
                .ok_or_else(|| FinError::PositionNotFound(symbol.to_string()))?;
            let mv = (pos.quantity * price.value()).abs();
            let w = mv / gross;
            hhi += w * w;
        }
        Ok(Some(hhi))
    }

    /// Returns the margin required: `gross_exposure × margin_rate`.
    ///
    /// # Errors
    /// Returns [`FinError::PositionNotFound`] if a non-flat position has no price in `prices`.
    pub fn margin_used(&self, prices: &HashMap<String, Price>, margin_rate: Decimal) -> Result<Decimal, FinError> {
        let mut gross = Decimal::ZERO;
        for (symbol, pos) in &self.positions {
            if pos.quantity == Decimal::ZERO {
                continue;
            }
            let price = prices
                .get(symbol.as_str())
                .ok_or_else(|| FinError::PositionNotFound(symbol.to_string()))?;
            gross += (pos.quantity * price.value()).abs();
        }
        Ok(gross * margin_rate)
    }

    /// Returns the count of tracked positions with zero quantity (flat positions).
    pub fn flat_count(&self) -> usize {
        self.positions.values().filter(|p| p.is_flat()).count()
    }

    /// Returns the open position with the smallest absolute quantity.
    ///
    /// Returns `None` if there are no open (non-flat) positions.
    pub fn smallest_position(&self) -> Option<&Position> {
        self.positions
            .values()
            .filter(|p| !p.is_flat())
            .min_by(|a, b| a.quantity.abs().partial_cmp(&b.quantity.abs()).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// Returns the symbol with the highest unrealized PnL given current `prices`.
    ///
    /// Returns `None` if there are no open positions or the price map is empty.
    pub fn most_profitable_symbol(
        &self,
        prices: &HashMap<String, Price>,
    ) -> Option<&Symbol> {
        self.positions
            .iter()
            .filter(|(_, p)| !p.is_flat())
            .filter_map(|(sym, p)| {
                let price = prices.get(sym.as_str())?;
                let pnl = p.unrealized_pnl(*price);
                Some((sym, pnl))
            })
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(sym, _)| sym)
    }

    /// Returns the symbol with the lowest (most negative) unrealized PnL given current `prices`.
    ///
    /// Returns `None` if there are no open positions or the price map is empty.
    pub fn least_profitable_symbol(
        &self,
        prices: &HashMap<String, Price>,
    ) -> Option<&Symbol> {
        self.positions
            .iter()
            .filter(|(_, p)| !p.is_flat())
            .filter_map(|(sym, p)| {
                let price = prices.get(sym.as_str())?;
                let pnl = p.unrealized_pnl(*price);
                Some((sym, pnl))
            })
            .min_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(sym, _)| sym)
    }

    /// Returns the cumulative commissions paid across all fills processed by this ledger.
    pub fn total_commission_paid(&self) -> Decimal {
        self.total_commission_paid
    }

    /// Returns all open positions as `(Symbol, unrealized_pnl)` sorted by PnL descending.
    ///
    /// Symbols without a price entry in `prices` are skipped.
    pub fn symbols_with_pnl(
        &self,
        prices: &HashMap<String, Price>,
    ) -> Vec<(&Symbol, Decimal)> {
        let mut result: Vec<(&Symbol, Decimal)> = self
            .positions
            .iter()
            .filter(|(_, p)| !p.is_flat())
            .filter_map(|(sym, p)| {
                let price = prices.get(sym.as_str())?;
                Some((sym, p.unrealized_pnl(*price)))
            })
            .collect();
        result.sort_by(|(_, a), (_, b)| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
        result
    }

    /// Returns the fraction of total portfolio value held in a single symbol (as a percentage).
    ///
    /// `concentration = market_value(symbol) / total_market_value * 100`.
    /// Returns `None` if the symbol is not found, price is missing, or total value is zero.
    pub fn concentration_pct(
        &self,
        symbol: &Symbol,
        prices: &HashMap<String, Price>,
    ) -> Option<Decimal> {
        let pos = self.positions.get(symbol)?;
        let price = prices.get(symbol.as_str())?;
        let mv = pos.quantity.abs() * price.value();
        let total = self
            .positions
            .values()
            .filter_map(|p| {
                let pr = prices.get(p.symbol.as_str())?;
                Some(p.quantity.abs() * pr.value())
            })
            .sum::<Decimal>();
        if total.is_zero() {
            return None;
        }
        Some(mv / total * Decimal::ONE_HUNDRED)
    }

    /// Returns `true` if all registered positions are flat (zero quantity).
    pub fn all_flat(&self) -> bool {
        self.positions.values().all(|p| p.is_flat())
    }

    /// Total market value of all long (positive quantity) positions.
    ///
    /// Skips any symbol not present in `prices`. Returns `Decimal::ZERO` when there are no longs.
    pub fn long_exposure(&self, prices: &HashMap<String, Price>) -> Decimal {
        self.positions
            .iter()
            .filter(|(_, p)| p.is_long())
            .filter_map(|(sym, p)| {
                let price = prices.get(sym.as_str())?;
                Some(p.quantity.abs() * price.value())
            })
            .sum()
    }

    /// Total market value of all short (negative quantity) positions.
    ///
    /// Skips any symbol not present in `prices`. Returns `Decimal::ZERO` when there are no shorts.
    pub fn short_exposure(&self, prices: &HashMap<String, Price>) -> Decimal {
        self.positions
            .iter()
            .filter(|(_, p)| p.is_short())
            .filter_map(|(sym, p)| {
                let price = prices.get(sym.as_str())?;
                Some(p.quantity.abs() * price.value())
            })
            .sum()
    }

    /// Signed net market value: `long_exposure - short_exposure`.
    ///
    /// Positive = net long; negative = net short; zero = balanced or flat.
    pub fn net_delta(&self, prices: &HashMap<String, Price>) -> Decimal {
        self.long_exposure(prices) - self.short_exposure(prices)
    }

    /// Returns the average cost basis for `symbol`, or `None` if the position is flat or unknown.
    pub fn avg_cost_basis(&self, symbol: &Symbol) -> Option<Decimal> {
        let pos = self.positions.get(symbol)?;
        if pos.is_flat() { return None; }
        Some(pos.avg_cost)
    }

    /// Returns a list of symbols that currently have a non-flat (open) position.
    pub fn active_symbols(&self) -> Vec<&Symbol> {
        self.positions
            .iter()
            .filter(|(_, pos)| !pos.is_flat())
            .map(|(sym, _)| sym)
            .collect()
    }

    /// Returns the total number of symbols tracked by this ledger (including flat positions).
    pub fn symbol_count(&self) -> usize {
        self.positions.len()
    }

    /// Returns the realized P&L for every symbol that has a non-zero realized P&L,
    /// sorted descending by value.
    ///
    /// Symbols with zero realized P&L are excluded.
    pub fn realized_pnl_by_symbol(&self) -> Vec<(Symbol, Decimal)> {
        let mut pairs: Vec<(Symbol, Decimal)> = self
            .positions
            .iter()
            .filter_map(|(sym, pos)| {
                let r = pos.realized_pnl;
                if r != Decimal::ZERO { Some((sym.clone(), r)) } else { None }
            })
            .collect();
        pairs.sort_by(|a, b| b.1.cmp(&a.1));
        pairs
    }

    /// Returns up to `n` open positions with the worst (most negative) unrealized P&L.
    ///
    /// Positions missing from `prices` receive an unrealized PnL of zero.
    /// Returns an empty slice when `n == 0` or no open positions exist.
    pub fn top_losers<'a>(
        &'a self,
        n: usize,
        prices: &HashMap<String, Price>,
    ) -> Vec<&'a Position> {
        if n == 0 {
            return vec![];
        }
        let mut open: Vec<&Position> =
            self.positions.values().filter(|p| !p.is_flat()).collect();
        open.sort_by(|a, b| {
            let pnl_a = prices
                .get(a.symbol.as_str())
                .map_or(Decimal::ZERO, |&p| a.unrealized_pnl(p));
            let pnl_b = prices
                .get(b.symbol.as_str())
                .map_or(Decimal::ZERO, |&p| b.unrealized_pnl(p));
            pnl_a.cmp(&pnl_b) // ascending: worst first
        });
        open.into_iter().take(n).collect()
    }

    /// Returns the symbols that currently have flat (zero-quantity) positions,
    /// sorted lexicographically.
    pub fn flat_symbols(&self) -> Vec<&Symbol> {
        let mut syms: Vec<&Symbol> = self.positions
            .iter()
            .filter_map(|(sym, pos)| if pos.is_flat() { Some(sym) } else { None })
            .collect();
        syms.sort();
        syms
    }

    /// Largest unrealized loss among all open positions.
    ///
    /// Returns `None` if there are no open positions or all unrealized PnLs are non-negative.
    pub fn max_unrealized_loss(&self, prices: &HashMap<String, Price>) -> Option<Decimal> {
        self.positions
            .values()
            .filter(|p| !p.is_flat())
            .filter_map(|p| {
                let price = prices.get(p.symbol.as_str()).copied()?;
                let upnl = p.unrealized_pnl(price);
                if upnl < Decimal::ZERO { Some(upnl) } else { None }
            })
            .min_by(|a, b| a.cmp(b))
    }

    /// Returns the position with the largest positive unrealized P&L at the given prices.
    ///
    /// Returns `None` if there are no open positions or no position has a positive unrealized PnL.
    pub fn largest_winner<'a>(&'a self, prices: &HashMap<String, Price>) -> Option<&'a Position> {
        self.positions
            .values()
            .filter(|p| !p.is_flat())
            .filter_map(|p| {
                let price = prices.get(p.symbol.as_str()).copied()?;
                let upnl = p.unrealized_pnl(price);
                if upnl > Decimal::ZERO { Some((p, upnl)) } else { None }
            })
            .max_by(|a, b| a.1.cmp(&b.1))
            .map(|(p, _)| p)
    }

    /// Returns the position with the largest negative unrealized P&L at the given prices.
    ///
    /// Returns `None` if there are no open positions or no position has a negative unrealized PnL.
    pub fn largest_loser<'a>(&'a self, prices: &HashMap<String, Price>) -> Option<&'a Position> {
        self.positions
            .values()
            .filter(|p| !p.is_flat())
            .filter_map(|p| {
                let price = prices.get(p.symbol.as_str()).copied()?;
                let upnl = p.unrealized_pnl(price);
                if upnl < Decimal::ZERO { Some((p, upnl)) } else { None }
            })
            .min_by(|a, b| a.1.cmp(&b.1))
            .map(|(p, _)| p)
    }

    /// Returns the gross market exposure: sum of absolute market values across all open positions.
    pub fn gross_market_exposure(&self, prices: &HashMap<String, Price>) -> Decimal {
        self.positions
            .values()
            .filter(|p| !p.is_flat())
            .filter_map(|p| {
                let price = prices.get(p.symbol.as_str()).copied()?;
                Some(p.market_value(price).abs())
            })
            .sum()
    }

    /// Returns the largest single-position market value as a percentage of total gross exposure.
    ///
    /// Returns `None` if there are no open positions or total exposure is zero.
    pub fn largest_position_pct(&self, prices: &HashMap<String, Price>) -> Option<Decimal> {
        let total = self.gross_market_exposure(prices);
        if total.is_zero() { return None; }
        let max_mv = self.positions
            .values()
            .filter(|p| !p.is_flat())
            .filter_map(|p| {
                let price = prices.get(p.symbol.as_str()).copied()?;
                Some(p.market_value(price).abs())
            })
            .max_by(|a, b| a.cmp(b))?;
        Some(max_mv / total * Decimal::from(100u32))
    }

    /// Total unrealized P&L as a percentage of total cost basis.
    ///
    /// `upnl_pct = unrealized_pnl_total / total_cost_basis × 100`
    ///
    /// Returns `None` if total cost basis is zero.
    pub fn unrealized_pnl_pct(&self, prices: &HashMap<String, Price>) -> Option<Decimal> {
        let total_upnl = self.unrealized_pnl_total(prices).ok()?;
        let total_cost: Decimal = self.positions
            .values()
            .filter(|p| !p.is_flat())
            .map(|p| p.cost_basis().abs())
            .sum();
        if total_cost.is_zero() { return None; }
        Some(total_upnl / total_cost * Decimal::from(100u32))
    }

    /// Returns the symbols of all open positions with positive unrealized P&L at `prices`.
    ///
    /// A position is "up" if `unrealized_pnl > 0` at the given prices.
    pub fn symbols_up<'a>(&'a self, prices: &HashMap<String, Price>) -> Vec<&'a Symbol> {
        self.positions
            .values()
            .filter(|p| !p.is_flat())
            .filter(|p| {
                prices.get(p.symbol.as_str())
                    .map(|&price| p.unrealized_pnl(price) > Decimal::ZERO)
                    .unwrap_or(false)
            })
            .map(|p| &p.symbol)
            .collect()
    }

    /// Returns the symbols of all open positions with negative unrealized P&L at `prices`.
    ///
    /// A position is "down" if `unrealized_pnl < 0` at the given prices.
    pub fn symbols_down<'a>(&'a self, prices: &HashMap<String, Price>) -> Vec<&'a Symbol> {
        self.positions
            .values()
            .filter(|p| !p.is_flat())
            .filter(|p| {
                prices.get(p.symbol.as_str())
                    .map(|&price| p.unrealized_pnl(price) < Decimal::ZERO)
                    .unwrap_or(false)
            })
            .map(|p| &p.symbol)
            .collect()
    }

    /// Returns the open position with the largest positive unrealized P&L at `prices`.
    ///
    /// Alias for [`PositionLedger::largest_winner`] with a more descriptive name.
    /// Returns `None` if no positions have positive unrealized PnL.
    pub fn largest_unrealized_gain<'a>(&'a self, prices: &HashMap<String, Price>) -> Option<&'a Position> {
        self.largest_winner(prices)
    }

    /// Average realized P&L per symbol across all positions (including flat ones).
    ///
    /// Returns `None` if there are no positions.
    pub fn avg_realized_pnl_per_symbol(&self) -> Option<Decimal> {
        if self.positions.is_empty() { return None; }
        let total: Decimal = self.positions.values().map(|p| p.realized_pnl).sum();
        #[allow(clippy::cast_possible_truncation)]
        Some(total / Decimal::from(self.positions.len() as u32))
    }

    /// Win rate: fraction of positions with strictly positive realized P&L, as a percentage.
    ///
    /// Only positions that have been at least partially closed (non-zero realized PnL activity)
    /// are considered; positions with zero realized P&L are treated as losses.
    ///
    /// Returns `None` if there are no positions.
    pub fn win_rate(&self) -> Option<Decimal> {
        if self.positions.is_empty() { return None; }
        let total = self.positions.len();
        let winners = self.positions.values()
            .filter(|p| p.realized_pnl > Decimal::ZERO)
            .count();
        #[allow(clippy::cast_possible_truncation)]
        Some(Decimal::from(winners as u32) / Decimal::from(total as u32) * Decimal::from(100u32))
    }

    /// Total P&L (realized + unrealized) excluding a specific symbol.
    ///
    /// Useful for single-symbol attribution analysis.
    /// Returns `Err` if any open position's price is missing from `prices`.
    pub fn net_pnl_excluding(
        &self,
        exclude: &Symbol,
        prices: &HashMap<String, Price>,
    ) -> Result<Decimal, FinError> {
        let total = self.net_pnl(prices)?;
        let excluded_rpnl = self.realized_pnl(exclude).unwrap_or(Decimal::ZERO);
        let excluded_upnl = if let Some(pos) = self.positions.get(exclude) {
            if !pos.is_flat() {
                let price = prices.get(exclude.as_str())
                    .copied()
                    .ok_or_else(|| FinError::InvalidSymbol(exclude.as_str().to_string()))?;
                pos.unrealized_pnl(price)
            } else {
                Decimal::ZERO
            }
        } else {
            Decimal::ZERO
        };
        Ok(total - excluded_rpnl - excluded_upnl)
    }

    /// Ratio of total long market exposure to total absolute short market exposure.
    ///
    /// `long_short_ratio = long_exposure / |short_exposure|`
    ///
    /// Returns `None` if there is no short exposure or `short_exposure` is zero.
    pub fn long_short_ratio(&self, prices: &HashMap<String, Price>) -> Option<Decimal> {
        let long_exp = self.long_exposure(prices);
        let short_exp = self.short_exposure(prices).abs();
        if short_exp.is_zero() { return None; }
        long_exp.checked_div(short_exp)
    }

    /// Returns `(long_count, short_count)` — the number of open long and short positions.
    pub fn position_count_by_direction(&self) -> (usize, usize) {
        let longs = self.positions.values()
            .filter(|p| !p.is_flat() && p.quantity > Decimal::ZERO)
            .count();
        let shorts = self.positions.values()
            .filter(|p| !p.is_flat() && p.quantity < Decimal::ZERO)
            .count();
        (longs, shorts)
    }

    /// Returns the age in bars of the oldest open position.
    ///
    /// Returns `None` if there are no open positions or no position has an open bar set.
    pub fn max_position_age_bars(&self, current_bar: usize) -> Option<usize> {
        self.positions.values()
            .filter(|p| !p.is_flat())
            .map(|p| p.position_age_bars(current_bar))
            .max()
    }

    /// Returns the mean age in bars of all open positions.
    ///
    /// Returns `None` if there are no open positions.
    pub fn avg_position_age_bars(&self, current_bar: usize) -> Option<Decimal> {
        let ages: Vec<usize> = self.positions.values()
            .filter(|p| !p.is_flat())
            .map(|p| p.position_age_bars(current_bar))
            .collect();
        if ages.is_empty() { return None; }
        let sum: usize = ages.iter().sum();
        Some(Decimal::from(sum as u64) / Decimal::from(ages.len() as u64))
    }

    /// Herfindahl-Hirschman Index (HHI) of portfolio concentration by market value.
    ///
    /// HHI = Σ(weight_i²) where weight_i = |market_value_i| / total_gross_exposure.
    /// Range [0, 1]: 0 = perfectly diversified, 1 = entirely in one position.
    ///
    /// Returns `None` if there are no open positions or total gross exposure is zero.
    pub fn hhi_concentration(&self, prices: &HashMap<String, Price>) -> Option<Decimal> {
        let open_positions: Vec<_> = self.positions.values()
            .filter(|p| !p.is_flat())
            .collect();
        if open_positions.is_empty() { return None; }
        let mvs: Vec<Decimal> = open_positions.iter()
            .filter_map(|p| {
                prices.get(p.symbol.as_str())
                    .map(|&price| p.market_value(price).abs())
            })
            .collect();
        let total: Decimal = mvs.iter().sum();
        if total.is_zero() { return None; }
        Some(mvs.iter().map(|mv| {
            let w = mv / total;
            w * w
        }).sum())
    }

    /// Ratio of total long unrealized P&L to absolute total short unrealized P&L.
    ///
    /// Values > 1 mean longs are outperforming; values < 1 mean shorts are leading.
    /// Returns `None` if short PnL is zero or no short prices are available.
    pub fn long_short_pnl_ratio(&self, prices: &HashMap<String, Price>) -> Option<Decimal> {
        let long_pnl: Decimal = self.positions.values()
            .filter(|p| p.is_long())
            .filter_map(|p| prices.get(p.symbol.as_str()).map(|&pr| p.unrealized_pnl(pr)))
            .sum();
        let short_pnl: Decimal = self.positions.values()
            .filter(|p| p.is_short())
            .filter_map(|p| prices.get(p.symbol.as_str()).map(|&pr| p.unrealized_pnl(pr)))
            .sum();
        let short_abs = short_pnl.abs();
        if short_abs.is_zero() { return None; }
        Some(long_pnl / short_abs)
    }

    /// Unrealized P&L for each open position, keyed by symbol string.
    ///
    /// Positions absent from `prices` are omitted from the result.
    pub fn unrealized_pnl_by_symbol(&self, prices: &HashMap<String, Price>) -> HashMap<String, Decimal> {
        self.positions
            .iter()
            .filter(|(_, p)| !p.is_flat())
            .filter_map(|(sym, p)| {
                prices.get(sym.as_str())
                    .map(|&price| (sym.as_str().to_owned(), p.unrealized_pnl(price)))
            })
            .collect()
    }

    /// Portfolio-level beta: sum of (weight * beta) for each open position.
    ///
    /// `betas` maps symbol string to the symbol's beta coefficient.
    /// Positions with unknown beta or missing from `prices` are skipped.
    /// Returns `None` if total market value is zero or no betas are available.
    pub fn portfolio_beta(
        &self,
        prices: &HashMap<String, Price>,
        betas: &HashMap<String, f64>,
    ) -> Option<f64> {
        use rust_decimal::prelude::ToPrimitive;
        let open: Vec<&Position> = self.positions.values().filter(|p| !p.is_flat()).collect();
        if open.is_empty() { return None; }
        let total_mv: Decimal = open.iter()
            .filter_map(|p| prices.get(p.symbol.as_str()).map(|&pr| p.market_value(pr).abs()))
            .sum();
        if total_mv.is_zero() { return None; }
        let total_mv_f64 = total_mv.to_f64()?;
        let beta_sum: f64 = open.iter().filter_map(|p| {
            let mv = prices.get(p.symbol.as_str()).map(|&pr| p.market_value(pr).abs())?;
            let b = betas.get(p.symbol.as_str())?;
            let w = mv.to_f64()? / total_mv_f64;
            Some(w * b)
        }).sum();
        Some(beta_sum)
    }

    /// Returns the total notional value: sum of `|quantity| × price` for all open positions.
    ///
    /// Positions absent from `prices` are skipped. Returns `None` if no open positions
    /// have a matching price.
    pub fn total_notional(&self, prices: &HashMap<String, Price>) -> Option<Decimal> {
        let total: Decimal = self.positions.values()
            .filter(|p| !p.is_flat())
            .filter_map(|p| {
                prices.get(p.symbol.as_str())
                    .map(|&price| p.quantity_abs() * price.value())
            })
            .sum();
        if total.is_zero() { None } else { Some(total) }
    }

    /// Returns the largest unrealized gain (most positive unrealized P&L) among open positions.
    ///
    /// Returns `None` if no open positions have a matching price, or all unrealized P&Ls are
    /// non-positive.
    pub fn max_unrealized_pnl(&self, prices: &HashMap<String, Price>) -> Option<Decimal> {
        self.positions.values()
            .filter(|p| !p.is_flat())
            .filter_map(|p| {
                prices.get(p.symbol.as_str())
                    .map(|&price| p.unrealized_pnl(price))
            })
            .filter(|&pnl| pnl > Decimal::ZERO)
            .max()
    }

    /// Returns the 1-based rank (1 = best) of `symbol`'s realized P&L among all symbols
    /// that have non-zero realized P&L.
    ///
    /// Returns `None` if `symbol` has no realized P&L or if it is not found.
    pub fn realized_pnl_rank(&self, symbol: &Symbol) -> Option<usize> {
        let target = self.positions.get(symbol).map(|p| p.realized_pnl)?;
        if target == Decimal::ZERO { return None; }
        let mut sorted: Vec<Decimal> = self.positions.values()
            .map(|p| p.realized_pnl)
            .filter(|&r| r != Decimal::ZERO)
            .collect();
        sorted.sort_by(|a, b| b.cmp(a));
        sorted.iter().position(|&r| r == target).map(|i| i + 1)
    }

    /// Returns a `Vec` of references to all open (non-flat) positions, sorted by symbol.
    pub fn open_positions_vec(&self) -> Vec<&Position> {
        let mut open: Vec<&Position> = self.positions.values()
            .filter(|p| !p.is_flat())
            .collect();
        open.sort_by(|a, b| a.symbol.as_str().cmp(b.symbol.as_str()));
        open
    }

    /// Returns all symbols whose realized P&L strictly exceeds `threshold`.
    ///
    /// Results are sorted by realized P&L descending.
    pub fn symbols_with_pnl_above(&self, threshold: Decimal) -> Vec<Symbol> {
        let mut pairs: Vec<(Symbol, Decimal)> = self.positions.iter()
            .filter_map(|(sym, pos)| {
                if pos.realized_pnl > threshold { Some((sym.clone(), pos.realized_pnl)) } else { None }
            })
            .collect();
        pairs.sort_by(|a, b| b.1.cmp(&a.1));
        pairs.into_iter().map(|(s, _)| s).collect()
    }

    /// Returns `(long_count, short_count)` of currently open (non-flat) positions.
    pub fn net_long_short_count(&self) -> (usize, usize) {
        let long = self.positions.values().filter(|p| p.is_long()).count();
        let short = self.positions.values().filter(|p| p.is_short()).count();
        (long, short)
    }

    /// Returns the symbol of the open position with the largest absolute quantity.
    ///
    /// Returns `None` if there are no open positions.
    pub fn largest_open_position(&self) -> Option<&Symbol> {
        self.positions.iter()
            .filter(|(_, p)| !p.is_flat())
            .max_by(|(_, a), (_, b)| a.quantity.abs().cmp(&b.quantity.abs()))
            .map(|(sym, _)| sym)
    }

    /// Market exposure broken down by direction: `(long_exposure, short_exposure)`.
    ///
    /// Both values are positive (abs). Positions not in `prices` contribute zero.
    pub fn exposure_by_direction(&self, prices: &HashMap<String, Price>) -> (Decimal, Decimal) {
        let long: Decimal = self.positions.values()
            .filter(|p| p.is_long())
            .filter_map(|p| prices.get(p.symbol.as_str()).map(|&pr| p.market_value(pr)))
            .sum();
        let short: Decimal = self.positions.values()
            .filter(|p| p.is_short())
            .filter_map(|p| prices.get(p.symbol.as_str()).map(|&pr| p.market_value(pr).abs()))
            .sum();
        (long, short)
    }

    /// Returns the sum of realized P&L across all positions in this ledger.
    pub fn total_realized_pnl(&self) -> Decimal {
        self.positions.values().map(|p| p.realized_pnl).sum()
    }

    /// Returns the number of positions whose realized P&L is strictly below `threshold`.
    pub fn count_with_pnl_below(&self, threshold: Decimal) -> usize {
        self.positions.values().filter(|p| p.realized_pnl < threshold).count()
    }

    /// Returns `true` if the sum of all position quantities is positive (net long exposure).
    pub fn is_net_long(&self) -> bool {
        let net: Decimal = self.positions.values().map(|p| p.quantity).sum();
        net > Decimal::ZERO
    }

    /// Total unrealized P&L across all open positions that have a price available.
    ///
    /// Positions absent from `prices` contribute zero.
    pub fn total_unrealized_pnl(&self, prices: &HashMap<String, Price>) -> Decimal {
        self.positions.values()
            .filter(|p| !p.is_flat())
            .filter_map(|p| prices.get(p.symbol.as_str()).map(|&pr| p.unrealized_pnl(pr)))
            .sum()
    }

    /// Returns symbols that have a flat (zero-quantity) position in this ledger, sorted.
    pub fn symbols_flat(&self) -> Vec<&Symbol> {
        let mut flat: Vec<&Symbol> = self.positions.iter()
            .filter(|(_, p)| p.is_flat())
            .map(|(sym, _)| sym)
            .collect();
        flat.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        flat
    }

    /// Returns the average unrealized P&L percentage across all open positions.
    ///
    /// Each position's unrealized PnL % is `unrealized_pnl / (avg_price * qty).abs() * 100`.
    /// Returns `None` if there are no open positions with valid prices.
    pub fn avg_unrealized_pnl_pct(&self, prices: &HashMap<String, Price>) -> Option<Decimal> {
        let pcts: Vec<Decimal> = self.positions.values()
            .filter(|p| !p.is_flat())
            .filter_map(|p| {
                prices.get(p.symbol.as_str()).and_then(|&pr| {
                    let cost_basis = (p.avg_cost * p.quantity).abs();
                    if cost_basis.is_zero() { return None; }
                    Some(p.unrealized_pnl(pr) / cost_basis * Decimal::ONE_HUNDRED)
                })
            })
            .collect();
        if pcts.is_empty() { return None; }
        Some(pcts.iter().sum::<Decimal>() / Decimal::from(pcts.len()))
    }

    /// Returns the symbol with the worst (most negative) unrealized P&L.
    ///
    /// Returns `None` if there are no open positions or none have a price in `prices`.
    pub fn max_drawdown_symbol<'a>(&'a self, prices: &HashMap<String, Price>) -> Option<&'a Symbol> {
        self.positions.iter()
            .filter(|(_, p)| !p.is_flat())
            .filter_map(|(sym, p)| {
                prices.get(p.symbol.as_str())
                    .map(|&price| (sym, p.unrealized_pnl(price)))
            })
            .min_by(|(_, a), (_, b)| a.cmp(b))
            .map(|(sym, _)| sym)
    }

    /// Average unrealized P&L across all open positions that have a price in `prices`.
    ///
    /// Returns `None` if there are no open positions with prices available.
    pub fn avg_unrealized_pnl(&self, prices: &HashMap<String, Price>) -> Option<Decimal> {
        let pnls: Vec<Decimal> = self.positions.values()
            .filter(|p| !p.is_flat())
            .filter_map(|p| prices.get(p.symbol.as_str()).map(|&pr| p.unrealized_pnl(pr)))
            .collect();
        if pnls.is_empty() { return None; }
        #[allow(clippy::cast_possible_truncation)]
        Some(pnls.iter().sum::<Decimal>() / Decimal::from(pnls.len() as u32))
    }

    /// Returns a sorted `Vec` of all symbols tracked by this ledger (open or closed).
    pub fn position_symbols(&self) -> Vec<&Symbol> {
        let mut syms: Vec<&Symbol> = self.positions.keys().collect();
        syms.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        syms
    }

    /// Returns the count of positions with strictly positive realized P&L.
    pub fn count_profitable(&self) -> usize {
        self.positions.values().filter(|p| p.realized_pnl > Decimal::ZERO).count()
    }

    /// Returns the count of positions with strictly negative realized P&L.
    pub fn count_losing(&self) -> usize {
        self.positions.values().filter(|p| p.realized_pnl < Decimal::ZERO).count()
    }

    /// Returns the top `n` open positions by absolute notional exposure (`|qty * price|`),
    /// sorted descending. Positions without a price in `prices` are excluded.
    pub fn top_n_by_exposure<'a>(
        &'a self,
        prices: &HashMap<String, Price>,
        n: usize,
    ) -> Vec<(&'a Symbol, Decimal)> {
        let mut exposures: Vec<(&Symbol, Decimal)> = self.positions.iter()
            .filter(|(_, p)| !p.is_flat())
            .filter_map(|(sym, p)| {
                prices.get(p.symbol.as_str())
                    .map(|&pr| (sym, (p.quantity * pr.value()).abs()))
            })
            .collect();
        exposures.sort_by(|a, b| b.1.cmp(&a.1));
        exposures.truncate(n);
        exposures
    }

    /// Returns `true` if there is at least one non-flat position.
    pub fn has_open_positions(&self) -> bool {
        self.positions.values().any(|p| !p.is_flat())
    }

    /// Symbols with a strictly positive (long) quantity.
    pub fn long_symbols(&self) -> Vec<&Symbol> {
        self.positions.iter()
            .filter(|(_, p)| p.quantity > Decimal::ZERO)
            .map(|(sym, _)| sym)
            .collect()
    }

    /// Symbols with a strictly negative (short) quantity.
    pub fn short_symbols(&self) -> Vec<&Symbol> {
        self.positions.iter()
            .filter(|(_, p)| p.quantity < Decimal::ZERO)
            .map(|(sym, _)| sym)
            .collect()
    }

    /// Herfindahl-Hirschman Index of notional exposure: `Σ w_i²` where `w_i = |notional_i| / Σ|notional|`.
    ///
    /// Returns `1.0` (full concentration) for a single position.
    /// Returns `None` if there are no open positions with available prices.
    pub fn concentration_ratio(&self, prices: &HashMap<String, Price>) -> Option<f64> {
        use rust_decimal::prelude::ToPrimitive;
        let notionals: Vec<Decimal> = self.positions.values()
            .filter(|p| !p.is_flat())
            .filter_map(|p| {
                prices.get(p.symbol.as_str())
                    .map(|&pr| (p.quantity * pr.value()).abs())
            })
            .collect();
        if notionals.is_empty() { return None; }
        let total: Decimal = notionals.iter().sum();
        if total.is_zero() { return None; }
        let hhi: f64 = notionals.iter()
            .filter_map(|n| (n / total).to_f64())
            .map(|w| w * w)
            .sum();
        Some(hhi)
    }

    /// Minimum unrealized P&L across all open positions.
    ///
    /// Returns `None` if there are no open positions with a known price.
    pub fn min_unrealized_pnl(&self, prices: &HashMap<String, Price>) -> Option<Decimal> {
        self.positions.values()
            .filter(|p| !p.is_flat())
            .filter_map(|p| prices.get(p.symbol.as_str()).map(|&pr| p.unrealized_pnl(pr)))
            .min_by(|a, b| a.cmp(b))
    }

    /// Percentage of non-flat positions that are long (quantity > 0).
    ///
    /// Returns `None` if there are no open positions.
    pub fn pct_long(&self) -> Option<Decimal> {
        let open: Vec<&Position> = self.positions.values().filter(|p| !p.is_flat()).collect();
        if open.is_empty() { return None; }
        let longs = open.iter().filter(|p| p.quantity > Decimal::ZERO).count() as u32;
        Some(Decimal::from(longs) / Decimal::from(open.len() as u32) * Decimal::ONE_HUNDRED)
    }

    /// Percentage of non-flat positions that are short (quantity < 0).
    ///
    /// Returns `None` if there are no open positions.
    pub fn pct_short(&self) -> Option<Decimal> {
        let open: Vec<&Position> = self.positions.values().filter(|p| !p.is_flat()).collect();
        if open.is_empty() { return None; }
        let shorts = open.iter().filter(|p| p.quantity < Decimal::ZERO).count() as u32;
        Some(Decimal::from(shorts) / Decimal::from(open.len() as u32) * Decimal::ONE_HUNDRED)
    }

    /// Sum of absolute values of all realized P&L across positions.
    pub fn realized_pnl_total_abs(&self) -> Decimal {
        self.positions.values().map(|p| p.realized_pnl.abs()).sum()
    }

    /// Average entry price for a symbol's current position.
    ///
    /// Returns `None` if the symbol is not tracked or the position is flat.
    pub fn average_entry_price(&self, symbol: &Symbol) -> Option<Price> {
        self.positions.get(symbol)?.avg_entry_price()
    }

    /// Net sum of all position quantities across all symbols.
    pub fn net_quantity(&self) -> Decimal {
        self.positions.values().map(|p| p.quantity).sum()
    }

    /// Maximum notional exposure (`|qty * price|`) of any single long position.
    ///
    /// Returns `None` if no long positions have a price in `prices`.
    pub fn max_long_notional(&self, prices: &HashMap<String, Price>) -> Option<Decimal> {
        self.positions.values()
            .filter(|p| p.quantity > Decimal::ZERO)
            .filter_map(|p| {
                prices.get(p.symbol.as_str()).map(|&pr| (p.quantity * pr.value()).abs())
            })
            .max_by(|a, b| a.cmp(b))
    }

    /// Maximum notional exposure (`|qty * price|`) of any single short position.
    ///
    /// Returns `None` if no short positions have a price in `prices`.
    pub fn max_short_notional(&self, prices: &HashMap<String, Price>) -> Option<Decimal> {
        self.positions.values()
            .filter(|p| p.quantity < Decimal::ZERO)
            .filter_map(|p| {
                prices.get(p.symbol.as_str()).map(|&pr| (p.quantity * pr.value()).abs())
            })
            .max_by(|a, b| a.cmp(b))
    }

    /// Symbol with the highest realized P&L.
    ///
    /// Returns `None` if no positions have been tracked.
    pub fn max_realized_pnl(&self) -> Option<(&Symbol, Decimal)> {
        self.positions.iter()
            .map(|(sym, p)| (sym, p.realized_pnl))
            .max_by(|(_, a), (_, b)| a.cmp(b))
    }

    /// Symbol with the lowest (most negative) realized P&L.
    ///
    /// Returns `None` if no positions have been tracked.
    pub fn min_realized_pnl(&self) -> Option<(&Symbol, Decimal)> {
        self.positions.iter()
            .map(|(sym, p)| (sym, p.realized_pnl))
            .min_by(|(_, a), (_, b)| a.cmp(b))
    }

    /// Average holding duration in bars for all open positions.
    ///
    /// Uses `current_bar - p.open_bar` for each open position.
    /// Returns `None` if there are no open positions.
    pub fn avg_holding_bars(&self, current_bar: usize) -> Option<f64> {
        let open: Vec<usize> = self.positions.values()
            .filter(|p| !p.is_flat())
            .map(|p| current_bar.saturating_sub(p.open_bar))
            .collect();
        if open.is_empty() { return None; }
        Some(open.iter().sum::<usize>() as f64 / open.len() as f64)
    }

    /// Symbols of open positions that currently have a negative unrealized P&L.
    pub fn symbols_with_unrealized_loss(&self, prices: &HashMap<String, Price>) -> Vec<&Symbol> {
        self.positions.iter()
            .filter(|(_, p)| !p.is_flat())
            .filter_map(|(sym, p)| {
                prices.get(p.symbol.as_str())
                    .map(|&pr| (sym, p.unrealized_pnl(pr)))
            })
            .filter(|(_, pnl)| *pnl < Decimal::ZERO)
            .map(|(sym, _)| sym)
            .collect()
    }

    /// Volume-weighted average entry price across all open long positions. Returns `None` if
    /// there are no long positions.
    pub fn avg_long_entry_price(&self) -> Option<Decimal> {
        let longs: Vec<&Position> = self.positions.values()
            .filter(|p| p.is_long())
            .collect();
        if longs.is_empty() { return None; }
        let total_qty: Decimal = longs.iter().map(|p| p.quantity.abs()).sum();
        if total_qty.is_zero() { return None; }
        let weighted: Decimal = longs.iter().map(|p| p.avg_cost * p.quantity.abs()).sum();
        Some(weighted / total_qty)
    }

    /// Volume-weighted average entry price across all open short positions. Returns `None` if
    /// there are no short positions.
    pub fn avg_short_entry_price(&self) -> Option<Decimal> {
        let shorts: Vec<&Position> = self.positions.values()
            .filter(|p| p.is_short())
            .collect();
        if shorts.is_empty() { return None; }
        let total_qty: Decimal = shorts.iter().map(|p| p.quantity.abs()).sum();
        if total_qty.is_zero() { return None; }
        let weighted: Decimal = shorts.iter().map(|p| p.avg_cost * p.quantity.abs()).sum();
        Some(weighted / total_qty)
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
    fn test_position_ledger_pnl_by_symbol() {
        let mut ledger = PositionLedger::new(dec!(10000));
        ledger.apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0")).unwrap();
        ledger.apply_fill(make_fill("GOOG", Side::Bid, "5", "200", "0")).unwrap();
        let mut prices = HashMap::new();
        prices.insert("AAPL".to_owned(), Price::new(dec!(110)).unwrap());
        prices.insert("GOOG".to_owned(), Price::new(dec!(190)).unwrap());
        let pnl = ledger.pnl_by_symbol(&prices).unwrap();
        assert_eq!(*pnl.get(&sym("AAPL")).unwrap(), dec!(100));  // (110-100)*10
        assert_eq!(*pnl.get(&sym("GOOG")).unwrap(), dec!(-50));  // (190-200)*5
    }

    #[test]
    fn test_position_ledger_pnl_by_symbol_missing_price() {
        let mut ledger = PositionLedger::new(dec!(10000));
        ledger.apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0")).unwrap();
        let prices: HashMap<String, Price> = HashMap::new();
        assert!(ledger.pnl_by_symbol(&prices).is_err());
    }

    #[test]
    fn test_position_ledger_delta_neutral_no_positions() {
        let ledger = PositionLedger::new(dec!(10000));
        let prices: HashMap<String, Price> = HashMap::new();
        assert!(ledger.delta_neutral_check(&prices).unwrap());
    }

    #[test]
    fn test_position_ledger_delta_neutral_long_short_balanced() {
        let mut ledger = PositionLedger::new(dec!(10000));
        ledger.apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0")).unwrap();
        ledger.apply_fill(make_fill("GOOG", Side::Ask, "10", "100", "0")).unwrap();
        let mut prices = HashMap::new();
        prices.insert("AAPL".to_owned(), Price::new(dec!(100)).unwrap());
        prices.insert("GOOG".to_owned(), Price::new(dec!(100)).unwrap());
        // net=0, gross=2000 → ratio=0 → neutral
        assert!(ledger.delta_neutral_check(&prices).unwrap());
    }

    #[test]
    fn test_position_ledger_delta_neutral_one_sided_not_neutral() {
        let mut ledger = PositionLedger::new(dec!(10000));
        ledger.apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0")).unwrap();
        let mut prices = HashMap::new();
        prices.insert("AAPL".to_owned(), Price::new(dec!(100)).unwrap());
        // net=1000, gross=1000 → ratio=1 → not neutral
        assert!(!ledger.delta_neutral_check(&prices).unwrap());
    }

    #[test]
    fn test_position_ledger_open_count_zero_when_empty() {
        assert_eq!(PositionLedger::new(dec!(10000)).open_count(), 0);
    }

    #[test]
    fn test_position_ledger_open_count_tracks_positions() {
        let mut ledger = PositionLedger::new(dec!(10000));
        ledger.apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0")).unwrap();
        assert_eq!(ledger.open_count(), 1);
        ledger.apply_fill(make_fill("GOOG", Side::Bid, "5", "200", "0")).unwrap();
        assert_eq!(ledger.open_count(), 2);
        // close AAPL fully
        ledger.apply_fill(make_fill("AAPL", Side::Ask, "10", "105", "0")).unwrap();
        assert_eq!(ledger.open_count(), 1);
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

    #[test]
    fn test_position_ledger_total_long_exposure() {
        let mut ledger = PositionLedger::new(dec!(100000));
        ledger.apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0")).unwrap();
        // 10 * avg_cost(100) = 1000
        assert_eq!(ledger.total_long_exposure(), dec!(1000));
    }

    #[test]
    fn test_position_ledger_total_long_exposure_zero_when_flat() {
        let ledger = PositionLedger::new(dec!(10000));
        assert_eq!(ledger.total_long_exposure(), dec!(0));
    }

    #[test]
    fn test_position_ledger_total_short_exposure_zero_when_no_shorts() {
        let mut ledger = PositionLedger::new(dec!(100000));
        ledger.apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0")).unwrap();
        assert_eq!(ledger.total_short_exposure(), dec!(0));
    }

    #[test]
    fn test_allocation_pct_single_position() {
        let mut ledger = PositionLedger::new(dec!(100000));
        ledger.apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0")).unwrap();
        let mut prices = HashMap::new();
        let sym = Symbol::new("AAPL").unwrap();
        prices.insert("AAPL".to_string(), Price::new(dec!(100)).unwrap());
        let pct = ledger.allocation_pct(&sym, &prices).unwrap();
        // 10 shares * $100 / ($1000 total) = 100%
        assert_eq!(pct, Some(dec!(100)));
    }

    #[test]
    fn test_allocation_pct_flat_position_returns_none() {
        let ledger = PositionLedger::new(dec!(100000));
        let mut prices = HashMap::new();
        let sym = Symbol::new("AAPL").unwrap();
        prices.insert("AAPL".to_string(), Price::new(dec!(100)).unwrap());
        // No fill → no position in ledger → error
        assert!(ledger.allocation_pct(&sym, &prices).is_err());
    }

    #[test]
    fn test_positions_sorted_by_pnl_descending() {
        let mut ledger = PositionLedger::new(dec!(100000));
        ledger.apply_fill(make_fill("AAPL", Side::Bid, "1", "100", "0")).unwrap();
        ledger.apply_fill(make_fill("GOOG", Side::Bid, "1", "200", "0")).unwrap();
        let mut prices = HashMap::new();
        // AAPL gained $10, GOOG gained $50
        prices.insert("AAPL".to_string(), Price::new(dec!(110)).unwrap());
        prices.insert("GOOG".to_string(), Price::new(dec!(250)).unwrap());
        let sorted = ledger.positions_sorted_by_pnl(&prices);
        // GOOG (pnl=50) should come before AAPL (pnl=10)
        assert_eq!(sorted[0].symbol.as_str(), "GOOG");
        assert_eq!(sorted[1].symbol.as_str(), "AAPL");
    }

    #[test]
    fn test_positions_sorted_by_pnl_empty_when_all_flat() {
        let ledger = PositionLedger::new(dec!(100000));
        let prices = HashMap::new();
        assert!(ledger.positions_sorted_by_pnl(&prices).is_empty());
    }

    #[test]
    fn test_all_flat_initially() {
        let ledger = PositionLedger::new(dec!(100000));
        assert!(ledger.all_flat());
    }

    #[test]
    fn test_all_flat_false_after_open_position() {
        let mut ledger = PositionLedger::new(dec!(100000));
        ledger.apply_fill(make_fill("AAPL", Side::Bid, "10", "150", "0"));
        assert!(!ledger.all_flat());
    }

    #[test]
    fn test_all_flat_true_after_close_position() {
        let mut ledger = PositionLedger::new(dec!(100000));
        ledger.apply_fill(make_fill("AAPL", Side::Bid, "10", "150", "0"));
        ledger.apply_fill(make_fill("AAPL", Side::Ask, "10", "155", "0"));
        assert!(ledger.all_flat());
    }

    #[test]
    fn test_concentration_pct_single_position() {
        let mut ledger = PositionLedger::new(dec!(100000));
        ledger.apply_fill(make_fill("AAPL", Side::Bid, "10", "150", "0"));
        let sym = Symbol::new("AAPL").unwrap();
        let mut prices = HashMap::new();
        prices.insert("AAPL".to_string(), Price::new(dec!(150)).unwrap());
        // Only one position so concentration = 100%
        let pct = ledger.concentration_pct(&sym, &prices).unwrap();
        assert_eq!(pct, dec!(100));
    }

    #[test]
    fn test_concentration_pct_two_equal_positions() {
        let mut ledger = PositionLedger::new(dec!(100000));
        ledger.apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0"));
        ledger.apply_fill(make_fill("GOOG", Side::Bid, "10", "100", "0"));
        let sym = Symbol::new("AAPL").unwrap();
        let mut prices = HashMap::new();
        prices.insert("AAPL".to_string(), Price::new(dec!(100)).unwrap());
        prices.insert("GOOG".to_string(), Price::new(dec!(100)).unwrap());
        let pct = ledger.concentration_pct(&sym, &prices).unwrap();
        assert_eq!(pct, dec!(50));
    }

    #[test]
    fn test_concentration_pct_missing_price_returns_none() {
        let mut ledger = PositionLedger::new(dec!(100000));
        ledger.apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0"));
        let sym = Symbol::new("AAPL").unwrap();
        let prices = HashMap::new(); // empty price map
        assert!(ledger.concentration_pct(&sym, &prices).is_none());
    }

    #[test]
    fn test_avg_realized_pnl_per_symbol_none_when_empty() {
        let ledger = PositionLedger::new(dec!(100000));
        assert!(ledger.avg_realized_pnl_per_symbol().is_none());
    }

    #[test]
    fn test_avg_realized_pnl_per_symbol_with_closed_trade() {
        let mut ledger = PositionLedger::new(dec!(100000));
        // Buy 10 @ 100, sell 10 @ 110 → realized = +100
        ledger.apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0"));
        ledger.apply_fill(make_fill("AAPL", Side::Ask, "10", "110", "0"));
        let avg = ledger.avg_realized_pnl_per_symbol().unwrap();
        assert_eq!(avg, dec!(100));
    }

    #[test]
    fn test_net_exposure_no_prices_returns_none() {
        let mut ledger = PositionLedger::new(dec!(100000));
        ledger.apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0"));
        let prices = HashMap::new();
        assert!(ledger.net_market_exposure(&prices).is_none());
    }

    #[test]
    fn test_net_exposure_long_only() {
        let mut ledger = PositionLedger::new(dec!(100000));
        ledger.apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0"));
        let mut prices = HashMap::new();
        prices.insert("AAPL".to_string(), Price::new(dec!(110)).unwrap());
        assert_eq!(ledger.net_market_exposure(&prices).unwrap(), dec!(1100));
    }

    #[test]
    fn test_win_rate_none_when_empty() {
        let ledger = PositionLedger::new(dec!(100000));
        assert!(ledger.win_rate().is_none());
    }

    #[test]
    fn test_win_rate_one_winner() {
        let mut ledger = PositionLedger::new(dec!(100000));
        // Buy and sell AAPL for +100 realized
        ledger.apply_fill(make_fill("AAPL", Side::Bid, "10", "100", "0"));
        ledger.apply_fill(make_fill("AAPL", Side::Ask, "10", "110", "0"));
        // GOOG still open at cost (realized=0)
        ledger.apply_fill(make_fill("GOOG", Side::Bid, "10", "100", "0"));
        let rate = ledger.win_rate().unwrap();
        // 1 winner (AAPL) out of 2 positions = 50%
        assert_eq!(rate, dec!(50));
    }
}
