//! The core that is always compiled, even with `default-features = false`:
//! validated types, ticks, the L2 order book, ticks-to-OHLCV aggregation, the
//! `Signal` trait and the normal distribution.
//!
//! This file must keep compiling with `cargo test --no-default-features --test core_only`,
//! so it only touches modules that are never behind a feature.

use fin_primitives::normal;
use fin_primitives::ohlcv::{OhlcvAggregator, Timeframe};
use fin_primitives::orderbook::{BookDelta, DeltaAction, OrderBook};
use fin_primitives::signals::{BarInput, Signal, SignalValue};
use fin_primitives::tick::Tick;
use fin_primitives::types::{NanoTimestamp, Price, Quantity, Side, Symbol};
use fin_primitives::FinError;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn delta(side: Side, price: Decimal, qty: Decimal, seq: u64) -> Result<BookDelta, FinError> {
    Ok(BookDelta {
        side,
        price: Price::new(price)?,
        quantity: Quantity::new(qty)?,
        action: DeltaAction::Set,
        sequence: seq,
    })
}

#[test]
fn price_and_quantity_validate_at_construction() -> Result<(), FinError> {
    assert!(matches!(Price::new(dec!(0)), Err(FinError::InvalidPrice(_))));
    assert!(matches!(Price::new(dec!(-1.5)), Err(FinError::InvalidPrice(_))));
    assert!(matches!(Quantity::new(dec!(-0.001)), Err(FinError::InvalidQuantity(_))));
    assert!(Symbol::new("   ").is_err());

    let p = Price::new(dec!(101.25))?;
    let q = Quantity::new(dec!(3))?;
    assert_eq!(p.value(), dec!(101.25));
    assert_eq!(p.checked_mul(q), Some(dec!(303.75)));
    assert_eq!(q.checked_add(Quantity::new(dec!(2))?), Some(Quantity::new(dec!(5))?));
    // Quantities never go negative: 3 - 5 is refused, not wrapped.
    assert_eq!(q.checked_sub(Quantity::new(dec!(5))?), None);
    // Zero is a valid quantity (an empty level), not a valid price.
    assert_eq!(Quantity::new(dec!(0))?.value(), Decimal::ZERO);
    Ok(())
}

#[test]
fn order_book_tracks_top_of_book_and_rejects_bad_deltas() -> Result<(), FinError> {
    let mut book = OrderBook::new(Symbol::new("BTC-USD")?);
    book.apply_delta(delta(Side::Ask, dec!(100.5), dec!(2), 1)?)?;
    book.apply_delta(delta(Side::Ask, dec!(101.0), dec!(3), 2)?)?;
    book.apply_delta(delta(Side::Bid, dec!(100.0), dec!(1), 3)?)?;
    book.apply_delta(delta(Side::Bid, dec!(99.5), dec!(4), 4)?)?;

    assert_eq!(book.best_bid_price(), Some(Price::new(dec!(100.0))?));
    assert_eq!(book.best_ask_price(), Some(Price::new(dec!(100.5))?));
    assert_eq!(book.spread(), Some(dec!(0.5)));
    assert_eq!(book.mid_price(), Some(dec!(100.25)));
    assert_eq!(book.total_levels(), 4);

    // Buying 3 takes 2 @ 100.5 and 1 @ 101.0.
    let vwap = book.vwap_for_qty(Side::Ask, Quantity::new(dec!(3))?)?;
    assert_eq!(vwap, dec!(302) / dec!(3));
    // More than the whole ask side is refused.
    assert!(book.vwap_for_qty(Side::Ask, Quantity::new(dec!(6))?).is_err());

    // A gap in the sequence is refused and changes nothing.
    let gap = book.apply_delta(delta(Side::Bid, dec!(99.0), dec!(1), 7)?);
    assert!(matches!(gap, Err(FinError::SequenceMismatch { expected: 5, got: 7 })));
    assert_eq!(book.sequence(), 4);

    // A bid through the best ask would cross the book: rolled back.
    let crossing = book.apply_delta(delta(Side::Bid, dec!(100.5), dec!(1), 5)?);
    assert!(matches!(crossing, Err(FinError::InvertedSpread { .. })));
    assert_eq!(book.sequence(), 4);
    assert_eq!(book.best_bid_price(), Some(Price::new(dec!(100.0))?));
    assert!(!book.is_crossed());

    // Removing the best ask moves the top of book.
    book.apply_delta(BookDelta {
        side: Side::Ask,
        price: Price::new(dec!(100.5))?,
        quantity: Quantity::zero(),
        action: DeltaAction::Remove,
        sequence: 5,
    })?;
    assert_eq!(book.best_ask_price(), Some(Price::new(dec!(101.0))?));
    Ok(())
}

