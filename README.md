# fin-primitives

[![CI](https://github.com/Mattbusel/fin-primitives/actions/workflows/ci.yml/badge.svg)](https://github.com/Mattbusel/fin-primitives/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/fin-primitives.svg)](https://crates.io/crates/fin-primitives)
[![docs.rs](https://docs.rs/fin-primitives/badge.svg)](https://docs.rs/fin-primitives)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![codecov](https://codecov.io/gh/Mattbusel/fin-primitives/branch/main/graph/badge.svg)](https://codecov.io/gh/Mattbusel/fin-primitives)
[![Minimum Rust Version](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org)

A zero-panic, decimal-precise foundation for high-frequency trading and quantitative
systems in Rust. `fin-primitives` provides the building blocks — validated types,
order book, OHLCV aggregation, streaming technical indicators, position ledger, and
composable risk monitoring — so that upstream crates and applications can focus on
strategy rather than infrastructure.

---

## What Is Included

| Module | What it provides | Key guarantee |
|--------|-----------------|---------------|
| [`types`] | `Price`, `Quantity`, `Symbol`, `NanoTimestamp`, `Side` newtypes | Validation at construction; no invalid value can exist at runtime |
| [`tick`] | `Tick`, `TickFilter`, `TickReplayer` | Filter is pure; replayer always yields ticks in ascending timestamp order |
| [`orderbook`] | L2 `OrderBook` with `apply_delta`, spread, mid-price, VWAP, top-N levels | Sequence validation; inverted spreads are detected and rolled back |
| [`ohlcv`] | `OhlcvBar`, `Timeframe`, `OhlcvAggregator`, `OhlcvSeries` | Bar invariants (`high >= low`, etc.) enforced on every push |
| [`signals`] | `Signal` trait, `SignalPipeline`, `Sma`, `Ema`, `Rsi` | Returns `Unavailable` until warm-up period is satisfied; no silent NaN |
| [`position`] | `Position`, `Fill`, `PositionLedger` | VWAP average cost; realized and unrealized P&L net of commissions |
| [`risk`] | `DrawdownTracker`, `RiskRule` trait, `MaxDrawdownRule`, `MinEquityRule`, `RiskMonitor` | All breaches returned as a typed `Vec<RiskBreach>`; never silently swallowed |

---

## Design Principles

- **Zero panics.** Every fallible operation returns `Result<_, FinError>`.
  No `unwrap` or `expect` in production code paths.
- **Decimal precision.** All prices and quantities use [`rust_decimal::Decimal`].
  Floating-point drift is structurally impossible.
- **Nanosecond timestamps.** `NanoTimestamp` is a newtype over `i64` nanoseconds
  since Unix epoch, suitable for microsecond-accurate event ordering and replay.
- **Composable by design.** `RiskRule`, `Signal`, and `TickFilter` are traits;
  plug in your own implementations without forking.
- **Separation of concerns.** Each module has a documented responsibility contract
  and an explicit "NOT Responsible For" section.

---

## Mathematical Definitions

### Price and Quantity Types

| Type | Invariant | Backing type |
|------|-----------|-------------|
| `Price` | `d > 0` (strictly positive) | `rust_decimal::Decimal` |
| `Quantity` | `d >= 0` (non-negative) | `rust_decimal::Decimal` |
| `NanoTimestamp` | any `i64`; nanoseconds since Unix epoch (UTC) | `i64` |
| `Symbol` | non-empty, no whitespace | `String` |

### Technical Indicators

| Indicator | Formula | Warm-up bars | Notes |
|-----------|---------|-------------|-------|
| **SMA(n)** | `Σ close[i] / n` over last n bars | n | Rolling `VecDeque` capped at `n`. Exactly equal to the arithmetic mean. |
| **EMA(n)** | `close × k + prev_EMA × (1 − k)`, `k = 2 / (n + 1)` | n | First n bars produce an SMA seed; subsequent bars apply the multiplier. Matches standard EMA convention (TradingView, Bloomberg). |
| **RSI(n)** | `100 − 100 / (1 + RS)`, `RS = avg_gain / avg_loss` using Wilder smoothing: `avg = (prev_avg × (n−1) + new) / n` | n + 1 | One extra bar is required to compute the first price change. All-gain → 100; all-loss → 0; always clamped to [0, 100]. Matches Wilder (1978), TradingView, and Bloomberg. |

### OHLCV Invariants

Every `OhlcvBar` that enters an `OhlcvSeries` (via `push`) or that is returned by
`OhlcvAggregator::push_tick` has been validated to satisfy:

```
high   >=   open
high   >=   close
low    <=   open
low    <=   close
high   >=   low
```

Any bar that violates these relationships is rejected with `FinError::BarInvariant`.

### Order Book Guarantees

- Bids are maintained in descending price order (best bid = highest price).
- Asks are maintained in ascending price order (best ask = lowest price).
- Sequence numbers are strictly monotone; `delta.sequence` must equal `book.sequence() + 1`.
- A delta that would produce `best_bid >= best_ask` is rejected and the book is rolled back atomically.

### Risk Metrics

- **Drawdown %**: `(peak_equity − current_equity) / peak_equity × 100`. Always ≥ 0.
- `MaxDrawdownRule` triggers when `drawdown_pct > threshold_pct` (strictly greater).
- `MinEquityRule` triggers when `equity < floor` (strictly less).

### Position P&L

Average-cost (FIFO) method:

- **Realized P&L** (on reduce/close): `closed_qty × (fill_price − avg_cost)` for long positions.
- **Unrealized P&L**: `position_qty × (current_price − avg_cost)`.
- Both are **net of commissions**.

---

## Quickstart

Add to `Cargo.toml`:

```toml
[dependencies]
fin-primitives = "1.1"
rust_decimal_macros = "1"
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

### Example: Order book with VWAP fill

```rust
use fin_primitives::orderbook::{BookDelta, DeltaAction, OrderBook};
use fin_primitives::types::{Price, Quantity, Side, Symbol};
use rust_decimal_macros::dec;

fn main() -> Result<(), fin_primitives::FinError> {
    let mut book = OrderBook::new(Symbol::new("AAPL")?);
    book.apply_delta(BookDelta {
        side: Side::Ask,
        price: Price::new(dec!(150))?,
        quantity: Quantity::new(dec!(100))?,
        action: DeltaAction::Set,
        sequence: 1,
    })?;
    book.apply_delta(BookDelta {
        side: Side::Ask,
        price: Price::new(dec!(151))?,
        quantity: Quantity::new(dec!(50))?,
        action: DeltaAction::Set,
        sequence: 2,
    })?;

    // VWAP to fill 120 units on the ask side: 100 @ 150 + 20 @ 151 = 150.1667
    let vwap = book.vwap_for_qty(Side::Ask, Quantity::new(dec!(120))?)?;
    println!("VWAP to fill 120: {vwap}");

    println!("spread={:?}  mid={:?}", book.spread(), book.mid_price());
    Ok(())
}
```

### Example: RSI(14) computation

```rust
use fin_primitives::signals::indicators::Rsi;
use fin_primitives::signals::{Signal, SignalValue};
use fin_primitives::ohlcv::OhlcvBar;
use fin_primitives::types::{NanoTimestamp, Price, Quantity, Symbol};
use rust_decimal_macros::dec;

fn main() -> Result<(), fin_primitives::FinError> {
    let mut rsi = Rsi::new("rsi14", 14);
    // Feed 15 bars (period + 1 = 15 required before first value)
    let closes = [44, 44, 44, 43, 44, 44, 45, 45, 43, 44, 44, 45, 45, 43, 44u32];
    for c in closes {
        let p = Price::new(dec!(1) * rust_decimal::Decimal::from(c))?;
        let bar = OhlcvBar {
            symbol: Symbol::new("X")?,
            open: p, high: p, low: p, close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp(0),
            ts_close: NanoTimestamp(1),
            tick_count: 1,
        };
        if let SignalValue::Scalar(v) = rsi.update(&bar)? {
            println!("RSI(14) = {v:.2}");
        }
    }
    Ok(())
}
```

---

## API Reference

### `types` module

```rust
// Validated newtypes — construction is the only fallible step.
Price::new(d: Decimal)    -> Result<Price, FinError>       // d > 0
Quantity::new(d: Decimal) -> Result<Quantity, FinError>    // d >= 0
Quantity::zero()          -> Quantity                      // convenience
Symbol::new(s: &str)      -> Result<Symbol, FinError>      // non-empty, no whitespace
NanoTimestamp::now()      -> NanoTimestamp                 // current UTC nanoseconds
NanoTimestamp::to_datetime() -> DateTime<Utc>
```

### `tick` module

```rust
Tick::new(symbol, price, quantity, side, timestamp) -> Tick
tick.notional() -> Decimal   // price * quantity

TickFilter::new()            // matches everything
  .symbol(sym)               // restrict to symbol
  .side(side)                // restrict to side
  .min_quantity(qty)         // restrict to qty >= min
  .matches(&tick) -> bool

TickReplayer::new(ticks: Vec<Tick>) -> TickReplayer  // sorts ascending by timestamp
  .next_tick()  -> Option<&Tick>
  .remaining()  -> usize
  .reset()
```

### `orderbook` module

```rust
OrderBook::new(symbol)         -> OrderBook
  .apply_delta(delta)          -> Result<(), FinError>   // SequenceMismatch | InvertedSpread
  .best_bid()                  -> Option<PriceLevel>
  .best_ask()                  -> Option<PriceLevel>
  .spread()                    -> Option<Decimal>        // best_ask - best_bid
  .mid_price()                 -> Option<Decimal>        // (bid + ask) / 2
  .vwap_for_qty(side, qty)     -> Result<Decimal, FinError>  // InsufficientLiquidity
  .top_bids(n)                 -> Vec<PriceLevel>        // descending
  .top_asks(n)                 -> Vec<PriceLevel>        // ascending
  .sequence()                  -> u64
  .bid_count() / ask_count()   -> usize
```

### `ohlcv` module

```rust
OhlcvBar::validate()           -> Result<(), FinError>   // BarInvariant
OhlcvBar::typical_price()      -> Decimal                // (H + L + C) / 3
OhlcvBar::range()              -> Decimal                // H - L
OhlcvBar::is_bullish()         -> bool                   // close >= open

Timeframe::Seconds(n) | Minutes(n) | Hours(n) | Days(n)
Timeframe::to_nanos()          -> Result<i64, FinError>
Timeframe::bucket_start(ts)    -> Result<NanoTimestamp, FinError>

OhlcvAggregator::new(symbol, tf) -> Result<Self, FinError>
  .push_tick(&tick)            -> Result<Option<OhlcvBar>, FinError>
  .flush()                     -> Option<OhlcvBar>
  .current_bar()               -> Option<&OhlcvBar>

OhlcvSeries::new()
  .push(bar)                   -> Result<(), FinError>
  .window(n)                   -> &[OhlcvBar]
  .closes()                    -> Vec<Decimal>
  .volumes()                   -> Vec<Decimal>
```

### `signals` module

```rust
// Signal trait — implement for custom indicators
trait Signal {
    fn name(&self)   -> &str;
    fn update(&mut self, bar: &OhlcvBar) -> Result<SignalValue, FinError>;
    fn is_ready(&self) -> bool;
    fn period(&self) -> usize;
}

Sma::new(name, period)   // period bars warm-up
Ema::new(name, period)   // period bars warm-up; SMA seed
Rsi::new(name, period)   // period + 1 bars warm-up; Wilder smoothing

SignalPipeline::new()
  .add(signal)           // builder pattern
  .update(&bar)          -> Result<SignalMap, FinError>
  .ready_count()         -> usize

SignalMap::get(name)     -> Option<&SignalValue>
// SignalValue: Scalar(Decimal) | Unavailable
```

### `position` module

```rust
Position::new(symbol)
  .apply_fill(&fill)                    -> Result<Decimal, FinError>  // realized P&L
  .unrealized_pnl(current_price)        -> Decimal
  .market_value(current_price)          -> Decimal
  .is_flat()                            -> bool

PositionLedger::new(initial_cash)
  .apply_fill(fill)                     -> Result<(), FinError>   // InsufficientFunds
  .position(&symbol)                    -> Option<&Position>
  .cash()                               -> Decimal
  .realized_pnl_total()                 -> Decimal
  .unrealized_pnl_total(&prices)        -> Result<Decimal, FinError>
  .equity(&prices)                      -> Result<Decimal, FinError>
```

### `risk` module

```rust
DrawdownTracker::new(initial_equity)
  .update(equity)
  .current_drawdown_pct()   -> Decimal   // (peak - current) / peak * 100, always >= 0
  .peak()                   -> Decimal
  .is_below_threshold(pct)  -> bool

// Implement RiskRule for custom rules
trait RiskRule {
    fn name(&self) -> &str;
    fn check(&self, equity: Decimal, drawdown_pct: Decimal) -> Option<RiskBreach>;
}

MaxDrawdownRule { threshold_pct: Decimal }  // fires when dd > threshold
MinEquityRule   { floor: Decimal }          // fires when equity < floor

RiskMonitor::new(initial_equity)
  .add_rule(rule)           // builder pattern
  .update(equity)           -> Vec<RiskBreach>   // empty if compliant
```

---

## Precision and Accuracy Notes

### Decimal arithmetic

All prices and quantities use [`rust_decimal::Decimal`] (128-bit fixed-point).
This eliminates all floating-point drift:

```rust
// This is safe and exact with Decimal — never silently rounds:
let price = Price::new(dec!(150.25)).unwrap();
let qty   = Quantity::new(dec!(1000)).unwrap();
let notional = price.value() * qty.value();  // exactly 150250.00
```

### Indicator precision

- **SMA**: exact arithmetic; `sum / n` via `checked_div`. Overflow returns `FinError::ArithmeticOverflow`.
- **EMA**: multiplier `k = 2 / (n + 1)` is computed in Decimal. Small rounding error accumulates
  over very long series but is bounded by `Decimal`'s 28-digit precision.
- **RSI**: Wilder smoothing carries the same Decimal precision. Edge cases:
  - All-gains (avg_loss = 0): returns exactly 100.
  - All-losses (avg_gain = 0): returns exactly 0.
  - Always clamped to [0, 100].

### Order book VWAP

`vwap_for_qty` sweeps levels from best to worst with exact Decimal arithmetic.
Result is `total_cost / total_qty` where both accumulators are Decimal — no
intermediate f64 conversion.

---

## Performance Notes

- **O(1) order book mutations**: `apply_delta` performs a single `BTreeMap::insert`
  or `BTreeMap::remove`. The inverted-spread check reads two keys and does not allocate.
- **O(1) streaming indicators**: `Ema` and `Rsi` maintain a constant-size state
  regardless of history length. `Sma` uses a `VecDeque` capped at `period` elements.
- **Zero-copy tick replay**: `TickReplayer` sorts once at construction and returns
  shared references on each `next_tick` call; no per-tick heap allocation.
- **Composable risk without boxing overhead**: `RiskMonitor::update` is a linear scan
  over `Vec<Box<dyn RiskRule>>`; one virtual dispatch per rule per equity update.

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

All arrows represent pure data flow. No shared mutable state crosses module
boundaries. Wrap any component in `Arc<Mutex<_>>` for multi-threaded use.

---

## Running Tests

```bash
# Unit and integration tests
cargo test

# With proptest cases increased (recommended for CI)
PROPTEST_CASES=1000 cargo test

# Release-mode correctness check
cargo test --release

# Check lints
cargo clippy --all-features -- -D warnings

# Build docs locally
cargo doc --no-deps --open

# Security audit
cargo audit
```

The test suite includes unit tests in every module, integration tests in `tests/`,
and property-based tests using `proptest`.

---

## Custom Implementations

### Custom `RiskRule`

```rust
use fin_primitives::risk::{RiskBreach, RiskRule};
use rust_decimal::Decimal;

struct HaltOnLoss { limit: Decimal }

impl RiskRule for HaltOnLoss {
    fn name(&self) -> &str { "halt_on_loss" }
    fn check(&self, equity: Decimal, _dd: Decimal) -> Option<RiskBreach> {
        if equity < self.limit {
            Some(RiskBreach {
                rule: self.name().into(),
                detail: format!("equity {equity} < halt limit {}", self.limit),
            })
        } else {
            None
        }
    }
}
```

### Custom `Signal`

```rust
use fin_primitives::signals::{Signal, SignalValue};
use fin_primitives::ohlcv::OhlcvBar;
use fin_primitives::error::FinError;

struct AlwaysZero { name: String }

impl Signal for AlwaysZero {
    fn name(&self) -> &str { &self.name }
    fn update(&mut self, _bar: &OhlcvBar) -> Result<SignalValue, FinError> {
        Ok(SignalValue::Scalar(rust_decimal::Decimal::ZERO))
    }
    fn is_ready(&self) -> bool { true }
    fn period(&self) -> usize { 0 }
}
```

---

## Contributing

1. Fork the repository and create a branch from `main`.
2. All public items must have `///` doc comments explaining purpose, arguments,
   return values, and errors.
3. All fallible operations must return `Result`; no `unwrap`, `expect`, or `panic!`
   in non-test code.
4. Every new behavior must have at least one test covering the happy path and one
   covering the error/edge case.
5. Run `cargo fmt`, `cargo clippy -- -D warnings`, and `cargo test` before opening
   a pull request.
6. Update `CHANGELOG.md` under `[Unreleased]` with a brief description of your change.

---

## License

MIT — see [LICENSE](LICENSE).

> Also used inside [tokio-prompt-orchestrator](https://github.com/Mattbusel/tokio-prompt-orchestrator),
> a production Rust orchestration layer for LLM pipelines.
