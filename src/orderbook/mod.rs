//! # Module: orderbook
//!
//! ## Responsibility
//! Maintains a level-2 order book for a single symbol. Processes incremental
//! `BookDelta` updates with sequence-number validation, and provides best bid/ask,
//! spread, VWAP-to-fill, and top-N level queries.
//!
//! ## Guarantees
//! - Sequence numbers are validated: each delta must be exactly `self.sequence + 1`
//! - Bids are maintained in descending price order (best bid = highest price)
//! - Asks are maintained in ascending price order (best ask = lowest price)
//! - `vwap_for_qty` returns `InsufficientLiquidity` when the book cannot fill `qty`
//! - Thread-safe: `OrderBook` implements neither `Send` nor `Sync` by default (use `Arc<Mutex>` externally)
//!
//! ## NOT Responsible For
//! - Cross-symbol aggregation
//! - Persistence

use crate::error::FinError;
use crate::types::{Price, Quantity, Side, Symbol};
use rust_decimal::Decimal;
use std::collections::BTreeMap;

/// A single price level in the order book.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PriceLevel {
    /// The price of this level.
    pub price: Price,
    /// The resting quantity at this price.
    pub quantity: Quantity,
}

/// Whether a delta sets or removes a price level.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum DeltaAction {
    /// Set the quantity at this price level.
    Set,
    /// Remove this price level entirely.
    Remove,
}

/// An incremental update to an order book.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BookDelta {
    /// Which side of the book this update applies to.
    pub side: Side,
    /// The price level being updated.
    pub price: Price,
    /// The new quantity (used for `Set`; ignored for `Remove`).
    pub quantity: Quantity,
    /// The action to take.
    pub action: DeltaAction,
    /// Must equal `book.sequence() + 1`.
    pub sequence: u64,
}

/// A level-2 order book for a single symbol.
#[derive(Debug, Clone)]
pub struct OrderBook {
    /// The instrument this book tracks.
    pub symbol: Symbol,
    /// Bid levels: price → quantity. Iterated in ascending key order by `BTreeMap`;
    /// we use `.iter().rev()` to get descending (best bid first).
    bids: BTreeMap<Decimal, Decimal>,
    /// Ask levels: price → quantity. Iterated in ascending key order (best ask first).
    asks: BTreeMap<Decimal, Decimal>,
    /// Last successfully applied sequence number.
    sequence: u64,
}

impl OrderBook {
    /// Constructs a new empty `OrderBook` for `symbol`. Sequence starts at 0.
    pub fn new(symbol: Symbol) -> Self {
        Self {
            symbol,
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            sequence: 0,
        }
    }

    /// Applies a `BookDelta` to the order book.
    ///
    /// # Errors
    /// Returns [`FinError::SequenceMismatch`] if `delta.sequence != self.sequence + 1`.
    #[allow(clippy::needless_pass_by_value)]
    pub fn apply_delta(&mut self, delta: BookDelta) -> Result<(), FinError> {
        let expected = self.sequence + 1;
        if delta.sequence != expected {
            return Err(FinError::SequenceMismatch {
                expected,
                got: delta.sequence,
            });
        }
        // Save the pre-mutation value for potential rollback of a Remove action.
        let prev_val = match delta.side {
            Side::Bid => self.bids.get(&delta.price.value()).copied(),
            Side::Ask => self.asks.get(&delta.price.value()).copied(),
        };

        let book_side = match delta.side {
            Side::Bid => &mut self.bids,
            Side::Ask => &mut self.asks,
        };
        match delta.action {
            DeltaAction::Set => {
                book_side.insert(delta.price.value(), delta.quantity.value());
            }
            DeltaAction::Remove => {
                book_side.remove(&delta.price.value());
            }
        }
        self.sequence = delta.sequence;

        // Guard against inverted spreads that would corrupt VWAP and mid-price.
        // Copy the prices out before any mutable borrow.
        let maybe_inversion = {
            let best_bid_p = self.bids.keys().next_back().copied();
            let best_ask_p = self.asks.keys().next().copied();
            match (best_bid_p, best_ask_p) {
                (Some(b), Some(a)) if b >= a => Some((b, a)),
                _ => None,
            }
        };
        if let Some((best_bid_p, best_ask_p)) = maybe_inversion {
            // Roll back the mutation to keep the book consistent.
            match delta.action {
                DeltaAction::Set => match delta.side {
                    Side::Bid => {
                        self.bids.remove(&delta.price.value());
                    }
                    Side::Ask => {
                        self.asks.remove(&delta.price.value());
                    }
                },
                // Restore the level to its prior quantity (not delta.quantity, which is
                // zero by convention for Remove deltas and would corrupt the book).
                DeltaAction::Remove => match delta.side {
                    Side::Bid => {
                        if let Some(qty) = prev_val {
                            self.bids.insert(delta.price.value(), qty);
                        }
                    }
                    Side::Ask => {
                        if let Some(qty) = prev_val {
                            self.asks.insert(delta.price.value(), qty);
                        }
                    }
                },
            }
            self.sequence = expected - 1;
            return Err(FinError::InvertedSpread {
                best_bid: best_bid_p,
                best_ask: best_ask_p,
            });
        }

        Ok(())
    }

    /// Returns the best bid (highest price) or `None` if the bid side is empty.
    ///
    /// Returns `None` if the book is empty or if the stored price is somehow
    /// non-positive (which is structurally prevented by `apply_delta`).
    pub fn best_bid(&self) -> Option<PriceLevel> {
        self.bids.iter().next_back().and_then(|(p, q)| {
            Some(PriceLevel {
                price: Price::new(*p).ok()?,
                quantity: Quantity::new(*q).unwrap_or_else(|_| Quantity::zero()),
            })
        })
    }

    /// Returns `(best_bid, best_ask)` as a tuple, or `None` if either side is empty.
    ///
    /// Convenience wrapper for accessing both sides of the top-of-book in one call.
    pub fn best_quote(&self) -> Option<(PriceLevel, PriceLevel)> {
        Some((self.best_bid()?, self.best_ask()?))
    }

    /// Returns the best ask (lowest price) or `None` if the ask side is empty.
    ///
    /// Returns `None` if the book is empty or if the stored price is somehow
    /// non-positive (which is structurally prevented by `apply_delta`).
    pub fn best_ask(&self) -> Option<PriceLevel> {
        self.asks.iter().next().and_then(|(p, q)| {
            Some(PriceLevel {
                price: Price::new(*p).ok()?,
                quantity: Quantity::new(*q).unwrap_or_else(|_| Quantity::zero()),
            })
        })
    }

