# fin-primitives tests and benchmarks

<p><a href="../README.md">README</a> &middot; <a href="ARCHITECTURE.md">Architecture</a> &middot; <a href="REFERENCE.md">Module guides and API reference</a> &middot; <a href="TESTING.md">Tests and benchmarks</a> &middot; <a href="https://docs.rs/fin-primitives">docs.rs</a></p>


```bash
cargo test                 # unit, integration and property tests
cargo test --doc           # every doc example, including the crate-level example below
cargo run --example candles
cargo bench --bench tick_bench
cargo doc --no-deps --open
```

Status on `main` as of 2026-09-25: all 255 integration tests in `tests/` and all 716
doc examples pass (one is marked `ignore`). Of 5,540 unit tests inside the modules, 18 currently fail (in
`clustering`, `events`, `execution`, `factor`, `fixed_income::swaps`, `options::greeks`,
`pairs_trading`, `portfolio`, `regime::hmm`, `risk`, `signals::multi_tf` and
`yield_curve`), so `cargo test` exits non-zero until those are fixed. The core types,
order book, aggregator, ledger and risk monitor have no failing tests.

CI (`.github/workflows/ci.yml`) runs `cargo check`, builds and runs the examples, and runs
the doc tests on every push.

Criterion microbenchmarks for tick filtering, order book deltas and indicators live in
`benches/tick_bench.rs`. No benchmark numbers are published here; run them on your own
hardware.

## The crate-level example

<details>
<summary><b>More: the crate-level example</b> (compiled and run by <code>cargo test --doc</code>)</summary>

<br>

```rust
use fin_primitives::orderbook::{BookDelta, DeltaAction, OrderBook};
use fin_primitives::types::{Price, Quantity, Side, Symbol};
use rust_decimal_macros::dec;

fn main() -> Result<(), fin_primitives::FinError> {
    let mut book = OrderBook::new(Symbol::new("BTC-USD")?);
    let levels = [
        (Side::Ask, dec!(64250.50), dec!(0.842)),
        (Side::Ask, dec!(64251.00), dec!(1.310)),
        (Side::Bid, dec!(64250.00), dec!(1.204)),
        (Side::Bid, dec!(64249.50), dec!(0.655)),
    ];
    for (seq, (side, price, qty)) in (1..).zip(levels) {
        book.apply_delta(BookDelta {
            side,
            price: Price::new(price)?,
            quantity: Quantity::new(qty)?,
            action: DeltaAction::Set,
            sequence: seq,
        })?;
    }

    assert_eq!(book.spread(), Some(dec!(0.50)));
    assert_eq!(book.mid_price(), Some(dec!(64250.25)));

    // Buying 1 BTC takes all 0.842 at the best ask and 0.158 at the next level.
    let vwap = book.vwap_for_qty(Side::Ask, Quantity::new(dec!(1))?)?;
    assert_eq!(vwap, dec!(64250.579));

    // A delta that would cross the book is rejected and rolled back.
    let crossing = BookDelta {
        side: Side::Bid,
        price: Price::new(dec!(64251.00))?,
        quantity: Quantity::new(dec!(2))?,
        action: DeltaAction::Set,
        sequence: 5,
    };
    assert!(book.apply_delta(crossing).is_err());
    assert_eq!(book.sequence(), 4);
    Ok(())
}
```

</details>
