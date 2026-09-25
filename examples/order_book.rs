//! Build a level-2 order book from sequenced deltas, print it as a depth ladder,
//! and show the two guarantees `apply_delta` enforces: sequence numbers must be
//! contiguous, and a delta that would cross the book is rolled back.
//!
//! ```text
//! cargo run --example order_book
//! ```
//!
//! Colors are on when stdout is a terminal. Set `NO_COLOR=1` to turn them off,
//! or `FORCE_COLOR=1` to keep them when piping.

use fin_primitives::orderbook::{BookDelta, DeltaAction, OrderBook, PriceLevel};
use fin_primitives::types::{Price, Quantity, Side, Symbol};
use fin_primitives::FinError;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

mod support;
use support::{bar, fixed, money, Paint};

fn main() -> Result<(), FinError> {
    let paint = Paint::detect();
    let mut book = OrderBook::new(Symbol::new("BTC-USD")?);

    // A snapshot arrives as a run of `Set` deltas with contiguous sequence numbers.
    let asks = [
        (dec!(64250.50), dec!(0.842)),
        (dec!(64251.00), dec!(1.310)),
        (dec!(64251.50), dec!(0.415)),
        (dec!(64252.00), dec!(2.077)),
        (dec!(64253.00), dec!(3.460)),
        (dec!(64254.50), dec!(1.125)),
        (dec!(64256.00), dec!(4.900)),
    ];
    let bids = [
        (dec!(64250.00), dec!(1.204)),
        (dec!(64249.50), dec!(0.655)),
        (dec!(64249.00), dec!(2.380)),
        (dec!(64248.00), dec!(1.720)),
        (dec!(64247.50), dec!(3.015)),
        (dec!(64246.00), dec!(2.260)),
        (dec!(64244.50), dec!(5.400)),
    ];
    let mut seq = 0;
    for (side, levels) in [(Side::Ask, &asks), (Side::Bid, &bids)] {
        for &(price, qty) in levels {
            seq += 1;
            book.apply_delta(set(side, price, qty, seq)?)?;
        }
    }

    print_ladder(&book, &paint, 7);

    // Walk the ask side to price a 5 BTC market buy.
    let mid = book.mid_price().unwrap_or_default();
    let size = Quantity::new(dec!(5))?;
    let vwap = book.vwap_for_qty(Side::Ask, size)?;
    let slippage_bps = (vwap - mid) / mid * dec!(10000);
    println!(
        "  {:<12} {:>11}  {}",
        paint.dim("buy 5 BTC"),
        paint.bold(&money(vwap, 2)),
        paint.dim(&format!(
            "VWAP walking the asks, {} bps over mid",
            slippage_bps.round_dp(2)
        ))
    );

    // Guarantee 1: a delta that would cross the book is rejected and undone.
    println!();
    let seq = book.sequence() + 1;
    let crossing = set(Side::Bid, dec!(64251.00), dec!(2.0), seq)?;
    if let Err(e) = book.apply_delta(crossing) {
        println!(
            "  {:<12} {:<20} {:<12} {}",
            paint.dim(&format!("seq {seq}")),
            "bid 64,251.00 x 2.0",
            paint.red("rejected"),
            paint.dim(&e.to_string())
        );
    }
    println!(
        "  {:<12} {:<20} {:<12} {}",
        "",
        "book unchanged",
        paint.green("rolled back"),
        paint.dim(&format!(
            "best bid still {}, seq still {}",
            money(
                book.best_bid().map_or(Decimal::ZERO, |l| l.price.value()),
                2
            ),
            book.sequence()
        ))
    );

    // Guarantee 2: a gap in the feed is an error, not a silently stale book.
    let seq = book.sequence() + 3;
    let gapped = set(Side::Ask, dec!(64250.50), dec!(0.5), seq)?;
    if let Err(e) = book.apply_delta(gapped) {
        println!(
            "  {:<12} {:<20} {:<12} {}",
            paint.dim(&format!("seq {seq}")),
            "ask 64,250.50 x 0.5",
            paint.red("rejected"),
            paint.dim(&e.to_string())
        );
    }
    Ok(())
}

fn set(side: Side, price: Decimal, qty: Decimal, sequence: u64) -> Result<BookDelta, FinError> {
    Ok(BookDelta {
        side,
        price: Price::new(price)?,
        quantity: Quantity::new(qty)?,
        action: DeltaAction::Set,
        sequence,
    })
}

fn print_ladder(book: &OrderBook, paint: &Paint, depth: usize) {
    let asks = book.top_asks(depth);
    let bids = book.top_bids(depth);
    let cum = |levels: &[PriceLevel]| -> Vec<Decimal> {
        levels
            .iter()
            .scan(Decimal::ZERO, |acc, l| {
                *acc += l.quantity.value();
                Some(*acc)
            })
            .collect()
    };
    let ask_cum = cum(&asks);
    let bid_cum = cum(&bids);
    let max = ask_cum
        .last()
        .copied()
        .unwrap_or_default()
        .max(bid_cum.last().copied().unwrap_or_default());

    println!();
    println!(
        "  {}  level-2 book, top {} levels, seq {}",
        paint.bold(book.symbol.as_str()),
        depth,
        book.sequence()
    );
    println!();
    println!(
        "  {:<4} {:>11} {:>8} {:>8}  {}",
        paint.dim("side"),
        paint.dim("price"),
        paint.dim("size"),
        paint.dim("cum"),
        paint.dim("depth")
    );
    // Asks print worst-to-best so the best ask sits right above the spread.
    for (level, total) in asks.iter().zip(&ask_cum).rev() {
        println!(
            "  {:<4} {:>11} {:>8} {:>8}  {}",
            paint.red("ask"),
            paint.red(&money(level.price.value(), 2)),
            fixed(level.quantity.value(), 3),
            fixed(*total, 3),
            paint.red(&bar(*total, max, 26))
        );
    }
    let spread = book.spread().unwrap_or_default();
    let bps = book.spread_bps().unwrap_or_default();
    let mid = book.mid_price().unwrap_or_default();
    println!(
        "  {}",
        paint.dim(&format!(
            "---- spread {} ({} bps)  mid {} ----",
            fixed(spread, 2),
            bps.round_dp(2),
            money(mid, 2)
        ))
    );
    for (level, total) in bids.iter().zip(&bid_cum) {
        println!(
            "  {:<4} {:>11} {:>8} {:>8}  {}",
            paint.green("bid"),
            paint.green(&money(level.price.value(), 2)),
            fixed(level.quantity.value(), 3),
            fixed(*total, 3),
            paint.green(&bar(*total, max, 26))
        );
    }

    println!();
    let micro = book.weighted_mid().unwrap_or_default();
    let imb = book.imbalance().unwrap_or_default();
    println!(
        "  {:<12} {:>11}  {}",
        paint.dim("mid"),
        money(mid, 2),
        paint.dim("(best bid + best ask) / 2")
    );
    println!(
        "  {:<12} {:>11}  {}",
        paint.dim("micro-price"),
        money(micro, 2),
        paint.dim("size-weighted mid")
    );
    println!(
        "  {:<12} {:>11}  {}",
        paint.dim("imbalance"),
        format!("{:+}", imb.round_dp(3)),
        paint.dim(if imb >= Decimal::ZERO {
            "bid-heavy"
        } else {
            "ask-heavy"
        })
    );
}
