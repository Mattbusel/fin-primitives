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

    /// Returns the notional value using checked arithmetic, or `None` on overflow.
    pub fn notional_checked(&self) -> Option<Decimal> {
        self.price.checked_mul(self.quantity)
    }

    /// Returns `true` if this tick represents an aggressive buy (bid-side aggressor).
    pub fn is_buy_aggressor(&self) -> bool {
        self.side == Side::Bid
    }

    /// Returns `true` if this tick represents an aggressive sell (ask-side aggressor).
    pub fn is_sell_aggressor(&self) -> bool {
        self.side == Side::Ask
    }

    /// Returns `true` if this tick is on the buy (bid) side.
    pub fn is_buy(&self) -> bool {
        self.side == Side::Bid
    }

    /// Returns `true` if this tick is on the sell (ask) side.
    pub fn is_sell(&self) -> bool {
        self.side == Side::Ask
    }

    /// Returns `true` if this tick's price is strictly higher than `prev`.
    pub fn is_uptick(&self, prev: &Tick) -> bool {
        self.price.value() > prev.price.value()
    }

    /// Returns `true` if this tick's price is strictly lower than `prev`.
    pub fn is_downtick(&self, prev: &Tick) -> bool {
        self.price.value() < prev.price.value()
    }

    /// Returns buy volume minus sell volume for a slice of ticks.
    ///
    /// Positive delta indicates net buying pressure; negative indicates net selling.
    /// Equivalent to `buy_volume - sell_volume`.
    pub fn delta(ticks: &[Tick]) -> Decimal {
        ticks.iter().map(|t| {
            match t.side {
                Side::Bid => t.quantity.value(),
                Side::Ask => -t.quantity.value(),
            }
        }).sum()
    }

    /// Returns the running cumulative delta across a tick slice.
    ///
    /// Each entry in the returned `Vec` is the running total of
    /// `buy_volume - sell_volume` up to and including that tick.
    /// An empty slice returns an empty `Vec`.
    pub fn cumulative_delta(ticks: &[Tick]) -> Vec<Decimal> {
        let mut running = Decimal::ZERO;
        ticks
            .iter()
            .map(|t| {
                match t.side {
                    Side::Bid => running += t.quantity.value(),
                    Side::Ask => running -= t.quantity.value(),
                }
                running
            })
            .collect()
    }

    /// Returns the simple (unweighted) average price from a slice of ticks.
    ///
    /// Returns `None` if the slice is empty. For volume-weighted price, use [`Tick::vwap_from_slice`].
    pub fn average_price(ticks: &[Tick]) -> Option<Decimal> {
        if ticks.is_empty() {
            return None;
        }
        #[allow(clippy::cast_possible_truncation)]
        let sum: Decimal = ticks.iter().map(|t| t.price.value()).sum();
        Some(sum / Decimal::from(ticks.len() as u32))
    }

    /// Returns the total bid-side (buy aggressor) volume from a slice of ticks.
    ///
    /// Useful for computing buy pressure and delta (buy volume − sell volume).
    pub fn buy_volume(ticks: &[Tick]) -> Decimal {
        ticks
            .iter()
            .filter(|t| t.side == Side::Bid)
            .map(|t| t.quantity.value())
            .sum()
    }

    /// Returns the total ask-side (sell aggressor) volume from a slice of ticks.
    ///
    /// Useful for computing sell pressure and delta (buy volume − sell volume).
    pub fn sell_volume(ticks: &[Tick]) -> Decimal {
        ticks
            .iter()
            .filter(|t| t.side == Side::Ask)
            .map(|t| t.quantity.value())
            .sum()
    }

    /// Computes the VWAP (volume-weighted average price) over a slice of ticks.
    ///
    /// `VWAP = Σ(price * quantity) / Σ(quantity)`
    ///
    /// Returns `None` when `ticks` is empty or total quantity is zero.
    pub fn vwap_from_slice(ticks: &[Tick]) -> Option<Decimal> {
        let total_qty: Decimal = ticks.iter().map(|t| t.quantity.value()).sum();
        if total_qty.is_zero() {
            return None;
        }
        let weighted: Decimal = ticks.iter().map(|t| t.price.value() * t.quantity.value()).sum();
        Some(weighted / total_qty)
    }

    /// Returns the highest traded price in the slice, or `None` if empty.
    pub fn max_price(ticks: &[Tick]) -> Option<Price> {
        ticks.iter().map(|t| t.price).max_by(|a, b| a.value().cmp(&b.value()))
    }

    /// Returns the lowest traded price in the slice, or `None` if empty.
    pub fn min_price(ticks: &[Tick]) -> Option<Price> {
        ticks.iter().map(|t| t.price).min_by(|a, b| a.value().cmp(&b.value()))
    }

    /// Time-Weighted Average Price from a tick slice.
    ///
    /// Each price is weighted by the elapsed nanoseconds since the previous tick.
    /// The first tick receives zero weight. Returns `None` for slices with fewer than
    /// 2 ticks or zero total elapsed time.
    pub fn time_weighted_avg_price(ticks: &[Tick]) -> Option<Decimal> {
        if ticks.len() < 2 {
            return None;
        }
        let mut total_weight = 0u128;
        let mut weighted_sum = Decimal::ZERO;
        for i in 1..ticks.len() {
            let elapsed = ticks[i].timestamp.nanos()
                .saturating_sub(ticks[i - 1].timestamp.nanos())
                .max(0) as u128;
            total_weight = total_weight.saturating_add(elapsed);
            #[allow(clippy::cast_possible_truncation)]
            let w = Decimal::from(elapsed as u64);
            weighted_sum += ticks[i].price.value() * w;
        }
        if total_weight == 0 {
            return None;
        }
        #[allow(clippy::cast_possible_truncation)]
        Some(weighted_sum / Decimal::from(total_weight as u64))
    }

    /// Returns the tick with the highest notional value (`price × quantity`) in the slice.
    ///
    /// Returns `None` if the slice is empty.
    pub fn largest_trade(ticks: &[Tick]) -> Option<&Tick> {
        ticks.iter().max_by(|a, b| {
            let na = a.price.value() * a.quantity.value();
            let nb = b.price.value() * b.quantity.value();
            na.cmp(&nb)
        })
    }

    /// Returns a static label classifying the aggressor side of this tick.
    ///
    /// - `"market_buy"` when the aggressor is the buyer (`Side::Bid`)
    /// - `"market_sell"` when the aggressor is the seller (`Side::Ask`)
    ///
    /// Useful for logging, display, and building aggressor-pressure histograms.
    pub fn classify_aggressor(&self) -> &'static str {
        match self.side {
            Side::Bid => "market_buy",
            Side::Ask => "market_sell",
        }
    }

    /// Returns buy volume as a fraction of total volume: `buy_vol / (buy_vol + sell_vol)`.
    ///
    /// Result is in `[0.0, 1.0]`. Returns `None` when total volume is zero.
    /// Values above `0.5` indicate net buying pressure; below `0.5` net selling pressure.
    pub fn imbalance_ratio(ticks: &[Tick]) -> Option<Decimal> {
        let total: Decimal = ticks.iter().map(|t| t.quantity.value()).sum();
        if total.is_zero() {
            return None;
        }
        let buy_vol = Self::buy_volume(ticks);
        Some(buy_vol / total)
    }

    /// Returns `(buy_count, sell_count)` — tick counts by aggressor side.
    ///
    /// Useful for measuring trade-frequency imbalance independently of volume.
    pub fn count_by_side(ticks: &[Tick]) -> (usize, usize) {
        let buy = ticks.iter().filter(|t| t.side == Side::Bid).count();
        let sell = ticks.len() - buy;
        (buy, sell)
    }

    /// Returns the total notional value: `Σ(price × quantity)` across all ticks.
    ///
    /// Zero when the slice is empty.
    pub fn notional_volume(ticks: &[Tick]) -> Decimal {
        ticks.iter().map(|t| t.price.value() * t.quantity.value()).sum()
    }

    /// Returns the median trade price across `ticks`.
    ///
    /// Sorts prices and returns the middle value (lower-middle for even counts).
    /// Returns `None` when the slice is empty.
    pub fn median_price(ticks: &[Tick]) -> Option<Decimal> {
        if ticks.is_empty() {
            return None;
        }
        let mut prices: Vec<Decimal> = ticks.iter().map(|t| t.price.value()).collect();
        prices.sort_unstable_by(|a, b| a.cmp(b));
        let mid = prices.len() / 2;
        if prices.len() % 2 == 0 {
            Some((prices[mid - 1] + prices[mid]) / Decimal::TWO)
        } else {
            Some(prices[mid])
        }
    }
}

