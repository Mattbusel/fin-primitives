# fin-primitives

[![CI](https://github.com/Mattbusel/fin-primitives/actions/workflows/ci.yml/badge.svg)](https://github.com/Mattbusel/fin-primitives/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/fin-primitives.svg)](https://crates.io/crates/fin-primitives)
[![docs.rs](https://docs.rs/fin-primitives/badge.svg)](https://docs.rs/fin-primitives)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![codecov](https://codecov.io/gh/Mattbusel/fin-primitives/branch/main/graph/badge.svg)](https://codecov.io/gh/Mattbusel/fin-primitives)

A zero-panic, decimal-precise foundation for high-frequency trading and quantitative systems in Rust. `fin-primitives` provides the building blocks -- validated types, order book, OHLCV aggregation, streaming technical indicators, position ledger, and composable risk monitoring -- so that upstream crates and applications can focus on strategy rather than infrastructure.

---

## Feature Table

| Module | What it provides | Key guarantee |
|--------|-----------------|---------------|
| `types` | `Price`, `Quantity`, `Symbol`, `NanoTimestamp`, `Side` newtypes | Validation at construction; no invalid value can exist at runtime |
| `tick` | `Tick`, `TickFilter`, `TickReplayer` | Filter is pure; replayer always yields ticks in ascending timestamp order |
| `orderbook` | L2 `OrderBook` with `apply_delta`, spread, mid-price, VWAP, top-N levels | Sequence validation; inverted spreads are detected and rolled back |
| `ohlcv` | `OhlcvBar`, `Timeframe`, `OhlcvAggregator`, `OhlcvSeries` | Bar invariants (`high >= low`, etc.) enforced on every push |
| `signals` | `Signal` trait, `SignalPipeline`, `Sma`, `Ema`, `Rsi` | Returns `Unavailable` until warm-up period is satisfied; no silent NaN |
| `position` | `Position`, `Fill`, `PositionLedger` | VWAP average cost; realized and unrealized P&L net of commissions |
| `risk` | `DrawdownTracker`, `RiskRule` trait, `MaxDrawdownRule`, `MinEquityRule`, `RiskMonitor` | All breaches returned as a typed `Vec<RiskBreach>`; never silently swallowed |

---

## Design Principles

- **Zero panics.** Every fallible operation returns `Result<_, FinError>`. No `unwrap` or `expect` in production paths.
- **Decimal precision.** All prices and quantities use [`rust_decimal::Decimal`](https://docs.rs/rust_decimal). Floating-point drift is structurally impossible.
- **Nanosecond timestamps.** `NanoTimestamp` is a newtype over `i64` nanoseconds since Unix epoch, suitable for microsecond-accurate event ordering and replay.
- **Composable by design.** `RiskRule`, `Signal`, and `TickFilter` are traits; plug in your own implementations without forking.
- **Separation of concerns.** Each module has a documented responsibility contract and an explicit "NOT Responsible For" section.

---

## Architecture Overview

```
                      Tick stream
                          |
                    TickReplayer / TickFilter
                          |
              +-----------+-----------+
              |                       |
        OhlcvAggregator          OrderBook
              |                 (apply_delta)
        OhlcvSeries                   |
              |             vwap_for_qty / spread
        SignalPipeline
        (Sma / Ema / Rsi)
              |
         SignalMap
              |
     PositionLedger (Fill)
              |
        DrawdownTracker
              |
         RiskMonitor
              |
       Vec<RiskBreach>
```

All arrows represent pure data flow. No shared mutable state crosses module boundaries. Wrap any component in `Arc<Mutex<_>>` for multi-threaded use.

---

## Quickstart

Add to `Cargo.toml`:

```toml
[dependencies]
fin-primitives = { git = "https://github.com/Mattbusel/fin-primitives" }
rust_decimal_macros = "1"
```

Or with cargo:

```bash
cargo add --git https://github.com/Mattbusel/fin-primitives fin-primitives
```

### Example: Buy, mark-to-market, check risk

```rust
use fin_primitives::position::{Fill, PositionLedger};
use fin_primitives::risk::{MaxDrawdownRule, RiskMonitor};
use fin_primitives::types::{NanoTimestamp, Price, Quantity, Side, Symbol};
use rust_decimal_macros::dec;
use std::collections::HashMap;

fn main() -> Result<(), fin_primitives::FinError> {
    let mut ledger = PositionLedger::new(dec!(100_000));
    let mut monitor = RiskMonitor::new(dec!(100_000))
        .add_rule(MaxDrawdownRule { threshold_pct: dec!(10) });

    // Execute a buy fill
    ledger.apply_fill(Fill {
        symbol: Symbol::new("AAPL")?,
        side: Side::Bid,
        quantity: Quantity::new(dec!(100))?,
        price: Price::new(dec!(175))?,
        timestamp: NanoTimestamp::now(),
        commission: dec!(1),
    })?;

    // Mark to market after adverse price move
    let mut prices = HashMap::new();
    prices.insert("AAPL".to_owned(), Price::new(dec!(155))?);
    let equity = ledger.equity(&prices)?;

    let breaches = monitor.update(equity);
    for b in &breaches {
        eprintln!("Risk breach [{}]: {}", b.rule, b.detail);
    }

    Ok(())
}
```

### Example: Tick-to-OHLCV with SMA signal

```rust
use fin_primitives::ohlcv::{OhlcvAggregator, Timeframe};
use fin_primitives::signals::SignalPipeline;
use fin_primitives::signals::indicators::Sma;
use fin_primitives::tick::Tick;
use fin_primitives::types::{NanoTimestamp, Price, Quantity, Side, Symbol};
use rust_decimal_macros::dec;

fn main() -> Result<(), fin_primitives::FinError> {
    let sym = Symbol::new("BTC")?;
    let mut agg = OhlcvAggregator::new(sym.clone(), Timeframe::Minutes(1))?;
    let mut pipeline = SignalPipeline::new().add(Sma::new("sma20", 20));

    let tick = Tick::new(
        sym,
        Price::new(dec!(65_000))?,
        Quantity::new(dec!(0.5))?,
        Side::Ask,
        NanoTimestamp::now(),
    );

    if let Some(bar) = agg.push_tick(&tick)? {
        let signals = pipeline.update(&bar)?;
        println!("sma20 = {:?}", signals.get("sma20"));
    }

    Ok(())
}
```

---

## Running Tests

```bash
# Unit and integration tests
cargo test

# With proptest cases increased (recommended for CI)
PROPTEST_CASES=1000 cargo test

# Check lints
cargo clippy --all-features -- -D warnings

# Build docs locally
cargo doc --open
```

The test suite includes unit tests in every module, integration tests in `tests/`, and property-based tests using `proptest`.

---

## Contributing

1. Fork the repository and create a branch from `main`.
2. All public items must have `///` doc comments explaining purpose, arguments, return values, and errors.
3. All fallible operations must return `Result`; no `unwrap`, `expect`, or `panic!` in non-test code.
4. Every new behavior must have at least one test covering the happy path and one covering the error/edge case.
5. Run `cargo fmt`, `cargo clippy -- -D warnings`, and `cargo test` before opening a pull request.
6. Update `CHANGELOG.md` under `[Unreleased]` with a brief description of your change.

---

> Also used inside [tokio-prompt-orchestrator](https://github.com/Mattbusel/tokio-prompt-orchestrator), a production Rust orchestration layer for LLM pipelines.
