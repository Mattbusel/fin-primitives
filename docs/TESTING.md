# fin-primitives tests and benchmarks

<p><a href="../README.md">README</a> &middot; <a href="ARCHITECTURE.md">Architecture</a> &middot; <a href="REFERENCE.md">Module guides and API reference</a> &middot; <a href="TESTING.md">Tests and benchmarks</a> &middot; <a href="https://docs.rs/fin-primitives">docs.rs</a></p>


```bash
cargo test                 # unit, integration and property tests
cargo test --doc           # every doc example, including the crate-level example below
cargo run --example candles
cargo bench --bench tick_bench
cargo doc --no-deps --open
```

Status on `main` as of 2026-09-30 (2.14.3): all 5,541 unit tests, all 255 integration tests
in `tests/` and all 716 doc examples pass (one doc example is marked `ignore`), on Windows
locally and on Linux in GitLab CI. The 18 unit tests that failed through 2.14.2 were fixed in
2.14.3: five of them exposed four real bugs (silhouette score, event-study t-statistic, par-curve bootstrap,
Cornish-Fisher VaR) and thirteen had wrong expectations; see CHANGELOG.md.

CI (`.gitlab-ci.yml`) runs `cargo check`, the unit tests (`cargo test --lib`), builds and runs
the examples, and runs the doc and integration tests on every push to `main`.

Criterion microbenchmarks for tick filtering, order book deltas and indicators live in
`benches/tick_bench.rs`. No benchmark numbers are published here; run them on your own
hardware. A reproducible accuracy and speed comparison with TA-Lib lives in
[`bench/talib_compare/`](../bench/talib_compare/README.md).

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