/// Filters ticks by optional symbol, side, price range, and minimum quantity predicates.
///
/// All predicates are `ANDed` together. Unset predicates always pass.
#[derive(Clone)]
pub struct TickFilter {
    symbol: Option<Symbol>,
    side: Option<Side>,
    min_qty: Option<Quantity>,
    max_qty: Option<Quantity>,
    min_price: Option<Price>,
    max_price: Option<Price>,
    min_notional: Option<rust_decimal::Decimal>,
    max_notional: Option<rust_decimal::Decimal>,
    from_ts: Option<NanoTimestamp>,
    to_ts: Option<NanoTimestamp>,
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
            min_notional: None,
            max_notional: None,
            from_ts: None,
            to_ts: None,
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

    /// Restrict matches to ticks with notional (`price * quantity`) >= `n`.
    #[must_use]
    pub fn min_notional(mut self, n: rust_decimal::Decimal) -> Self {
        self.min_notional = Some(n);
        self
    }

    /// Restrict matches to ticks with notional (`price * quantity`) <= `n`.
    #[must_use]
    pub fn max_notional(mut self, n: rust_decimal::Decimal) -> Self {
        self.max_notional = Some(n);
        self
    }

    /// Restrict matches to ticks whose timestamp falls within `[from, to]` (inclusive).
    #[must_use]
    pub fn timestamp_range(mut self, from: NanoTimestamp, to: NanoTimestamp) -> Self {
        self.from_ts = Some(from);
        self.to_ts = Some(to);
        self
    }

