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
//! - Thread-safe: `OrderBook` implements neither `Send` nor `Sync` by default (use Arc<Mutex> externally)
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
    /// Bid levels: price → quantity. Iterated in ascending key order by BTreeMap;
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
    pub fn apply_delta(&mut self, delta: BookDelta) -> Result<(), FinError> {
        let expected = self.sequence + 1;
        if delta.sequence != expected {
            return Err(FinError::SequenceMismatch { expected, got: delta.sequence });
        }
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
                    Side::Bid => { self.bids.remove(&delta.price.value()); }
                    Side::Ask => { self.asks.remove(&delta.price.value()); }
                },
                DeltaAction::Remove => match delta.side {
                    Side::Bid => { self.bids.insert(delta.price.value(), delta.quantity.value()); }
                    Side::Ask => { self.asks.insert(delta.price.value(), delta.quantity.value()); }
                },
            }
            self.sequence = expected - 1;
            return Err(FinError::InvertedSpread { best_bid: best_bid_p, best_ask: best_ask_p });
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
        let mut remaining = target;
        let mut total_cost = Decimal::ZERO;

        let levels: Box<dyn Iterator<Item = (&Decimal, &Decimal)>> = match side {
            Side::Bid => Box::new(self.bids.iter().rev()),
            Side::Ask => Box::new(self.asks.iter()),
        };

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

    /// Returns the number of bid price levels.
    pub fn bid_count(&self) -> usize {
        self.bids.len()
    }

    /// Returns the number of ask price levels.
    pub fn ask_count(&self) -> usize {
        self.asks.len()
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
        book.apply_delta(set_delta(Side::Bid, "100", "10", 1)).unwrap();
        let best = book.best_bid().unwrap();
        assert_eq!(best.price.value(), dec!(100));
        assert_eq!(best.quantity.value(), dec!(10));
    }

    #[test]
    fn test_orderbook_apply_delta_updates_ask() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Ask, "101", "5", 1)).unwrap();
        let best = book.best_ask().unwrap();
        assert_eq!(best.price.value(), dec!(101));
        assert_eq!(best.quantity.value(), dec!(5));
    }

    #[test]
    fn test_orderbook_sequence_mismatch_returns_error() {
        let mut book = make_book();
        let result = book.apply_delta(set_delta(Side::Bid, "100", "10", 2));
        assert!(matches!(result, Err(FinError::SequenceMismatch { expected: 1, got: 2 })));
    }

    #[test]
    fn test_orderbook_sequence_advances_correctly() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "100", "10", 1)).unwrap();
        assert_eq!(book.sequence(), 1);
        book.apply_delta(set_delta(Side::Ask, "101", "5", 2)).unwrap();
        assert_eq!(book.sequence(), 2);
    }

    #[test]
    fn test_orderbook_best_bid_max_price() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "99", "10", 1)).unwrap();
        book.apply_delta(set_delta(Side::Bid, "100", "5", 2)).unwrap();
        book.apply_delta(set_delta(Side::Bid, "98", "20", 3)).unwrap();
        let best = book.best_bid().unwrap();
        assert_eq!(best.price.value(), dec!(100));
    }

    #[test]
    fn test_orderbook_best_ask_min_price() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Ask, "102", "10", 1)).unwrap();
        book.apply_delta(set_delta(Side::Ask, "101", "5", 2)).unwrap();
        book.apply_delta(set_delta(Side::Ask, "103", "20", 3)).unwrap();
        let best = book.best_ask().unwrap();
        assert_eq!(best.price.value(), dec!(101));
    }

    #[test]
    fn test_orderbook_spread_positive() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "100", "10", 1)).unwrap();
        book.apply_delta(set_delta(Side::Ask, "101", "5", 2)).unwrap();
        let spread = book.spread().unwrap();
        assert_eq!(spread, dec!(1));
        assert!(spread > Decimal::ZERO);
    }

    #[test]
    fn test_orderbook_mid_price() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "100", "10", 1)).unwrap();
        book.apply_delta(set_delta(Side::Ask, "102", "5", 2)).unwrap();
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
        book.apply_delta(set_delta(Side::Ask, "101", "5", 1)).unwrap();
        let result = book.vwap_for_qty(Side::Ask, Quantity::new(dec!(100)).unwrap());
        assert!(matches!(result, Err(FinError::InsufficientLiquidity(_))));
    }

    #[test]
    fn test_orderbook_vwap_single_level() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Ask, "100", "10", 1)).unwrap();
        let vwap = book.vwap_for_qty(Side::Ask, Quantity::new(dec!(5)).unwrap()).unwrap();
        assert_eq!(vwap, dec!(100));
    }

    #[test]
    fn test_orderbook_vwap_multi_level() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Ask, "100", "5", 1)).unwrap();
        book.apply_delta(set_delta(Side::Ask, "101", "5", 2)).unwrap();
        // 5 @ 100 + 5 @ 101 = 1005 / 10 = 100.5
        let vwap = book.vwap_for_qty(Side::Ask, Quantity::new(dec!(10)).unwrap()).unwrap();
        assert_eq!(vwap, dec!(100.5));
    }

    #[test]
    fn test_orderbook_remove_level_delta() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "100", "10", 1)).unwrap();
        book.apply_delta(remove_delta(Side::Bid, "100", 2)).unwrap();
        assert!(book.best_bid().is_none());
    }

    #[test]
    fn test_orderbook_top_bids_order() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "98", "10", 1)).unwrap();
        book.apply_delta(set_delta(Side::Bid, "100", "5", 2)).unwrap();
        book.apply_delta(set_delta(Side::Bid, "99", "20", 3)).unwrap();
        let top = book.top_bids(2);
        assert_eq!(top[0].price.value(), dec!(100));
        assert_eq!(top[1].price.value(), dec!(99));
    }

    #[test]
    fn test_orderbook_top_asks_order() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Ask, "103", "10", 1)).unwrap();
        book.apply_delta(set_delta(Side::Ask, "101", "5", 2)).unwrap();
        book.apply_delta(set_delta(Side::Ask, "102", "20", 3)).unwrap();
        let top = book.top_asks(2);
        assert_eq!(top[0].price.value(), dec!(101));
        assert_eq!(top[1].price.value(), dec!(102));
    }

    #[test]
    fn test_orderbook_bid_count_ask_count() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "100", "1", 1)).unwrap();
        book.apply_delta(set_delta(Side::Ask, "101", "1", 2)).unwrap();
        assert_eq!(book.bid_count(), 1);
        assert_eq!(book.ask_count(), 1);
    }

    #[test]
    fn test_orderbook_vwap_zero_qty_returns_zero() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Ask, "100", "10", 1)).unwrap();
        let vwap = book.vwap_for_qty(Side::Ask, Quantity::zero()).unwrap();
        assert_eq!(vwap, Decimal::ZERO);
    }

    // ── Inverted spread guard ─────────────────────────────────────────────────

    #[test]
    fn test_apply_delta_rejects_inverted_spread() {
        let mut book = make_book();
        // Set ask at 100
        book.apply_delta(set_delta(Side::Ask, "100", "5", 1)).unwrap();
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
        book.apply_delta(set_delta(Side::Ask, "100", "5", 1)).unwrap();
        assert_eq!(book.sequence(), 1);
        // This should fail and leave sequence unchanged
        let _ = book.apply_delta(set_delta(Side::Bid, "101", "5", 2));
        assert_eq!(book.sequence(), 1, "sequence must not advance on rejected delta");
    }

    #[test]
    fn test_apply_delta_inverted_spread_rolled_back_book_state() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Ask, "100", "5", 1)).unwrap();
        // Rejected bid at 101 must not persist in the book
        let _ = book.apply_delta(set_delta(Side::Bid, "101", "5", 2));
        assert!(book.best_bid().is_none(), "rejected bid must not appear in book");
    }

    /// Empty book mid_price returns None.
    #[test]
    fn test_empty_book_mid_price_returns_none() {
        let book = make_book();
        assert!(book.mid_price().is_none(), "empty book mid_price must be None");
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
        book.apply_delta(set_delta(Side::Bid, "100", "10", 1)).unwrap();
        book.apply_delta(set_delta(Side::Bid, "105", "5", 2)).unwrap();
        book.apply_delta(set_delta(Side::Bid, "103", "8", 3)).unwrap();
        // Remove 105 (was best bid)
        book.apply_delta(remove_delta(Side::Bid, "105", 4)).unwrap();
        let best = book.best_bid().unwrap();
        assert_eq!(best.price.value(), dec!(103), "best bid after removing top level must be 103");
    }

    #[test]
    fn test_best_ask_after_many_inserts_and_removes() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Ask, "110", "10", 1)).unwrap();
        book.apply_delta(set_delta(Side::Ask, "108", "5", 2)).unwrap();
        book.apply_delta(set_delta(Side::Ask, "109", "8", 3)).unwrap();
        // Remove 108 (was best ask)
        book.apply_delta(remove_delta(Side::Ask, "108", 4)).unwrap();
        let best = book.best_ask().unwrap();
        assert_eq!(best.price.value(), dec!(109), "best ask after removing top level must be 109");
    }

    /// Crossed book detection: ask <= bid must return InvertedSpread.
    #[test]
    fn test_crossed_book_ask_at_bid_price_rejected() {
        let mut book = make_book();
        book.apply_delta(set_delta(Side::Bid, "100", "10", 1)).unwrap();
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
}
