//! # Module: tick
//!
//! ## Responsibility
//! Represents a single market trade (tick), provides filtering, and supports
//! deterministic replay of tick sequences in timestamp order.
//!
//! ## Guarantees
//! - `Tick::notional()` is always `price * quantity` without rounding
//! - `TickReplayer` always produces ticks in ascending timestamp order
//! - `TickReplayer` implements `Iterator<Item = Tick>` (yields cloned ticks)
//! - `TickFilter::matches` is pure (no side effects)
//!
//! ## NOT Responsible For
//! - Persistence or serialization to external stores
//! - Cross-symbol aggregation

use crate::types::{NanoTimestamp, Price, Quantity, Side, Symbol};
use rust_decimal::Decimal;

/// A single market trade event.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Tick {
    /// The traded instrument.
    pub symbol: Symbol,
    /// The trade price (positive).
    pub price: Price,
    /// The trade quantity (non-negative).
    pub quantity: Quantity,
    /// Whether this was a bid-side or ask-side aggressor.
    pub side: Side,
    /// Exchange timestamp in nanoseconds.
    pub timestamp: NanoTimestamp,
}

impl Tick {
    /// Constructs a new `Tick`.
    pub fn new(
        symbol: Symbol,
        price: Price,
        quantity: Quantity,
        side: Side,
        timestamp: NanoTimestamp,
    ) -> Self {
        Self {
            symbol,
            price,
            quantity,
            side,
            timestamp,
        }
    }

    /// Returns the notional value of this tick: `price * quantity`.
    pub fn notional(&self) -> Decimal {
        self.price.value() * self.quantity.value()
    }
}

/// Filters ticks by optional symbol, side, price range, and minimum quantity predicates.
///
/// All predicates are `ANDed` together. Unset predicates always pass.
pub struct TickFilter {
    symbol: Option<Symbol>,
    side: Option<Side>,
    min_qty: Option<Quantity>,
    max_qty: Option<Quantity>,
    min_price: Option<Price>,
    max_price: Option<Price>,
}

impl TickFilter {
    /// Creates a new `TickFilter` with no predicates set (matches everything).
    pub fn new() -> Self {
        Self {
            symbol: None,
            side: None,
            min_qty: None,
            max_qty: None,
            min_price: None,
            max_price: None,
        }
    }

    /// Restrict matches to ticks with this symbol.
    #[must_use]
    pub fn symbol(mut self, s: Symbol) -> Self {
        self.symbol = Some(s);
        self
    }

    /// Restrict matches to ticks on this side.
    #[must_use]
    pub fn side(mut self, s: Side) -> Self {
        self.side = Some(s);
        self
    }

    /// Restrict matches to ticks with quantity >= `q`.
    #[must_use]
    pub fn min_quantity(mut self, q: Quantity) -> Self {
        self.min_qty = Some(q);
        self
    }

    /// Restrict matches to ticks with quantity <= `q`.
    #[must_use]
    pub fn max_quantity(mut self, q: Quantity) -> Self {
        self.max_qty = Some(q);
        self
    }

    /// Restrict matches to ticks with price >= `p`.
    #[must_use]
    pub fn min_price(mut self, p: Price) -> Self {
        self.min_price = Some(p);
        self
    }

    /// Restrict matches to ticks with price <= `p`.
    #[must_use]
    pub fn max_price(mut self, p: Price) -> Self {
        self.max_price = Some(p);
        self
    }

    /// Returns `true` if the tick satisfies all configured predicates.
    pub fn matches(&self, tick: &Tick) -> bool {
        if let Some(ref sym) = self.symbol {
            if tick.symbol != *sym {
                return false;
            }
        }
        if let Some(ref side) = self.side {
            if tick.side != *side {
                return false;
            }
        }
        if let Some(ref min_qty) = self.min_qty {
            if tick.quantity < *min_qty {
                return false;
            }
        }
        if let Some(ref max_qty) = self.max_qty {
            if tick.quantity > *max_qty {
                return false;
            }
        }
        if let Some(ref min_p) = self.min_price {
            if tick.price < *min_p {
                return false;
            }
        }
        if let Some(ref max_p) = self.max_price {
            if tick.price > *max_p {
                return false;
            }
        }
        true
    }
}

impl Default for TickFilter {
    fn default() -> Self {
        Self::new()
    }
}

/// Replays a collection of ticks in ascending timestamp order.
pub struct TickReplayer {
    ticks: Vec<Tick>,
    index: usize,
}