    /// Returns the mid-price `(best_ask + best_bid) / 2`, or `None` if either side is empty.
    pub fn mid_price(&self) -> Option<Decimal> {
        let bid = self.best_bid()?.price.value();
        let ask = self.best_ask()?.price.value();
        Some((bid + ask) / Decimal::TWO)
    }

    /// Returns the spread `best_ask - best_bid`, or `None` if either side is empty.
    pub fn spread(&self) -> Option<Decimal> {
        let bid = self.best_bid()?.price.value();
        let ask = self.best_ask()?.price.value();
        Some(ask - bid)
    }

    /// Returns the spread as a percentage of the mid-price: `spread / mid * 100`.
    ///
    /// Returns `None` when either side is empty or mid-price is zero.
    pub fn spread_pct(&self) -> Option<Decimal> {
        let mid = self.mid_price()?;
        if mid.is_zero() {
            return None;
        }
        let spread = self.spread()?;
        Some(spread / mid * Decimal::ONE_HUNDRED)
    }

    /// Returns the resting quantity at a specific price level, or `None` if the level is absent.
    pub fn depth_at(&self, side: Side, price: Price) -> Option<Decimal> {
        let key = price.value();
        match side {
            Side::Bid => self.bids.get(&key).copied(),
            Side::Ask => self.asks.get(&key).copied(),
        }
    }

    /// Returns the top `n` bid levels in descending price order.
    pub fn top_bids(&self, n: usize) -> Vec<PriceLevel> {
        self.bids
            .iter()
            .rev()
            .take(n)
            .filter_map(|(p, q)| {
                let price = Price::new(*p).ok()?;
                let quantity = Quantity::new(*q).ok()?;
                Some(PriceLevel { price, quantity })
            })
            .collect()
    }

    /// Returns the top `n` ask levels in ascending price order.
    pub fn top_asks(&self, n: usize) -> Vec<PriceLevel> {
        self.asks
            .iter()
            .take(n)
            .filter_map(|(p, q)| {
                let price = Price::new(*p).ok()?;
                let quantity = Quantity::new(*q).ok()?;
                Some(PriceLevel { price, quantity })
            })
            .collect()
    }

    /// Computes the volume-weighted average price to fill `qty` on `side`.
    ///
    /// Walks levels from best to worst until `qty` is filled.
    ///
    /// # Errors
    /// Returns [`FinError::InsufficientLiquidity`] if the book cannot fill `qty`.
    pub fn vwap_for_qty(&self, side: Side, qty: Quantity) -> Result<Decimal, FinError> {
        let target = qty.value();
        if target <= Decimal::ZERO {
            return Ok(Decimal::ZERO);
        }
        match side {
            Side::Bid => Self::vwap_fill(self.bids.iter().rev(), target),
            Side::Ask => Self::vwap_fill(self.asks.iter(), target),
        }
    }

