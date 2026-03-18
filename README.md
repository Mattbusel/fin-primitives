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

## Module Hierarchy

Modules are layered from primitive validated types upward. Each module depends only on modules below it in this chain:

```
types  →  tick  →  orderbook
                →  ohlcv  →  signals  →  position  →  risk
```

- **types**: `Price`, `Quantity`, `Symbol`, `NanoTimestamp`, `Side` — no dependencies within the crate
- **orderbook**: depends on `types` for `Price`, `Quantity`, `Side`, `Symbol`
- **tick**: depends on `types`; provides `Tick`, `TickFilter`, `TickReplayer`
- **ohlcv**: depends on `tick` and `types`; produces `OhlcvBar` from tick streams
- **signals**: depends on `ohlcv`; `Signal` trait + `Sma`, `Ema`, `Rsi`, `SignalPipeline`
- **position**: depends on `types`; `Fill`, `Position`, `PositionLedger`
- **risk**: depends on `position` types indirectly via `PositionLedger::equity`; `DrawdownTracker`, `RiskMonitor`, `RiskRule`

### Creating a Price

```rust
use fin_primitives::types::{Price, Quantity, Symbol};
use rust_decimal_macros::dec;

let price = Price::new(dec!(150.25)).unwrap();  // strictly positive
let qty   = Quantity::new(dec!(10)).unwrap();   // non-negative
let sym   = Symbol::new("AAPL").unwrap();       // non-empty, no whitespace
println!("{}  qty={}  @ {}", sym, qty.value(), price.value());
```

### Pushing ticks to an OrderBook

```rust
use fin_primitives::orderbook::{BookDelta, DeltaAction, OrderBook};
use fin_primitives::types::{Price, Quantity, Side, Symbol};
use rust_decimal_macros::dec;

let mut book = OrderBook::new(Symbol::new("AAPL").unwrap());
book.apply_delta(BookDelta {
    side: Side::Bid, price: Price::new(dec!(150)).unwrap(),
    quantity: Quantity::new(dec!(100)).unwrap(),
    action: DeltaAction::Set, sequence: 1,
}).unwrap();
book.apply_delta(BookDelta {
    side: Side::Ask, price: Price::new(dec!(151)).unwrap(),
    quantity: Quantity::new(dec!(50)).unwrap(),
    action: DeltaAction::Set, sequence: 2,
}).unwrap();
println!("spread={} mid={}", book.spread().unwrap(), book.mid_price().unwrap());
```

### Streaming OHLCV bars

```rust
use fin_primitives::ohlcv::{OhlcvAggregator, Timeframe};
use fin_primitives::tick::Tick;
use fin_primitives::types::{NanoTimestamp, Price, Quantity, Side, Symbol};
use rust_decimal_macros::dec;

let sym = Symbol::new("AAPL").unwrap();
let mut agg = OhlcvAggregator::new(sym.clone(), Timeframe::Seconds(60)).unwrap();
let nanos_per_min = 60_000_000_000_i64;

// Two ticks within minute 0 — no bar completed yet.
agg.push_tick(&Tick::new(sym.clone(), Price::new(dec!(150)).unwrap(), Quantity::new(dec!(10)).unwrap(), Side::Ask, NanoTimestamp(0))).unwrap();
agg.push_tick(&Tick::new(sym.clone(), Price::new(dec!(152)).unwrap(), Quantity::new(dec!(5)).unwrap(),  Side::Bid, NanoTimestamp(30_000_000_000))).unwrap();

// Tick in minute 1 — triggers completion of minute-0 bar.
if let Some(bar) = agg.push_tick(&Tick::new(sym, Price::new(dec!(153)).unwrap(), Quantity::new(dec!(8)).unwrap(), Side::Ask, NanoTimestamp(nanos_per_min + 1))).unwrap() {
    println!("O={} H={} L={} C={} ticks={}", bar.open.value(), bar.high.value(), bar.low.value(), bar.close.value(), bar.tick_count);
}
```

### Computing RSI

```rust
use fin_primitives::signals::indicators::Rsi;
use fin_primitives::signals::{Signal, SignalValue};
use fin_primitives::ohlcv::OhlcvBar;
use fin_primitives::types::{NanoTimestamp, Price, Quantity, Symbol};

let mut rsi = Rsi::new("rsi14", 14);
let closes = [44.34_f64, 44.09, 44.15, 43.61, 44.33, 44.83, 45.10, 45.15,
              43.61, 44.33, 44.83, 45.10, 45.15, 43.61, 44.34];
for &c in &closes {
    let p = Price::new(c.to_string().parse().unwrap()).unwrap();
    let bar = OhlcvBar { symbol: Symbol::new("X").unwrap(), open: p, high: p, low: p, close: p,
        volume: Quantity::zero(), ts_open: NanoTimestamp(0), ts_close: NanoTimestamp(1), tick_count: 1 };
    if let SignalValue::Scalar(v) = rsi.update(&bar).unwrap() {
        println!("RSI(14) = {:.2}", v);
    }
}
```