impl TickReplayer {
    /// Constructs a `TickReplayer`, sorting `ticks` by timestamp ascending.
    pub fn new(mut ticks: Vec<Tick>) -> Self {
        ticks.sort_by_key(|t| t.timestamp);
        Self { ticks, index: 0 }
    }

    /// Returns the next tick in timestamp order, or `None` if exhausted.
    pub fn next_tick(&mut self) -> Option<&Tick> {
        let tick = self.ticks.get(self.index)?;
        self.index += 1;
        Some(tick)
    }

    /// Returns the number of ticks not yet yielded.
    pub fn remaining(&self) -> usize {
        self.ticks.len().saturating_sub(self.index)
    }

    /// Resets the replayer to the beginning of the tick sequence.
    pub fn reset(&mut self) {
        self.index = 0;
    }
}

impl Iterator for TickReplayer {
    type Item = Tick;

    fn next(&mut self) -> Option<Self::Item> {
        let tick = self.ticks.get(self.index)?.clone();
        self.index += 1;
        Some(tick)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn make_tick(sym: &str, price: &str, qty: &str, side: Side, ts: i64) -> Tick {
        Tick::new(
            Symbol::new(sym).unwrap(),
            Price::new(dec_from_str(price)).unwrap(),
            Quantity::new(dec_from_str(qty)).unwrap(),
            side,
            NanoTimestamp::new(ts),
        )
    }

    fn dec_from_str(s: &str) -> Decimal {
        s.parse().unwrap()
    }

    #[test]
    fn test_tick_notional_is_price_times_quantity() {
        let t = make_tick("AAPL", "150.00", "10", Side::Ask, 0);
        assert_eq!(t.notional(), dec!(1500.00));
    }

    #[test]
    fn test_tick_notional_zero_quantity() {
        let t = make_tick("AAPL", "150.00", "0", Side::Ask, 0);
        assert_eq!(t.notional(), dec!(0));
    }

    #[test]
    fn test_tick_filter_no_predicates_matches_all() {
        let f = TickFilter::new();
        let t = make_tick("AAPL", "1", "1", Side::Bid, 0);
        assert!(f.matches(&t));
    }

    #[test]
    fn test_tick_filter_by_symbol() {
        let sym = Symbol::new("AAPL").unwrap();
        let f = TickFilter::new().symbol(sym);
        let matching = make_tick("AAPL", "1", "1", Side::Bid, 0);
        let non_matching = make_tick("TSLA", "1", "1", Side::Bid, 0);
        assert!(f.matches(&matching));
        assert!(!f.matches(&non_matching));
    }

    #[test]
    fn test_tick_filter_by_side() {
        let f = TickFilter::new().side(Side::Ask);
        let ask_tick = make_tick("AAPL", "1", "1", Side::Ask, 0);
        let bid_tick = make_tick("AAPL", "1", "1", Side::Bid, 0);
        assert!(f.matches(&ask_tick));
        assert!(!f.matches(&bid_tick));
    }

    #[test]
    fn test_tick_filter_by_min_quantity() {
        let min_qty = Quantity::new(dec!(5)).unwrap();
        let f = TickFilter::new().min_quantity(min_qty);
        let large = make_tick("AAPL", "1", "10", Side::Bid, 0);
        let small = make_tick("AAPL", "1", "2", Side::Bid, 0);
        assert!(f.matches(&large));
        assert!(!f.matches(&small));
    }

    #[test]
    fn test_tick_filter_by_max_quantity() {
        let max_qty = Quantity::new(dec!(5)).unwrap();
        let f = TickFilter::new().max_quantity(max_qty);
        let small = make_tick("AAPL", "1", "3", Side::Bid, 0);
        let large = make_tick("AAPL", "1", "10", Side::Bid, 0);
        assert!(f.matches(&small));
        assert!(!f.matches(&large));
    }

    #[test]
    fn test_tick_filter_quantity_range() {
        let min_qty = Quantity::new(dec!(3)).unwrap();
        let max_qty = Quantity::new(dec!(7)).unwrap();
        let f = TickFilter::new().min_quantity(min_qty).max_quantity(max_qty);
        assert!(f.matches(&make_tick("X", "1", "5", Side::Bid, 0)));
        assert!(!f.matches(&make_tick("X", "1", "2", Side::Bid, 0)));
        assert!(!f.matches(&make_tick("X", "1", "10", Side::Bid, 0)));
    }

    #[test]
    fn test_tick_filter_by_min_price() {
        let min_p = Price::new(dec!(100)).unwrap();
        let f = TickFilter::new().min_price(min_p);
        let high = make_tick("AAPL", "150", "1", Side::Bid, 0);
        let low = make_tick("AAPL", "50", "1", Side::Bid, 0);
        assert!(f.matches(&high));
        assert!(!f.matches(&low));
    }

    #[test]
    fn test_tick_filter_by_max_price() {
        let max_p = Price::new(dec!(100)).unwrap();
        let f = TickFilter::new().max_price(max_p);
        let low = make_tick("AAPL", "50", "1", Side::Bid, 0);
        let high = make_tick("AAPL", "150", "1", Side::Bid, 0);
        assert!(f.matches(&low));
        assert!(!f.matches(&high));
    }

    #[test]
    fn test_tick_filter_price_range() {
        let min_p = Price::new(dec!(90)).unwrap();
        let max_p = Price::new(dec!(110)).unwrap();
        let f = TickFilter::new().min_price(min_p).max_price(max_p);
        assert!(f.matches(&make_tick("X", "100", "1", Side::Bid, 0)));
        assert!(!f.matches(&make_tick("X", "80", "1", Side::Bid, 0)));
        assert!(!f.matches(&make_tick("X", "120", "1", Side::Bid, 0)));
    }

    #[test]
    fn test_tick_filter_combined_predicates() {
        let sym = Symbol::new("AAPL").unwrap();
        let min_qty = Quantity::new(dec!(5)).unwrap();
        let f = TickFilter::new()
            .symbol(sym)
            .side(Side::Bid)
            .min_quantity(min_qty);
        let ok = make_tick("AAPL", "1", "10", Side::Bid, 0);
        let wrong_sym = make_tick("TSLA", "1", "10", Side::Bid, 0);
        let wrong_side = make_tick("AAPL", "1", "10", Side::Ask, 0);
        let wrong_qty = make_tick("AAPL", "1", "1", Side::Bid, 0);
        assert!(f.matches(&ok));
        assert!(!f.matches(&wrong_sym));
        assert!(!f.matches(&wrong_side));
        assert!(!f.matches(&wrong_qty));
    }

    #[test]
    fn test_tick_replayer_sorts_by_timestamp() {
        let ticks = vec![
            make_tick("A", "1", "1", Side::Bid, 300),
            make_tick("A", "1", "1", Side::Bid, 100),
            make_tick("A", "1", "1", Side::Bid, 200),
        ];
        let mut replayer = TickReplayer::new(ticks);
        let t1 = replayer.next_tick().unwrap();
        assert_eq!(t1.timestamp.nanos(), 100);
        let t2 = replayer.next_tick().unwrap();
        assert_eq!(t2.timestamp.nanos(), 200);
        let t3 = replayer.next_tick().unwrap();
        assert_eq!(t3.timestamp.nanos(), 300);
    }

    #[test]
    fn test_tick_replayer_next_tick_sequential() {
        let ticks = vec![
            make_tick("A", "1", "1", Side::Bid, 1),
            make_tick("A", "1", "1", Side::Bid, 2),
        ];
        let mut replayer = TickReplayer::new(ticks);
        assert!(replayer.next_tick().is_some());
        assert!(replayer.next_tick().is_some());
        assert!(replayer.next_tick().is_none());
    }

    #[test]
    fn test_tick_replayer_reset_restarts() {
        let ticks = vec![make_tick("A", "1", "1", Side::Bid, 1)];
        let mut replayer = TickReplayer::new(ticks);
        let _ = replayer.next_tick();
        assert!(replayer.next_tick().is_none());
        replayer.reset();
        assert!(replayer.next_tick().is_some());
    }

    #[test]
    fn test_tick_replayer_remaining() {
        let ticks = vec![
            make_tick("A", "1", "1", Side::Bid, 1),
            make_tick("A", "1", "1", Side::Bid, 2),
            make_tick("A", "1", "1", Side::Bid, 3),
        ];
        let mut replayer = TickReplayer::new(ticks);
        assert_eq!(replayer.remaining(), 3);
        let _ = replayer.next_tick();
        assert_eq!(replayer.remaining(), 2);
    }

    #[test]
    fn test_tick_replayer_iterator() {
        let ticks = vec![
            make_tick("A", "1", "1", Side::Bid, 1),
            make_tick("A", "2", "1", Side::Bid, 2),
            make_tick("A", "3", "1", Side::Bid, 3),
        ];
        let mut replayer = TickReplayer::new(ticks);
        let prices: Vec<_> = (&mut replayer).map(|t| t.price.value()).collect();
        assert_eq!(prices.len(), 3);
        assert_eq!(prices[0], dec!(1));
        assert_eq!(prices[1], dec!(2));
        assert_eq!(prices[2], dec!(3));
    }
}