    fn vwap_fill<'a>(
        levels: impl Iterator<Item = (&'a Decimal, &'a Decimal)>,
        target: Decimal,
    ) -> Result<Decimal, FinError> {
        let mut remaining = target;
        let mut total_cost = Decimal::ZERO;

        for (price, avail_qty) in levels {
            let fill = remaining.min(*avail_qty);
            total_cost += fill * price;
            remaining -= fill;
            if remaining <= Decimal::ZERO {
                break;
            }
        }

        if remaining > Decimal::ZERO {
            return Err(FinError::InsufficientLiquidity(target));
        }

        Ok(total_cost / target)
    }

    /// Returns the last successfully applied sequence number.
    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Returns the top `n` bid and ask levels as a snapshot.
    ///
    /// Returns `(bids, asks)` where bids are in descending price order and
    /// asks are in ascending price order.
    pub fn snapshot(&self, n: usize) -> (Vec<PriceLevel>, Vec<PriceLevel>) {
        (self.top_bids(n), self.top_asks(n))
    }

    /// Returns the number of bid price levels.
    pub fn bid_count(&self) -> usize {
        self.bids.len()
    }

    /// Returns the number of ask price levels.
    pub fn ask_count(&self) -> usize {
        self.asks.len()
    }

    /// Returns the number of price levels on the given `side`.
    pub fn level_count(&self, side: Side) -> usize {
        match side {
            Side::Bid => self.bids.len(),
            Side::Ask => self.asks.len(),
        }
    }

    /// Removes all price levels from both sides of the book, resetting sequence to 0.
    pub fn clear(&mut self) {
        self.bids.clear();
        self.asks.clear();
        self.sequence = 0;
    }

    /// Removes all resting levels from `side`, leaving the opposite side intact.
    ///
    /// Useful when a snapshot update arrives for one side only (e.g., bid-side snapshot).
    pub fn remove_all(&mut self, side: crate::types::Side) {
        use crate::types::Side;
        match side {
            Side::Bid => self.bids.clear(),
            Side::Ask => self.asks.clear(),
        }
    }

    /// Returns `true` if the book is currently in a crossed (inverted) state.
    ///
    /// A book is crossed when `best_bid >= best_ask`. Under normal operation this
    /// is always `false` since `apply_delta` rejects crossing deltas.
    /// Provided for diagnostic / assertion use.
    pub fn is_crossed(&self) -> bool {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => bid.price >= ask.price,
            _ => false,
        }
    }

    /// Returns `true` if both sides of the book have no resting quantity.
    pub fn is_empty(&self) -> bool {
        self.bids.is_empty() && self.asks.is_empty()
    }

    /// Returns the total number of distinct price levels across both sides.
    pub fn total_levels(&self) -> usize {
        self.bids.len() + self.asks.len()
    }

    /// Returns the total resting quantity available on `side` up to and including `price`.
    ///
    /// For bids: sums all bid levels at prices `>= price` (levels at or above the given price).
    /// For asks: sums all ask levels at prices `<= price` (levels at or below the given price).
    ///
    /// Returns `Decimal::ZERO` when there are no matching levels.
    pub fn cumulative_depth(&self, side: Side, price: Price) -> Decimal {
        let p = price.value();
        match side {
            Side::Bid => self
                .bids
                .range(p..)
                .map(|(_, qty)| *qty)
                .sum(),
            Side::Ask => self
                .asks
                .range(..=p)
                .map(|(_, qty)| *qty)
                .sum(),
        }
    }

    /// Returns the total resting quantity on the bid side.
    pub fn total_bid_volume(&self) -> Decimal {
        self.bids.values().copied().sum()
    }

    /// Returns the total resting quantity on the ask side.
    pub fn total_ask_volume(&self) -> Decimal {
        self.asks.values().copied().sum()
    }

    /// Returns the best bid price, or `None` if the bid side is empty.
    pub fn best_bid_price(&self) -> Option<Price> {
        self.bids.keys().next_back().and_then(|p| Price::new(*p).ok())
    }

    /// Returns the best ask price, or `None` if the ask side is empty.
    pub fn best_ask_price(&self) -> Option<Price> {
        self.asks.keys().next().and_then(|p| Price::new(*p).ok())
    }

    /// Returns the resting quantity at the best bid, or `None` if the bid side is empty.
    pub fn best_bid_qty(&self) -> Option<Quantity> {
        self.bids
            .values()
            .next_back()
            .and_then(|q| Quantity::new(*q).ok())
    }

    /// Returns the resting quantity at the best ask, or `None` if the ask side is empty.
    pub fn best_ask_qty(&self) -> Option<Quantity> {
        self.asks
            .values()
            .next()
            .and_then(|q| Quantity::new(*q).ok())
    }

    /// Returns the total resting quantity on `side` within `pct_from_mid` percent of the mid-price.
    ///
    /// For example, `liquidity_at_pct(Side::Ask, dec!(0.5))` returns all ask volume
    /// within 0.5% above the mid-price. Returns `None` when the book has no mid-price.
    pub fn liquidity_at_pct(&self, side: Side, pct_from_mid: Decimal) -> Option<Decimal> {
        let mid = self.mid_price()?;
        let band = mid * pct_from_mid / Decimal::ONE_HUNDRED;
        let (lo, hi) = match side {
            Side::Bid => (mid - band, mid),
            Side::Ask => (mid, mid + band),
        };
        let qty: Decimal = match side {
            Side::Bid => self
                .bids
                .range(lo..=hi)
                .map(|(_, q)| *q)
                .sum(),
            Side::Ask => self
                .asks
                .range(lo..=hi)
                .map(|(_, q)| *q)
                .sum(),
        };
        Some(qty)
    }

    /// Returns `true` if `price` is present in the given `side` of the book.
    pub fn has_price(&self, side: Side, price: Price) -> bool {
        let key = price.value();
        match side {
            Side::Bid => self.bids.contains_key(&key),
            Side::Ask => self.asks.contains_key(&key),
        }
    }

    /// Returns the quantity-weighted midpoint (micro-price).
    ///
    /// Weights best-bid by ask quantity and best-ask by bid quantity:
    /// `(bid_price × ask_qty + ask_price × bid_qty) / (bid_qty + ask_qty)`.
    /// Returns `None` when either side is empty.
    pub fn weighted_mid(&self) -> Option<Decimal> {
        let bid = self.best_bid()?;
        let ask = self.best_ask()?;
        let bid_qty = bid.quantity.value();
        let ask_qty = ask.quantity.value();
        let total = bid_qty + ask_qty;
        if total.is_zero() {
            return None;
        }
        Some((bid.price.value() * ask_qty + ask.price.value() * bid_qty) / total)
    }

    /// Returns the order-book imbalance: `(bid_vol - ask_vol) / (bid_vol + ask_vol)`.
    ///
    /// Returns `None` when both sides are empty (division by zero).
    /// Range is `(-1, 1)`: positive = bid-heavy, negative = ask-heavy.
    pub fn imbalance(&self) -> Option<Decimal> {
        let bid_vol = self.total_bid_volume();
        let ask_vol = self.total_ask_volume();
        let total = bid_vol + ask_vol;
        if total == Decimal::ZERO {
            return None;
        }
        Some((bid_vol - ask_vol) / total)
    }

    /// Returns the depth ratio `top_n_bid_vol / top_n_ask_vol` for the best `n` levels.
    ///
    /// A ratio > 1 indicates more buying pressure at the top of book; < 1 more selling pressure.
    /// Returns `None` when either side has no levels in the top-`n` or ask volume is zero.
    pub fn depth_ratio(&self, n: usize) -> Option<Decimal> {
        let bid_vol: Decimal = self.bids.values().rev().take(n).copied().sum();
        let ask_vol: Decimal = self.asks.values().take(n).copied().sum();
        if ask_vol.is_zero() {
            return None;
        }
        Some(bid_vol / ask_vol)
    }

    /// Returns the weighted mid price: `(best_bid * ask_qty + best_ask * bid_qty) / (bid_qty + ask_qty)`.
    ///
    /// Weights the midpoint by the opposite side's quantity, so a thick ask pulls the WMP toward bid.
    /// Returns `None` when either side is empty.
    pub fn weighted_mid_price(&self) -> Option<Decimal> {
        let (bid_p, bid_q) = self.bids.iter().next_back()?;
        let (ask_p, ask_q) = self.asks.iter().next()?;
        let total_q = bid_q + ask_q;
        if total_q.is_zero() {
            return None;
        }
        Some((*bid_p * *ask_q + *ask_p * *bid_q) / total_q)
    }

    /// Returns all price levels on `side` whose price falls within `[lo, hi]` (inclusive).
    ///
    /// Useful for computing the available liquidity within a price band.
    pub fn price_levels_between(&self, side: Side, lo: Price, hi: Price) -> Vec<PriceLevel> {
        let lo_val = lo.value();
        let hi_val = hi.value();
        match side {
            Side::Bid => self
                .bids
                .range(lo_val..=hi_val)
                .map(|(p, q)| PriceLevel {
                    price: Price::new(*p).unwrap_or(lo),
                    quantity: crate::types::Quantity::new(*q).unwrap_or_else(|_| crate::types::Quantity::zero()),
                })
                .collect(),
            Side::Ask => self
                .asks
                .range(lo_val..=hi_val)
                .map(|(p, q)| PriceLevel {
                    price: Price::new(*p).unwrap_or(lo),
                    quantity: crate::types::Quantity::new(*q).unwrap_or_else(|_| crate::types::Quantity::zero()),
                })
                .collect(),
        }
    }

    /// Returns the smallest price increment between adjacent levels on either side.
    ///
    /// Useful for estimating the instrument's native tick size from live book data.
    /// Returns `None` when both sides have fewer than 2 levels.
    pub fn tick_size(&self) -> Option<Decimal> {
        let bid_tick = self
            .bids
            .keys()
            .collect::<Vec<_>>()
            .windows(2)
            .map(|w| (*w[1] - *w[0]).abs())
            .filter(|d| !d.is_zero())
            .reduce(Decimal::min);
        let ask_tick = self
            .asks
            .keys()
            .collect::<Vec<_>>()
            .windows(2)
            .map(|w| (*w[1] - *w[0]).abs())
            .filter(|d| !d.is_zero())
            .reduce(Decimal::min);
        match (bid_tick, ask_tick) {
            (Some(b), Some(a)) => Some(b.min(a)),
            (Some(b), None) => Some(b),
            (None, Some(a)) => Some(a),
            (None, None) => None,
        }
    }

    /// Returns the bid-to-ask volume ratio: `total_bid_volume / total_ask_volume`.
    ///
    /// Values > 1 indicate more buy-side depth; values < 1 indicate more sell-side depth.
    /// Returns `None` if either side is empty (to avoid division by zero).
    pub fn bid_ask_ratio(&self) -> Option<Decimal> {
        let bid = self.total_bid_volume();
        let ask = self.total_ask_volume();
        if ask.is_zero() || bid.is_zero() {
            return None;
        }
        Some(bid / ask)
    }

    /// Estimates the average fill price for a market order of `qty` on `side`.
    ///
    /// Walks the book levels in price-time priority and returns the volume-weighted
    /// average price. Returns `None` if `qty` is zero or the book cannot fill `qty`
    /// in full (insufficient depth).
    pub fn price_impact(&self, side: crate::types::Side, qty: crate::types::Quantity) -> Option<Decimal> {
        use crate::types::Side;
        if qty.is_zero() {
            return None;
        }
        let levels: Vec<_> = match side {
            Side::Bid => {
                // Buying: walk asks from lowest to highest price
                let mut asks: Vec<_> = self.asks.iter().collect();
                asks.sort_by(|a, b| a.0.cmp(b.0));
                asks.into_iter().map(|(p, q)| (*p, *q)).collect()
            }
            Side::Ask => {
                // Selling: walk bids from highest to lowest price
                let mut bids: Vec<_> = self.bids.iter().collect();
                bids.sort_by(|a, b| b.0.cmp(a.0));
                bids.into_iter().map(|(p, q)| (*p, *q)).collect()
            }
        };
        let target = qty.value();
        let mut remaining = target;
        let mut notional = Decimal::ZERO;
        for (price, level_qty) in levels {
            let fill = level_qty.min(remaining);
            notional += price * fill;
            remaining -= fill;
            if remaining <= Decimal::ZERO {
                break;
            }
        }
        if remaining > Decimal::ZERO {
            None // insufficient depth
        } else {
            Some(notional / target)
        }
    }

    /// Returns the top `n` bid levels in descending price order (best bid first).
    ///
    /// Returns fewer than `n` levels if the bid side has fewer entries.
    pub fn bid_depth(&self, n: usize) -> Vec<PriceLevel> {
        self.bids
            .iter()
            .rev()
            .take(n)
            .map(|(price, qty)| PriceLevel {
                price: Price::new(*price).unwrap(),
                quantity: Quantity::new(*qty).unwrap(),
            })
            .collect()
    }

    /// Returns the top `n` ask levels in ascending price order (best ask first).
    ///
    /// Returns fewer than `n` levels if the ask side has fewer entries.
    pub fn ask_depth(&self, n: usize) -> Vec<PriceLevel> {
        self.asks
            .iter()
            .take(n)
            .map(|(price, qty)| PriceLevel {
                price: Price::new(*price).unwrap(),
                quantity: Quantity::new(*qty).unwrap(),
            })
            .collect()
    }

    /// Returns the depth imbalance ratio: `(bid_qty - ask_qty) / (bid_qty + ask_qty)`.
    ///
    /// Result is in `[-1.0, 1.0]`:
    /// - Positive → more bid-side depth (buying pressure)
    /// - Negative → more ask-side depth (selling pressure)
    /// - `None` when both sides are empty (total depth is zero)
    pub fn depth_imbalance(&self) -> Option<Decimal> {
        let bid_qty: Decimal = self.bids.values().sum();
        let ask_qty: Decimal = self.asks.values().sum();
        let total = bid_qty + ask_qty;
        if total.is_zero() {
            return None;
        }
        Some((bid_qty - ask_qty) / total)
    }

    /// Returns the ask-to-bid quantity ratio: `total_ask_qty / total_bid_qty`.
    ///
    /// Values above 1 indicate more supply than demand at visible depth levels.
    /// Returns `None` when total bid quantity is zero (avoid division by zero).
    pub fn ask_bid_ratio(&self) -> Option<Decimal> {
        let bid_qty: Decimal = self.bids.values().sum();
        let ask_qty: Decimal = self.asks.values().sum();
        if bid_qty.is_zero() {
            return None;
        }
        Some(ask_qty / bid_qty)
    }

    /// Returns the total quantity across all bid price levels.
    pub fn total_bid_depth(&self) -> Decimal {
        self.bids.values().sum()
    }

    /// Returns the total quantity across all ask price levels.
    pub fn total_ask_depth(&self) -> Decimal {
        self.asks.values().sum()
    }

    /// Walks the book on `side` to find the price level reached after consuming `target_qty`.
    ///
    /// For `Side::Ask` walks ascending (cheapest ask first).
    /// For `Side::Bid` walks descending (highest bid first).
    ///
    /// Returns the price of the level where `target_qty` is fully absorbed, or the last
    /// available level if the book lacks sufficient depth.
    /// Returns `None` when the side has no levels or `target_qty` is zero.
    pub fn price_at_volume(&self, side: Side, target_qty: Decimal) -> Option<Price> {
        if target_qty.is_zero() {
            return None;
        }
        let mut remaining = target_qty;
        let mut last_price: Option<Price> = None;

        match side {
            Side::Ask => {
                for (&px, &qty) in &self.asks {
                    last_price = Price::new(px).ok();
                    if qty >= remaining {
                        return last_price;
                    }
                    remaining -= qty;
                }
            }
            Side::Bid => {
                for (&px, &qty) in self.bids.iter().rev() {
                    last_price = Price::new(px).ok();
                    if qty >= remaining {
                        return last_price;
                    }
                    remaining -= qty;
                }
            }
        }
        last_price
    }

    /// Returns up to `n` best bid levels in descending price order (best bid first).
    ///
    /// Returns an empty `Vec` when the bid side is empty or `n == 0`.
    pub fn top_n_bid_levels(&self, n: usize) -> Vec<PriceLevel> {
        if n == 0 {
            return vec![];
        }
        self.bids
            .iter()
            .rev()
            .take(n)
            .filter_map(|(&px, &qty)| {
                let price = Price::new(px).ok()?;
                let quantity = Quantity::new(qty).ok()?;
                Some(PriceLevel { price, quantity })
            })
            .collect()
    }

    /// Returns up to `n` best ask levels in ascending price order (best ask first).
    ///
    /// Returns an empty `Vec` when the ask side is empty or `n == 0`.
    pub fn top_n_ask_levels(&self, n: usize) -> Vec<PriceLevel> {
        if n == 0 {
            return vec![];
        }
        self.asks
            .iter()
            .take(n)
            .filter_map(|(&px, &qty)| {
                let price = Price::new(px).ok()?;
                let quantity = Quantity::new(qty).ok()?;
                Some(PriceLevel { price, quantity })
            })
            .collect()
    }

    /// Returns the total quantity across the top `n` bid levels.
    ///
    /// Sweeps from the best (highest) bid downwards and sums quantities.
    /// Returns zero when the bid side is empty or `n == 0`.
    pub fn cumulative_bid_qty(&self, n: usize) -> Decimal {
        if n == 0 {
            return Decimal::ZERO;
        }
        self.bids.iter().rev().take(n).map(|(_, &qty)| qty).sum()
    }

    /// Returns the bid-ask spread in basis points.
    ///
    /// `spread_bps = (best_ask - best_bid) / mid_price * 10_000`.
    /// Returns `None` if either side is empty or mid-price is zero.
    pub fn spread_bps(&self) -> Option<Decimal> {
        let bid = self.best_bid()?.price.value();
        let ask = self.best_ask()?.price.value();
        let mid = (bid + ask) / Decimal::TWO;
        if mid.is_zero() {
            return None;
        }
        let spread = ask - bid;
        spread.checked_div(mid).map(|r| r * Decimal::from(10_000u32))
    }

    /// Returns the total quantity across the top `n` ask levels.
    ///
    /// Sweeps from the best (lowest) ask upwards and sums quantities.
    /// Returns zero when the ask side is empty or `n == 0`.
    pub fn cumulative_ask_qty(&self, n: usize) -> Decimal {
        if n == 0 {
            return Decimal::ZERO;
        }
        self.asks.iter().take(n).map(|(_, &qty)| qty).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn make_book() -> OrderBook {
        OrderBook::new(Symbol::new("AAPL").unwrap())
    }

    fn set_delta(side: Side, price: &str, qty: &str, seq: u64) -> BookDelta {
        BookDelta {
            side,
            price: Price::new(price.parse().unwrap()).unwrap(),
            quantity: Quantity::new(qty.parse().unwrap()).unwrap(),
            action: DeltaAction::Set,
            sequence: seq,
        }
    }

    fn remove_delta(side: Side, price: &str, seq: u64) -> BookDelta {
        BookDelta {
            side,
            price: Price::new(price.parse().unwrap()).unwrap(),
            quantity: Quantity::zero(),
            action: DeltaAction::Remove,
            sequence: seq,
        }
    }

    #[test]
    fn test_orderbook_apply_delta_updates_bid() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "100", "10", 1))
            .unwrap();
        let best = book.best_bid().unwrap();
        assert_eq!(best.price.value(), dec!(100));
        assert_eq!(best.quantity.value(), dec!(10));
    }

    #[test]
    fn test_orderbook_apply_delta_updates_ask() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Ask, "101", "5", 1))
            .unwrap();
        let best = book.best_ask().unwrap();
        assert_eq!(best.price.value(), dec!(101));
        assert_eq!(best.quantity.value(), dec!(5));
    }

    #[test]
    fn test_orderbook_sequence_mismatch_returns_error() {
        let mut book = make_book();
        let result = book.apply_delta(set_delta(Side::Bid, "100", "10", 2));
        assert!(matches!(
            result,
            Err(FinError::SequenceMismatch {
                expected: 1,
                got: 2
            })
        ));
    }

    #[test]
    fn test_orderbook_sequence_advances_correctly() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "100", "10", 1))
            .unwrap();
        assert_eq!(book.sequence(), 1);
        book.apply_delta(set_delta(Side::Ask, "101", "5", 2))
            .unwrap();
        assert_eq!(book.sequence(), 2);
    }

    #[test]
    fn test_orderbook_best_bid_max_price() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "99", "10", 1))
            .unwrap();
        book.apply_delta(set_delta(Side::Bid, "100", "5", 2))
            .unwrap();
        book.apply_delta(set_delta(Side::Bid, "98", "20", 3))
            .unwrap();
        let best = book.best_bid().unwrap();
        assert_eq!(best.price.value(), dec!(100));
    }

    #[test]
    fn test_orderbook_best_ask_min_price() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Ask, "102", "10", 1))
            .unwrap();
        book.apply_delta(set_delta(Side::Ask, "101", "5", 2))
            .unwrap();
        book.apply_delta(set_delta(Side::Ask, "103", "20", 3))
            .unwrap();
        let best = book.best_ask().unwrap();
        assert_eq!(best.price.value(), dec!(101));
    }

    #[test]
    fn test_orderbook_spread_positive() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "100", "10", 1))
            .unwrap();
        book.apply_delta(set_delta(Side::Ask, "101", "5", 2))
            .unwrap();
        let spread = book.spread().unwrap();
        assert_eq!(spread, dec!(1));
        assert!(spread > Decimal::ZERO);
    }

    #[test]
    fn test_orderbook_mid_price() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "100", "10", 1))
            .unwrap();
        book.apply_delta(set_delta(Side::Ask, "102", "5", 2))
            .unwrap();
        let mid = book.mid_price().unwrap();
        assert_eq!(mid, dec!(101));
    }

    #[test]
    fn test_orderbook_spread_none_when_empty() {
        let book = make_book();
        assert!(book.spread().is_none());
    }

    #[test]
    fn test_orderbook_vwap_insufficient_liquidity() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Ask, "101", "5", 1))
            .unwrap();
        let result = book.vwap_for_qty(Side::Ask, Quantity::new(dec!(100)).unwrap());
        assert!(matches!(result, Err(FinError::InsufficientLiquidity(_))));
    }

    #[test]
    fn test_orderbook_vwap_single_level() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Ask, "100", "10", 1))
            .unwrap();
        let vwap = book
            .vwap_for_qty(Side::Ask, Quantity::new(dec!(5)).unwrap())
            .unwrap();
        assert_eq!(vwap, dec!(100));
    }

    #[test]
    fn test_orderbook_vwap_multi_level() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Ask, "100", "5", 1))
            .unwrap();
        book.apply_delta(set_delta(Side::Ask, "101", "5", 2))
            .unwrap();
        // 5 @ 100 + 5 @ 101 = 1005 / 10 = 100.5
        let vwap = book
            .vwap_for_qty(Side::Ask, Quantity::new(dec!(10)).unwrap())
            .unwrap();
        assert_eq!(vwap, dec!(100.5));
    }

    #[test]
    fn test_orderbook_remove_level_delta() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "100", "10", 1))
            .unwrap();
        book.apply_delta(remove_delta(Side::Bid, "100", 2)).unwrap();
        assert!(book.best_bid().is_none());
    }

    #[test]
    fn test_orderbook_top_bids_order() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "98", "10", 1))
            .unwrap();
        book.apply_delta(set_delta(Side::Bid, "100", "5", 2))
            .unwrap();
        book.apply_delta(set_delta(Side::Bid, "99", "20", 3))
            .unwrap();
        let top = book.top_bids(2);
        assert_eq!(top[0].price.value(), dec!(100));
        assert_eq!(top[1].price.value(), dec!(99));
    }

    #[test]
    fn test_orderbook_top_asks_order() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Ask, "103", "10", 1))
            .unwrap();
        book.apply_delta(set_delta(Side::Ask, "101", "5", 2))
            .unwrap();
        book.apply_delta(set_delta(Side::Ask, "102", "20", 3))
            .unwrap();
        let top = book.top_asks(2);
        assert_eq!(top[0].price.value(), dec!(101));
        assert_eq!(top[1].price.value(), dec!(102));
    }

    #[test]
    fn test_orderbook_bid_count_ask_count() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "100", "1", 1))
            .unwrap();
        book.apply_delta(set_delta(Side::Ask, "101", "1", 2))
            .unwrap();
        assert_eq!(book.bid_count(), 1);
        assert_eq!(book.ask_count(), 1);
    }

    #[test]
    fn test_orderbook_vwap_zero_qty_returns_zero() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Ask, "100", "10", 1))
            .unwrap();
        let vwap = book.vwap_for_qty(Side::Ask, Quantity::zero()).unwrap();
        assert_eq!(vwap, Decimal::ZERO);
    }

    // ── Inverted spread guard ─────────────────────────────────────────────────

    #[test]
    fn test_apply_delta_rejects_inverted_spread() {
        let mut book = make_book();
        // Set ask at 100
        book.apply_delta(set_delta(Side::Ask, "100", "5", 1))
            .unwrap();
        // Try to set bid at 101 (would cross the ask): must fail
        let result = book.apply_delta(set_delta(Side::Bid, "101", "5", 2));
        assert!(
            matches!(result, Err(FinError::InvertedSpread { .. })),
            "expected InvertedSpread, got {:?}",
            result
        );
    }

    #[test]
    fn test_apply_delta_inverted_spread_rolls_back_sequence() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Ask, "100", "5", 1))
            .unwrap();
        assert_eq!(book.sequence(), 1);
        // This should fail and leave sequence unchanged
        let _ = book.apply_delta(set_delta(Side::Bid, "101", "5", 2));
        assert_eq!(
            book.sequence(),
            1,
            "sequence must not advance on rejected delta"
        );
    }

    #[test]
    fn test_apply_delta_inverted_spread_rolled_back_book_state() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Ask, "100", "5", 1))
            .unwrap();
        // Rejected bid at 101 must not persist in the book
        let _ = book.apply_delta(set_delta(Side::Bid, "101", "5", 2));
        assert!(
            book.best_bid().is_none(),
            "rejected bid must not appear in book"
        );
    }

    /// Empty book mid_price returns None.
    #[test]
    fn test_empty_book_mid_price_returns_none() {
        let book = make_book();
        assert!(
            book.mid_price().is_none(),
            "empty book mid_price must be None"
        );
    }

    /// Empty book best_bid returns None.
    #[test]
    fn test_empty_book_best_bid_returns_none() {
        let book = make_book();
        assert!(book.best_bid().is_none());
    }

    /// Empty book best_ask returns None.
    #[test]
    fn test_empty_book_best_ask_returns_none() {
        let book = make_book();
        assert!(book.best_ask().is_none());
    }

    /// Best bid/ask after many inserts and removes reflects only surviving levels.
    #[test]
    fn test_best_bid_after_many_inserts_and_removes() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "100", "10", 1))
            .unwrap();
        book.apply_delta(set_delta(Side::Bid, "105", "5", 2))
            .unwrap();
        book.apply_delta(set_delta(Side::Bid, "103", "8", 3))
            .unwrap();
        // Remove 105 (was best bid)
        book.apply_delta(remove_delta(Side::Bid, "105", 4)).unwrap();
        let best = book.best_bid().unwrap();
        assert_eq!(
            best.price.value(),
            dec!(103),
            "best bid after removing top level must be 103"
        );
    }

    #[test]
    fn test_best_ask_after_many_inserts_and_removes() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Ask, "110", "10", 1))
            .unwrap();
        book.apply_delta(set_delta(Side::Ask, "108", "5", 2))
            .unwrap();
        book.apply_delta(set_delta(Side::Ask, "109", "8", 3))
            .unwrap();
        // Remove 108 (was best ask)
        book.apply_delta(remove_delta(Side::Ask, "108", 4)).unwrap();
        let best = book.best_ask().unwrap();
        assert_eq!(
            best.price.value(),
            dec!(109),
            "best ask after removing top level must be 109"
        );
    }

    /// Crossed book detection: ask <= bid must return InvertedSpread.
    #[test]
    fn test_crossed_book_ask_at_bid_price_rejected() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "100", "10", 1))
            .unwrap();
        let result = book.apply_delta(set_delta(Side::Ask, "100", "5", 2));
        assert!(
            matches!(result, Err(FinError::InvertedSpread { .. })),
            "ask at bid price must produce InvertedSpread"
        );
    }

    /// Empty book spread returns None.
    #[test]
    fn test_empty_book_spread_returns_none() {
        let book = make_book();
        assert!(book.spread().is_none());
    }

    #[test]
    fn test_orderbook_snapshot_returns_top_n_both_sides() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "99", "10", 1)).unwrap();
        book.apply_delta(set_delta(Side::Bid, "100", "5", 2)).unwrap();
        book.apply_delta(set_delta(Side::Ask, "101", "3", 3)).unwrap();
        book.apply_delta(set_delta(Side::Ask, "102", "7", 4)).unwrap();
        let (bids, asks) = book.snapshot(2);
        assert_eq!(bids.len(), 2);
        assert_eq!(asks.len(), 2);
        assert_eq!(bids[0].price.value(), dec!(100));
        assert_eq!(asks[0].price.value(), dec!(101));
    }

    #[test]
    fn test_orderbook_snapshot_empty_book() {
        let book = make_book();
        let (bids, asks) = book.snapshot(5);
        assert!(bids.is_empty());
        assert!(asks.is_empty());
    }

    #[test]
    fn test_orderbook_clear_removes_all_levels() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "99", "10", 1)).unwrap();
        book.apply_delta(set_delta(Side::Ask, "101", "5", 2)).unwrap();
        assert_eq!(book.bid_count(), 1);
        assert_eq!(book.ask_count(), 1);
        book.clear();
        assert_eq!(book.bid_count(), 0);
        assert_eq!(book.ask_count(), 0);
        assert_eq!(book.sequence(), 0);
    }

    #[test]
    fn test_orderbook_clear_allows_fresh_deltas() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "100", "5", 1)).unwrap();
        book.clear();
        // After clear, sequence resets to 0, so next delta must be seq=1
        assert!(book.apply_delta(set_delta(Side::Bid, "100", "5", 1)).is_ok());
    }

    #[test]
    fn test_orderbook_total_bid_volume() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "100", "5", 1)).unwrap();
        book.apply_delta(set_delta(Side::Bid, "99", "3", 2)).unwrap();
        assert_eq!(book.total_bid_volume(), dec!(8));
    }

    #[test]
    fn test_orderbook_total_ask_volume() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Ask, "101", "4", 1)).unwrap();
        book.apply_delta(set_delta(Side::Ask, "102", "6", 2)).unwrap();
        assert_eq!(book.total_ask_volume(), dec!(10));
    }

    #[test]
    fn test_orderbook_total_bid_volume_empty() {
        let book = make_book();
        assert_eq!(book.total_bid_volume(), dec!(0));
    }

    #[test]
    fn test_orderbook_imbalance_balanced() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "100", "5", 1)).unwrap();
        book.apply_delta(set_delta(Side::Ask, "101", "5", 2)).unwrap();
        assert_eq!(book.imbalance().unwrap(), dec!(0));
    }

    #[test]
    fn test_orderbook_imbalance_bid_heavy() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "100", "9", 1)).unwrap();
        book.apply_delta(set_delta(Side::Ask, "101", "1", 2)).unwrap();
        // (9 - 1) / 10 = 0.8
        assert_eq!(book.imbalance().unwrap(), dec!(0.8));
    }

    #[test]
    fn test_orderbook_imbalance_ask_heavy() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "100", "1", 1)).unwrap();
        book.apply_delta(set_delta(Side::Ask, "101", "9", 2)).unwrap();
        // (1 - 9) / 10 = -0.8
        assert_eq!(book.imbalance().unwrap(), dec!(-0.8));
    }

    #[test]
    fn test_orderbook_imbalance_empty_returns_none() {
        let book = make_book();
        assert!(book.imbalance().is_none());
    }

    #[test]
    fn test_orderbook_has_price_bid_present() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "100", "5", 1)).unwrap();
        let price = Price::new(dec!(100)).unwrap();
        assert!(book.has_price(Side::Bid, price));
        assert!(!book.has_price(Side::Ask, price));
    }

    #[test]
    fn test_orderbook_has_price_ask_present() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Ask, "101", "3", 1)).unwrap();
        let price = Price::new(dec!(101)).unwrap();
        assert!(book.has_price(Side::Ask, price));
        assert!(!book.has_price(Side::Bid, price));
    }

    #[test]
    fn test_orderbook_has_price_absent() {
        let book = make_book();
        let price = Price::new(dec!(100)).unwrap();
        assert!(!book.has_price(Side::Bid, price));
        assert!(!book.has_price(Side::Ask, price));
    }

    #[test]
    fn test_orderbook_has_price_false_after_remove() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "100", "5", 1)).unwrap();
        book.apply_delta(BookDelta {
            side: Side::Bid,
            price: Price::new(dec!(100)).unwrap(),
            quantity: Quantity::zero(),
            action: DeltaAction::Remove,
            sequence: 2,
        })
        .unwrap();
        let price = Price::new(dec!(100)).unwrap();
        assert!(!book.has_price(Side::Bid, price));
    }

    #[test]
    fn test_orderbook_level_count_bids() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "100", "10", 1)).unwrap();
        book.apply_delta(set_delta(Side::Bid, "99", "5", 2)).unwrap();
        assert_eq!(book.level_count(Side::Bid), 2);
        assert_eq!(book.level_count(Side::Ask), 0);
    }

    #[test]
    fn test_orderbook_level_count_asks() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Ask, "101", "3", 1)).unwrap();
        assert_eq!(book.level_count(Side::Ask), 1);
        assert_eq!(book.level_count(Side::Bid), 0);
    }

    #[test]
    fn test_orderbook_weighted_mid_equal_qty() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "100", "5", 1)).unwrap();
        book.apply_delta(set_delta(Side::Ask, "102", "5", 2)).unwrap();
        // Equal qty → simple midpoint
        assert_eq!(book.weighted_mid().unwrap(), dec!(101));
    }

    #[test]
    fn test_orderbook_weighted_mid_bid_heavy() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "100", "9", 1)).unwrap();
        book.apply_delta(set_delta(Side::Ask, "110", "1", 2)).unwrap();
        // (100*1 + 110*9) / (9+1) = (100 + 990) / 10 = 109
        assert_eq!(book.weighted_mid().unwrap(), dec!(109));
    }

    #[test]
    fn test_orderbook_weighted_mid_empty_returns_none() {
        let book = make_book();
        assert!(book.weighted_mid().is_none());
    }

    #[test]
    fn test_orderbook_bid_ask_ratio_equal_volumes() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "100", "10", 1)).unwrap();
        book.apply_delta(set_delta(Side::Ask, "101", "10", 2)).unwrap();
        assert_eq!(book.bid_ask_ratio().unwrap(), dec!(1));
    }

    #[test]
    fn test_orderbook_bid_ask_ratio_bid_heavy() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "100", "20", 1)).unwrap();
        book.apply_delta(set_delta(Side::Ask, "101", "10", 2)).unwrap();
        assert_eq!(book.bid_ask_ratio().unwrap(), dec!(2));
    }

    #[test]
    fn test_orderbook_bid_ask_ratio_empty_returns_none() {
        let book = make_book();
        assert!(book.bid_ask_ratio().is_none());
    }

    #[test]
    fn test_orderbook_price_impact_buy_single_level() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Ask, "101", "10", 1)).unwrap();
        let qty = Quantity::new(dec!(5)).unwrap();
        let avg = book.price_impact(Side::Bid, qty).unwrap();
        assert_eq!(avg, dec!(101));
    }

    #[test]
    fn test_orderbook_price_impact_buy_spans_two_levels() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Ask, "100", "5", 1)).unwrap();
        book.apply_delta(set_delta(Side::Ask, "102", "5", 2)).unwrap();
        // 5 @ 100 + 5 @ 102 = 1010 / 10 = 101
        let qty = Quantity::new(dec!(10)).unwrap();
        let avg = book.price_impact(Side::Bid, qty).unwrap();
        assert_eq!(avg, dec!(101));
    }

    #[test]
    fn test_orderbook_price_impact_insufficient_depth_returns_none() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Ask, "101", "3", 1)).unwrap();
        let qty = Quantity::new(dec!(10)).unwrap();
        assert!(book.price_impact(Side::Bid, qty).is_none());
    }

    #[test]
    fn test_orderbook_price_impact_zero_qty_returns_none() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Ask, "101", "10", 1)).unwrap();
        let qty = Quantity::zero();
        assert!(book.price_impact(Side::Bid, qty).is_none());
    }

    #[test]
    fn test_orderbook_depth_at_existing_bid_level() {
        let mut book = make_book();
        // make_book sets seq=0; add a bid at 99 qty=5 with seq=1
        book.apply_delta(set_delta(Side::Bid, "99", "5", 1)).unwrap();
        let price = Price::new(dec!(99)).unwrap();
        assert_eq!(book.depth_at(Side::Bid, price), Some(dec!(5)));
    }

    #[test]
    fn test_orderbook_depth_at_absent_level_returns_none() {
        let book = make_book();
        let price = Price::new(dec!(50)).unwrap();
        assert!(book.depth_at(Side::Bid, price).is_none());
        assert!(book.depth_at(Side::Ask, price).is_none());
    }

    #[test]
    fn test_orderbook_bid_depth_returns_top_n_descending() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "100", "10", 1)).unwrap();
        book.apply_delta(set_delta(Side::Bid, "99", "5", 2)).unwrap();
        book.apply_delta(set_delta(Side::Bid, "98", "3", 3)).unwrap();
        let levels = book.bid_depth(2);
        assert_eq!(levels.len(), 2);
        assert_eq!(levels[0].price.value(), dec!(100)); // best bid first
        assert_eq!(levels[1].price.value(), dec!(99));
    }

    #[test]
    fn test_orderbook_ask_depth_returns_top_n_ascending() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Ask, "101", "10", 1)).unwrap();
        book.apply_delta(set_delta(Side::Ask, "102", "5", 2)).unwrap();
        book.apply_delta(set_delta(Side::Ask, "103", "3", 3)).unwrap();
        let levels = book.ask_depth(2);
        assert_eq!(levels.len(), 2);
        assert_eq!(levels[0].price.value(), dec!(101)); // best ask first
        assert_eq!(levels[1].price.value(), dec!(102));
    }

    #[test]
    fn test_orderbook_bid_depth_fewer_than_n() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "100", "10", 1)).unwrap();
        let levels = book.bid_depth(5);
        assert_eq!(levels.len(), 1);
    }

    #[test]
    fn test_orderbook_ask_depth_empty_book() {
        let book = make_book();
        assert!(book.ask_depth(3).is_empty());
    }

    #[test]
    fn test_orderbook_remove_all_bids_clears_bid_side() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "100", "10", 1)).unwrap();
        book.apply_delta(set_delta(Side::Bid, "99", "5", 2)).unwrap();
        book.remove_all(Side::Bid);
        assert!(book.best_bid().is_none());
    }

    #[test]
    fn test_orderbook_remove_all_bids_leaves_asks_intact() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "100", "10", 1)).unwrap();
        book.apply_delta(set_delta(Side::Ask, "101", "5", 2)).unwrap();
        book.remove_all(Side::Bid);
        assert!(book.best_bid().is_none());
        assert!(book.best_ask().is_some());
    }

    #[test]
    fn test_orderbook_remove_all_asks_clears_ask_side() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Ask, "101", "5", 1)).unwrap();
        book.apply_delta(set_delta(Side::Ask, "102", "3", 2)).unwrap();
        book.remove_all(Side::Ask);
        assert!(book.best_ask().is_none());
    }

    #[test]
    fn test_orderbook_total_levels_sums_both_sides() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "100", "10", 1)).unwrap();
        book.apply_delta(set_delta(Side::Bid, "99", "5", 2)).unwrap();
        book.apply_delta(set_delta(Side::Ask, "101", "8", 3)).unwrap();
        assert_eq!(book.total_levels(), 3);
    }

    #[test]
    fn test_orderbook_total_levels_empty_book() {
        let book = make_book();
        assert_eq!(book.total_levels(), 0);
    }
}