    /// Returns `true` if a symbol predicate has been set on this filter.
    pub fn has_symbol_filter(&self) -> bool {
        self.symbol.is_some()
    }

    /// Returns `true` if a side predicate has been set on this filter.
    pub fn has_side_filter(&self) -> bool {
        self.side.is_some()
    }

    /// Returns `true` if a minimum quantity predicate has been set on this filter.
    pub fn has_min_qty_filter(&self) -> bool {
        self.min_qty.is_some()
    }

    /// Returns `true` if a price range predicate has been set on this filter.
    pub fn has_price_filter(&self) -> bool {
        self.min_price.is_some() || self.max_price.is_some()
    }

    /// Returns `true` if a notional (min or max) predicate has been set on this filter.
    pub fn has_notional_filter(&self) -> bool {
        self.min_notional.is_some() || self.max_notional.is_some()
    }

    /// Resets all predicates, returning a fresh filter that matches every tick.
    ///
    /// Allows reuse of a filter builder without allocating a new one.
    pub fn clear(self) -> Self {
        Self::new()
    }

    /// Returns `true` if no predicates are configured — the filter matches any tick.
    ///
    /// Callers can skip filter evaluation entirely when no constraints have been set,
    /// avoiding unnecessary field comparisons on every tick.
    pub fn is_empty(&self) -> bool {
        self.symbol.is_none()
            && self.side.is_none()
            && self.min_qty.is_none()
            && self.max_qty.is_none()
            && self.min_price.is_none()
            && self.max_price.is_none()
            && self.min_notional.is_none()
            && self.max_notional.is_none()
            && self.from_ts.is_none()
            && self.to_ts.is_none()
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
        if let Some(ref min_n) = self.min_notional {
            if tick.notional() < *min_n {
                return false;
            }
        }
        if let Some(ref max_n) = self.max_notional {
            if tick.notional() > *max_n {
                return false;
            }
        }
        if let Some(from) = self.from_ts {
            if tick.timestamp.is_before(from) {
                return false;
            }
        }
        if let Some(to) = self.to_ts {
            if tick.timestamp.is_after(to) {
                return false;
            }
        }
        true
    }