#[test]
fn aggregator_turns_ticks_into_bars() -> Result<(), FinError> {
    let sym = Symbol::new("ETH-USD")?;
    let mut agg = OhlcvAggregator::new(sym.clone(), Timeframe::Minutes(1))?;
    let t0 = 1_767_625_200; // a minute boundary
    let trades = [
        (0, dec!(10), dec!(1)),
        (20, dec!(12), dec!(2)),
        (40, dec!(9), dec!(1)),
        (59, dec!(11), dec!(3)),
        (60, dec!(11.5), dec!(1)), // first trade of the next minute closes the first bar
    ];
    let mut closed = Vec::new();
    for (secs, px, qty) in trades {
        let tick = Tick::new(
            sym.clone(),
            Price::new(px)?,
            Quantity::new(qty)?,
            Side::Bid,
            NanoTimestamp::from_secs(t0 + secs),
        );
        closed.extend(agg.push_tick(&tick)?);
    }

    assert_eq!(closed.len(), 1);
    let bar = &closed[0];
    assert_eq!(bar.open.value(), dec!(10));
    assert_eq!(bar.high.value(), dec!(12));
    assert_eq!(bar.low.value(), dec!(9));
    assert_eq!(bar.close.value(), dec!(11));
    assert_eq!(bar.volume.value(), dec!(7));
    assert_eq!(bar.tick_count, 4);

    // The open second bar comes out on flush.
    let last = agg.flush().ok_or_else(|| FinError::InvalidInput("no open bar".into()))?;
    assert_eq!(last.open.value(), dec!(11.5));
    assert_eq!(last.tick_count, 1);
    Ok(())
}

/// A user-written indicator: the `Signal` trait is part of the core, so
/// downstream crates can implement it without the `indicators` feature.
struct RangeAvg {
    sum: Decimal,
    n: usize,
    period: usize,
}

impl Signal for RangeAvg {
    fn name(&self) -> &str {
        "range_avg"
    }
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.sum += bar.high - bar.low;
        self.n += 1;
        if self.n < self.period {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(self.sum / Decimal::from(self.n)))
    }
    fn is_ready(&self) -> bool {
        self.n >= self.period
    }
    fn period(&self) -> usize {
        self.period
    }
    fn reset(&mut self) {
        self.sum = Decimal::ZERO;
        self.n = 0;
    }
}

#[test]
fn custom_signal_runs_on_core_bars() -> Result<(), FinError> {
    let mut sig = RangeAvg { sum: Decimal::ZERO, n: 0, period: 2 };
    let a = BarInput::new(dec!(10), dec!(11), dec!(9), dec!(10), dec!(1));
    let b = BarInput::new(dec!(10), dec!(14), dec!(10), dec!(10), dec!(1));
    assert_eq!(sig.update(&a)?, SignalValue::Unavailable);
    assert_eq!(sig.update(&b)?, SignalValue::Scalar(dec!(3)));
    assert!(sig.is_ready());
    Ok(())
}

#[test]
fn normal_quantiles_without_statrs() {
    assert!((normal::inv_cdf(0.975) - 1.959_963_984_540_054).abs() < 1e-12);
    assert!((normal::cdf(normal::inv_cdf(0.01)) - 0.01).abs() < 1e-15);
}