### Tracking position PnL

```rust
use fin_primitives::position::{Fill, PositionLedger};
use fin_primitives::risk::{MaxDrawdownRule, MinEquityRule, RiskMonitor};
use fin_primitives::types::{NanoTimestamp, Price, Quantity, Side, Symbol};
use rust_decimal_macros::dec;
use std::collections::HashMap;

let mut ledger  = PositionLedger::new(dec!(100_000));
let mut monitor = RiskMonitor::new(dec!(100_000))
    .add_rule(MaxDrawdownRule { threshold_pct: dec!(10) })
    .add_rule(MinEquityRule   { floor: dec!(90_000) });

ledger.apply_fill(Fill {
    symbol: Symbol::new("AAPL").unwrap(), side: Side::Bid,
    quantity: Quantity::new(dec!(100)).unwrap(), price: Price::new(dec!(175)).unwrap(),
    timestamp: NanoTimestamp(0), commission: dec!(1),
}).unwrap();

let mut prices = HashMap::new();
prices.insert("AAPL".to_owned(), Price::new(dec!(165)).unwrap());
let equity   = ledger.equity(&prices).unwrap();
let breaches = monitor.update(equity);
for b in &breaches {
    println!("breach [{}]: {}", b.rule, b.detail);
}
```

## Indicator Reference

### Accuracy Table

| Indicator | Formula | Warm-up bars | Notes |
|-----------|---------|-------------|-------|
| **SMA(n)** | `Σ close[i] / n` over last n bars | n | Rolling `VecDeque` capped at `n`. Exactly equal to the arithmetic mean. |
| **EMA(n)** | `close × k + prev_EMA × (1 − k)`, `k = 2 / (n + 1)` | n | First n bars produce an SMA seed; subsequent bars apply the multiplier. Matches standard EMA convention (e.g. TradingView). |
| **RSI(n)** | `100 − 100 / (1 + RS)`, `RS = avg_gain / avg_loss` using Wilder smoothing: `avg = (prev_avg × (n−1) + new) / n` | n + 1 | One extra bar is required to compute the first price change. All-gain → 100; all-loss → 0; always clamped to [0, 100]. Matches Wilder (1978), TradingView, and Bloomberg. |

### Risk Module

`RiskMonitor` evaluates `Vec<Box<dyn RiskRule>>` on each equity update and returns every triggered breach.

| Rule | Trigger condition | Field |
|------|------------------|-------|
| `MaxDrawdownRule` | `drawdown_pct > threshold_pct` (strictly greater) | `threshold_pct: Decimal` |
| `MinEquityRule` | `equity < floor` (strictly less) | `floor: Decimal` |

Custom rules implement the `RiskRule` trait:

```rust
use fin_primitives::risk::{RiskBreach, RiskRule};
use rust_decimal::Decimal;

struct HaltOnLoss { limit: Decimal }

impl RiskRule for HaltOnLoss {
    fn name(&self) -> &str { "halt_on_loss" }
    fn check(&self, equity: Decimal, _dd: Decimal) -> Option<RiskBreach> {
        if equity < self.limit {
            Some(RiskBreach { rule: self.name().into(), detail: format!("equity {equity} < halt limit {}", self.limit) })
        } else {
            None
        }
    }
}
```

`DrawdownTracker` can be used standalone:

```rust
use fin_primitives::risk::DrawdownTracker;
use rust_decimal_macros::dec;

let mut tracker = DrawdownTracker::new(dec!(100_000));
tracker.update(dec!(85_000));
println!("drawdown: {}%", tracker.current_drawdown_pct()); // 15%
println!("peak: {}", tracker.peak()); // 100000
```

## Performance Notes

- **Lock-free order book** — `OrderBook` uses a `BTreeMap` with no internal synchronisation. The hot path (apply_delta) allocates only when inserting a new price level; updates to existing levels are in-place.
- **Exact arithmetic** — `rust_decimal` is used for every price and quantity. Floating-point drift is structurally impossible.
- **O(1) streaming indicators** — `Ema` and `Rsi` maintain a constant-size state regardless of history length. `Sma` uses a `VecDeque` capped at `period` elements.
- **Zero-copy tick replay** — `TickReplayer` sorts once at construction and returns shared references on each `next_tick` call; no per-tick heap allocation.
- **Composable risk without boxing** — `RiskMonitor::add_rule` boxes rules once into `Vec<Box<dyn RiskRule>>`. Evaluation is a simple linear scan; no dynamic dispatch per field access.

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