    /// Returns the number of ticks in `ticks` that satisfy all predicates.
    ///
    /// Equivalent to `ticks.iter().filter(|t| self.matches(t)).count()` but
    /// avoids allocating a filtered collection.
    pub fn count_matches(&self, ticks: &[Tick]) -> usize {
        ticks.iter().filter(|t| self.matches(t)).count()
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

    /// Returns a reference to the next tick without advancing the position.
    pub fn peek(&self) -> Option<&Tick> {
        self.ticks.get(self.index)
    }

    /// Returns a shared reference to all ticks in sorted order.
    pub fn ticks(&self) -> &[Tick] {
        &self.ticks
    }

    /// Resets the replayer to the beginning of the tick sequence.
    pub fn reset(&mut self) {
        self.index = 0;
    }

    /// Returns the total number of ticks (including already-yielded ones).
    pub fn count(&self) -> usize {
        self.ticks.len()
    }

    /// Returns the volume-weighted average price (VWAP) across all ticks.
    ///
    /// `VWAP = Σ(price × quantity) / Σ(quantity)`.
    ///
    /// Returns `None` if no ticks are loaded or total volume is zero.
    pub fn vwap(&self) -> Option<Decimal> {
        let total_vol: Decimal = self.ticks.iter().map(|t| t.quantity.value()).sum();
        if total_vol.is_zero() {
            return None;
        }
        let total_notional: Decimal = self.ticks.iter().map(|t| t.notional()).sum();
        Some(total_notional / total_vol)
    }

    /// Returns all ticks (from the full sorted slice) that match `filter`.
    pub fn filter_ticks(&self, filter: &TickFilter) -> Vec<Tick> {
        self.ticks
            .iter()
            .filter(|t| filter.matches(t))
            .cloned()
            .collect()
    }

    /// Returns all ticks whose timestamp falls within `[from, to]` (inclusive).
    pub fn between(&self, from: NanoTimestamp, to: NanoTimestamp) -> Vec<Tick> {
        self.ticks
            .iter()
            .filter(|t| !t.timestamp.is_before(from) && !t.timestamp.is_after(to))
            .cloned()
            .collect()
    }

    /// Returns the net delta: `buy_volume - sell_volume`.
    ///
    /// Positive = net buying pressure, negative = net selling pressure.
    pub fn delta(&self) -> Decimal {
        self.buy_volume() - self.sell_volume()
    }

    /// Returns the nanosecond time span from the first to the last tick.
    ///
    /// Returns `None` if there are fewer than 2 ticks.
    pub fn time_span_nanos(&self) -> Option<i64> {
        if self.ticks.len() < 2 {
            return None;
        }
        let first = self.ticks.first().unwrap().timestamp;
        let last = self.ticks.last().unwrap().timestamp;
        Some(last.elapsed_since(first))
    }

    /// Returns the sum of notional values (`price × quantity`) across all ticks.
    pub fn total_notional(&self) -> Decimal {
        self.ticks.iter().map(|t| t.notional()).sum()
    }

    /// Returns the total volume of all bid-side (buy) ticks.
    pub fn buy_volume(&self) -> Decimal {
        self.ticks
            .iter()
            .filter(|t| t.side == Side::Bid)
            .map(|t| t.quantity.value())
            .sum()
    }

    /// Returns the total volume of all ask-side (sell) ticks.
    pub fn sell_volume(&self) -> Decimal {
        self.ticks
            .iter()
            .filter(|t| t.side == Side::Ask)
            .map(|t| t.quantity.value())
            .sum()
    }

    /// Returns a reference to the first tick in the replay sequence, or `None` if empty.
    pub fn first(&self) -> Option<&Tick> {
        self.ticks.first()
    }

    /// Returns a reference to the last tick in the replay sequence, or `None` if empty.
    pub fn last(&self) -> Option<&Tick> {
        self.ticks.last()
    }

    /// Returns the VWAP for bid-side and ask-side ticks separately.
    ///
    /// The tuple is `(bid_vwap, ask_vwap)`. Either element is `None` if there
    /// are no ticks on that side or total volume for that side is zero.
    pub fn vwap_by_side(&self) -> (Option<Decimal>, Option<Decimal>) {
        let mut bid_notional = Decimal::ZERO;
        let mut bid_vol = Decimal::ZERO;
        let mut ask_notional = Decimal::ZERO;
        let mut ask_vol = Decimal::ZERO;
        for tick in &self.ticks {
            let vol = tick.quantity.value();
            let notional = tick.notional();
            match tick.side {
                Side::Bid => {
                    bid_notional += notional;
                    bid_vol += vol;
                }
                Side::Ask => {
                    ask_notional += notional;
                    ask_vol += vol;
                }
            }
        }
        let bid_vwap = if bid_vol.is_zero() { None } else { Some(bid_notional / bid_vol) };
        let ask_vwap = if ask_vol.is_zero() { None } else { Some(ask_notional / ask_vol) };
        (bid_vwap, ask_vwap)
    }

    /// Groups all ticks in this replayer by symbol.
    ///
    /// Returns a `HashMap` mapping each symbol to a `Vec<Tick>` in timestamp order.
    /// Ticks are cloned.
    pub fn collect_by_symbol(&self) -> std::collections::HashMap<Symbol, Vec<Tick>> {
        let mut map: std::collections::HashMap<Symbol, Vec<Tick>> = std::collections::HashMap::new();
        for tick in &self.ticks {
            map.entry(tick.symbol.clone()).or_default().push(tick.clone());
        }
        map
    }

    /// Returns the price range across all ticks: `max_price - min_price`.
    ///
    /// Returns `None` if there are no ticks.
    pub fn price_range(&self) -> Option<Decimal> {
        let mut max_p = self.ticks.first()?.price.value();
        let mut min_p = max_p;
        for t in &self.ticks {
            let p = t.price.value();
            if p > max_p { max_p = p; }
            if p < min_p { min_p = p; }
        }
        Some(max_p - min_p)
    }

    /// Returns a `(bid_count, ask_count)` tuple for the number of ticks on each side.
    pub fn tick_count_by_side(&self) -> (usize, usize) {
        let bid = self.ticks.iter().filter(|t| t.side == Side::Bid).count();
        let ask = self.ticks.iter().filter(|t| t.side == Side::Ask).count();
        (bid, ask)
    }

    /// Returns the median trade size (quantity) across all ticks.
    ///
    /// Uses the lower median for even-length sets. Returns `None` if empty.
    pub fn median_trade_size(&self) -> Option<Decimal> {
        if self.ticks.is_empty() {
            return None;
        }
        let mut sizes: Vec<Decimal> = self.ticks.iter().map(|t| t.quantity.value()).collect();
        sizes.sort();
        Some(sizes[sizes.len() / 2])
    }

    /// Returns the arithmetic mean tick quantity across all ticks.
    ///
    /// Returns `None` if there are no ticks.
    pub fn avg_trade_size(&self) -> Option<Decimal> {
        if self.ticks.is_empty() {
            return None;
        }
        let sum: Decimal = self.ticks.iter().map(|t| t.quantity.value()).sum();
        #[allow(clippy::cast_possible_truncation)]
        Some(sum / Decimal::from(self.ticks.len() as u64))
    }

    /// Returns the mean nanosecond interval between consecutive ticks.
    ///
    /// Returns `None` if there are fewer than 2 ticks.
    pub fn tick_interval_mean_nanos(&self) -> Option<i64> {
        if self.ticks.len() < 2 {
            return None;
        }
        let total = self.ticks.last().unwrap().timestamp.elapsed_since(self.ticks.first().unwrap().timestamp);
        Some(total / (self.ticks.len() as i64 - 1))
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

    #[test]
    fn test_tick_replayer_peek_does_not_advance() {
        let ticks = vec![
            make_tick("A", "1", "1", Side::Bid, 1),
            make_tick("A", "2", "1", Side::Bid, 2),
        ];
        let mut replayer = TickReplayer::new(ticks);
        let p1 = replayer.peek().map(|t| t.timestamp.nanos());
        let p2 = replayer.peek().map(|t| t.timestamp.nanos());
        assert_eq!(p1, p2, "peek must not advance the position");
        assert_eq!(replayer.remaining(), 2);
        let _ = replayer.next_tick();
        assert_eq!(replayer.remaining(), 1);
    }

    #[test]
    fn test_tick_replayer_peek_none_when_exhausted() {
        let mut replayer = TickReplayer::new(vec![]);
        assert!(replayer.peek().is_none());
    }

    #[test]
    fn test_tick_replayer_ticks_slice() {
        let ticks = vec![
            make_tick("A", "1", "1", Side::Bid, 2),
            make_tick("A", "2", "1", Side::Bid, 1),
        ];
        let replayer = TickReplayer::new(ticks);
        // ticks() returns sorted slice
        let slice = replayer.ticks();
        assert_eq!(slice.len(), 2);
        assert_eq!(slice[0].timestamp.nanos(), 1);
        assert_eq!(slice[1].timestamp.nanos(), 2);
    }

    #[test]
    fn test_tick_filter_has_symbol_filter_false_when_unset() {
        let f = TickFilter::new();
        assert!(!f.has_symbol_filter());
    }

    #[test]
    fn test_tick_filter_has_symbol_filter_true_when_set() {
        let f = TickFilter::new().symbol(Symbol::new("AAPL").unwrap());
        assert!(f.has_symbol_filter());
    }

    #[test]
    fn test_tick_filter_has_side_filter_false_when_unset() {
        let f = TickFilter::new();
        assert!(!f.has_side_filter());
    }

    #[test]
    fn test_tick_filter_has_side_filter_true_when_set() {
        let f = TickFilter::new().side(Side::Bid);
        assert!(f.has_side_filter());
    }

    #[test]
    fn test_tick_filter_has_min_qty_filter() {
        let f = TickFilter::new().min_quantity(Quantity::new(dec!(1)).unwrap());
        assert!(f.has_min_qty_filter());
    }

    #[test]
    fn test_tick_filter_has_price_filter_min() {
        let f = TickFilter::new().min_price(Price::new(dec!(10)).unwrap());
        assert!(f.has_price_filter());
    }

    #[test]
    fn test_tick_filter_has_price_filter_max() {
        let f = TickFilter::new().max_price(Price::new(dec!(100)).unwrap());
        assert!(f.has_price_filter());
    }

    #[test]
    fn test_tick_serde_roundtrip() {
        let tick = make_tick("AAPL", "150.50", "25", Side::Bid, 1_000_000_000);
        let json = serde_json::to_string(&tick).unwrap();
        let back: Tick = serde_json::from_str(&json).unwrap();
        assert_eq!(back.symbol, tick.symbol);
        assert_eq!(back.price, tick.price);
        assert_eq!(back.quantity, tick.quantity);
        assert_eq!(back.side, tick.side);
        assert_eq!(back.timestamp, tick.timestamp);
    }

    #[test]
    fn test_tick_replayer_count() {
        let ticks = vec![
            make_tick("AAPL", "100", "1", Side::Bid, 1),
            make_tick("AAPL", "101", "1", Side::Ask, 2),
            make_tick("AAPL", "102", "1", Side::Bid, 3),
        ];
        let replayer = TickReplayer::new(ticks);
        assert_eq!(replayer.count(), 3);
    }

    #[test]
    fn test_tick_replayer_count_empty() {
        let replayer = TickReplayer::new(vec![]);
        assert_eq!(replayer.count(), 0);
    }

    #[test]
    fn test_tick_replayer_filter_by_side() {
        let ticks = vec![
            make_tick("AAPL", "100", "1", Side::Bid, 1),
            make_tick("AAPL", "101", "1", Side::Ask, 2),
            make_tick("AAPL", "102", "1", Side::Bid, 3),
        ];
        let replayer = TickReplayer::new(ticks);
        let filter = TickFilter::new().side(Side::Bid);
        let filtered = replayer.filter_ticks(&filter);
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().all(|t| t.side == Side::Bid));
    }

    #[test]
    fn test_tick_replayer_filter_no_matches() {
        let ticks = vec![make_tick("AAPL", "100", "1", Side::Bid, 1)];
        let replayer = TickReplayer::new(ticks);
        let filter = TickFilter::new().side(Side::Ask);
        let filtered = replayer.filter_ticks(&filter);
        assert!(filtered.is_empty());
    }

    #[test]
    fn test_tick_filter_min_notional_passes_large() {
        let big = make_tick("AAPL", "100", "10", Side::Ask, 1); // notional = 1000
        let filter = TickFilter::new().min_notional(dec_from_str("500"));
        assert!(filter.matches(&big));
    }

    #[test]
    fn test_tick_filter_min_notional_rejects_small() {
        let small = make_tick("AAPL", "100", "1", Side::Bid, 1); // notional = 100
        let filter = TickFilter::new().min_notional(dec_from_str("500"));
        assert!(!filter.matches(&small));
    }

    #[test]
    fn test_tick_filter_is_empty_when_no_predicates() {
        let f = TickFilter::new();
        assert!(f.is_empty());
    }

    #[test]
    fn test_tick_filter_not_empty_after_symbol_set() {
        let f = TickFilter::new().symbol(Symbol::new("AAPL").unwrap());
        assert!(!f.is_empty());
    }

    #[test]
    fn test_tick_filter_not_empty_after_side_set() {
        let f = TickFilter::new().side(Side::Ask);
        assert!(!f.is_empty());
    }

    #[test]
    fn test_tick_notional_checked_matches_notional() {
        let t = make_tick("AAPL", "150.50", "10", Side::Bid, 0);
        assert_eq!(t.notional_checked(), Some(t.notional()));
    }

    #[test]
    fn test_tick_notional_checked_zero_qty() {
        let t = make_tick("AAPL", "100", "0", Side::Bid, 0);
        assert_eq!(t.notional_checked(), Some(dec!(0)));
    }

    #[test]
    fn test_tick_is_buy_bid_side() {
        let t = make_tick("AAPL", "100", "1", Side::Bid, 0);
        assert!(t.is_buy());
        assert!(!t.is_sell());
    }

    #[test]
    fn test_tick_is_sell_ask_side() {
        let t = make_tick("AAPL", "100", "1", Side::Ask, 0);
        assert!(t.is_sell());
        assert!(!t.is_buy());
    }

    #[test]
    fn test_tick_replayer_between_inclusive() {
        let ticks = vec![
            make_tick("AAPL", "100", "1", Side::Bid, 1),
            make_tick("AAPL", "101", "1", Side::Ask, 5),
            make_tick("AAPL", "102", "1", Side::Bid, 10),
        ];
        let replayer = TickReplayer::new(ticks);
        let result = replayer.between(NanoTimestamp::new(1), NanoTimestamp::new(5));
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_tick_replayer_between_no_matches() {
        let ticks = vec![make_tick("AAPL", "100", "1", Side::Bid, 100)];
        let replayer = TickReplayer::new(ticks);
        let result = replayer.between(NanoTimestamp::new(1), NanoTimestamp::new(50));
        assert!(result.is_empty());
    }

    #[test]
    fn test_tick_filter_timestamp_range() {
        let ticks = vec![
            make_tick("AAPL", "100", "1", Side::Bid, 1),
            make_tick("AAPL", "101", "1", Side::Ask, 5),
            make_tick("AAPL", "102", "1", Side::Bid, 10),
        ];
        let filter = TickFilter::new()
            .timestamp_range(NanoTimestamp::new(3), NanoTimestamp::new(10));
        let matched: Vec<_> = ticks.iter().filter(|t| filter.matches(t)).collect();
        assert_eq!(matched.len(), 2);
    }

    #[test]
    fn test_tick_replayer_first_returns_earliest() {
        let ticks = vec![
            make_tick("AAPL", "100", "1", Side::Bid, 5),
            make_tick("AAPL", "101", "1", Side::Ask, 1),
            make_tick("AAPL", "102", "1", Side::Bid, 10),
        ];
        let replayer = TickReplayer::new(ticks);
        let first = replayer.first().unwrap();
        assert_eq!(first.timestamp, NanoTimestamp::new(1));
    }

    #[test]
    fn test_tick_replayer_last_returns_latest() {
        let ticks = vec![
            make_tick("AAPL", "100", "1", Side::Bid, 5),
            make_tick("AAPL", "101", "1", Side::Ask, 1),
            make_tick("AAPL", "102", "1", Side::Bid, 10),
        ];
        let replayer = TickReplayer::new(ticks);
        let last = replayer.last().unwrap();
        assert_eq!(last.timestamp, NanoTimestamp::new(10));
    }

    #[test]
    fn test_tick_replayer_first_none_when_empty() {
        let replayer = TickReplayer::new(vec![]);
        assert!(replayer.first().is_none());
    }

    #[test]
    fn test_tick_replayer_last_none_when_empty() {
        let replayer = TickReplayer::new(vec![]);
        assert!(replayer.last().is_none());
    }

    #[test]
    fn test_tick_replayer_vwap_by_side_correct_values() {
        let ticks = vec![
            make_tick("AAPL", "100", "10", Side::Bid, 1),  // bid: notional=1000, vol=10
            make_tick("AAPL", "200", "5", Side::Ask, 2),   // ask: notional=1000, vol=5
        ];
        let replayer = TickReplayer::new(ticks);
        let (bid_vwap, ask_vwap) = replayer.vwap_by_side();
        assert_eq!(bid_vwap, Some(dec_from_str("100")));
        assert_eq!(ask_vwap, Some(dec_from_str("200")));
    }

    #[test]
    fn test_tick_replayer_vwap_by_side_no_asks_returns_none_ask() {
        let ticks = vec![make_tick("AAPL", "100", "10", Side::Bid, 1)];
        let replayer = TickReplayer::new(ticks);
        let (bid_vwap, ask_vwap) = replayer.vwap_by_side();
        assert!(bid_vwap.is_some());
        assert!(ask_vwap.is_none());
    }

    #[test]
    fn test_tick_replayer_vwap_by_side_empty_returns_none_both() {
        let replayer = TickReplayer::new(vec![]);
        let (bid_vwap, ask_vwap) = replayer.vwap_by_side();
        assert!(bid_vwap.is_none());
        assert!(ask_vwap.is_none());
    }

    #[test]
    fn test_tick_filter_clear_resets_all_predicates() {
        let f = TickFilter::new()
            .symbol(Symbol::new("AAPL").unwrap())
            .side(Side::Bid)
            .min_quantity(Quantity::new(dec!(1)).unwrap());
        let cleared = f.clear();
        assert!(cleared.is_empty());
    }

    #[test]
    fn test_tick_filter_has_notional_filter_false_when_unset() {
        let f = TickFilter::new();
        assert!(!f.has_notional_filter());
    }

    #[test]
    fn test_tick_filter_has_notional_filter_true_with_min() {
        let f = TickFilter::new().min_notional(dec_from_str("100"));
        assert!(f.has_notional_filter());
    }

    #[test]
    fn test_tick_filter_has_notional_filter_true_with_max() {
        let f = TickFilter::new().max_notional(dec_from_str("1000"));
        assert!(f.has_notional_filter());
    }

    #[test]
    fn test_tick_replayer_total_notional() {
        let ticks = vec![
            make_tick("AAPL", "100", "10", Side::Bid, 1),  // 100*10 = 1000
            make_tick("AAPL", "200", "5", Side::Ask, 2),   // 200*5  = 1000
        ];
        let replayer = TickReplayer::new(ticks);
        assert_eq!(replayer.total_notional(), dec_from_str("2000"));
    }

    #[test]
    fn test_tick_replayer_total_notional_empty() {
        let replayer = TickReplayer::new(vec![]);
        assert_eq!(replayer.total_notional(), dec_from_str("0"));
    }

    #[test]
    fn test_tick_replayer_buy_volume() {
        let ticks = vec![
            make_tick("AAPL", "100", "10", Side::Bid, 1),
            make_tick("AAPL", "100", "5", Side::Ask, 2),
        ];
        let replayer = TickReplayer::new(ticks);
        assert_eq!(replayer.buy_volume(), dec_from_str("10"));
    }

    #[test]
    fn test_tick_replayer_sell_volume() {
        let ticks = vec![
            make_tick("AAPL", "100", "10", Side::Bid, 1),
            make_tick("AAPL", "100", "7", Side::Ask, 2),
        ];
        let replayer = TickReplayer::new(ticks);
        assert_eq!(replayer.sell_volume(), dec_from_str("7"));
    }

    #[test]
    fn test_tick_replayer_delta_positive_when_more_buys() {
        let ticks = vec![
            make_tick("AAPL", "100", "10", Side::Bid, 1),
            make_tick("AAPL", "100", "3", Side::Ask, 2),
        ];
        let replayer = TickReplayer::new(ticks);
        assert_eq!(replayer.delta(), dec_from_str("7"));
    }

    #[test]
    fn test_tick_replayer_delta_negative_when_more_sells() {
        let ticks = vec![
            make_tick("AAPL", "100", "2", Side::Bid, 1),
            make_tick("AAPL", "100", "8", Side::Ask, 2),
        ];
        let replayer = TickReplayer::new(ticks);
        assert_eq!(replayer.delta(), dec_from_str("-6"));
    }

    #[test]
    fn test_tick_replayer_delta_zero_when_balanced() {
        let ticks = vec![
            make_tick("AAPL", "100", "5", Side::Bid, 1),
            make_tick("AAPL", "100", "5", Side::Ask, 2),
        ];
        let replayer = TickReplayer::new(ticks);
        assert_eq!(replayer.delta(), dec_from_str("0"));
    }

    #[test]
    fn test_tick_replayer_time_span_nanos_correct() {
        let ticks = vec![
            make_tick("AAPL", "100", "1", Side::Bid, 1_000_000),
            make_tick("AAPL", "100", "1", Side::Ask, 3_000_000),
        ];
        let replayer = TickReplayer::new(ticks);
        assert_eq!(replayer.time_span_nanos(), Some(2_000_000));
    }

    #[test]
    fn test_tick_replayer_time_span_nanos_none_for_single_tick() {
        let ticks = vec![make_tick("AAPL", "100", "1", Side::Bid, 1_000_000)];
        let replayer = TickReplayer::new(ticks);
        assert_eq!(replayer.time_span_nanos(), None);
    }

    #[test]
    fn test_tick_replayer_time_span_nanos_none_for_empty() {
        let replayer = TickReplayer::new(vec![]);
        assert_eq!(replayer.time_span_nanos(), None);
    }

    #[test]
    fn test_tick_replayer_price_range_returns_spread() {
        let ticks = vec![
            make_tick("AAPL", "100", "1", Side::Bid, 1),
            make_tick("AAPL", "105", "1", Side::Ask, 2),
            make_tick("AAPL", "98", "1", Side::Bid, 3),
        ];
        let replayer = TickReplayer::new(ticks);
        assert_eq!(replayer.price_range(), Some(dec_from_str("7")));
    }

    #[test]
    fn test_tick_replayer_price_range_none_for_empty() {
        let replayer = TickReplayer::new(vec![]);
        assert_eq!(replayer.price_range(), None);
    }

    #[test]
    fn test_tick_replayer_price_range_zero_for_single_price() {
        let ticks = vec![make_tick("AAPL", "100", "1", Side::Bid, 1)];
        let replayer = TickReplayer::new(ticks);
        assert_eq!(replayer.price_range(), Some(dec_from_str("0")));
    }

    #[test]
    fn test_tick_replayer_tick_count_by_side() {
        let ticks = vec![
            make_tick("AAPL", "100", "1", Side::Bid, 1),
            make_tick("AAPL", "100", "1", Side::Bid, 2),
            make_tick("AAPL", "100", "1", Side::Ask, 3),
        ];
        let replayer = TickReplayer::new(ticks);
        assert_eq!(replayer.tick_count_by_side(), (2, 1));
    }

    #[test]
    fn test_tick_replayer_tick_count_by_side_empty() {
        let replayer = TickReplayer::new(vec![]);
        assert_eq!(replayer.tick_count_by_side(), (0, 0));
    }

    #[test]
    fn test_tick_replayer_median_trade_size_single() {
        let ticks = vec![make_tick("AAPL", "100", "5", Side::Bid, 1)];
        let replayer = TickReplayer::new(ticks);
        assert_eq!(replayer.median_trade_size(), Some(dec_from_str("5")));
    }

    #[test]
    fn test_tick_replayer_median_trade_size_odd_count() {
        let ticks = vec![
            make_tick("AAPL", "100", "1", Side::Bid, 1),
            make_tick("AAPL", "100", "3", Side::Bid, 2),
            make_tick("AAPL", "100", "5", Side::Bid, 3),
        ];
        let replayer = TickReplayer::new(ticks);
        // Sorted: [1, 3, 5], median = index 1 = 3
        assert_eq!(replayer.median_trade_size(), Some(dec_from_str("3")));
    }

    #[test]
    fn test_tick_replayer_median_trade_size_none_for_empty() {
        let replayer = TickReplayer::new(vec![]);
        assert_eq!(replayer.median_trade_size(), None);
    }
}
