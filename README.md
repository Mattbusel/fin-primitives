# fin-primitives

Financial market primitives for Rust — the foundational layer for high-frequency trading and quantitative systems.

## What's inside

| Module | Description |
|--------|-------------|
| `types` | Newtypes for `Price`, `Quantity`, `Symbol`, `NanoTimestamp`, `Side` with compile-time validation |
| `orderbook` | Lock-free BTreeMap order book with `apply_delta`, spread, top-N levels, sequence tracking |
| `tick` | `Tick` ingestion, `TickFilter` (symbol/price/volume gates), `TickReplayer` for backtesting |
| `ohlcv` | OHLCV bar construction and aggregation from tick streams, multi-timeframe support |
| `signals` | SMA, EMA, RSI — streaming indicators with configurable periods |
| `position` | `Position` ledger with average-cost tracking, realized/unrealized PnL, commission handling |
| `risk` | `DrawdownTracker`, `MaxDrawdownRule`, `MinEquityRule`, composable `RiskMonitor` |

## Features

- **Zero panics** — every fallible operation returns `Result`
- **Decimal precision** — all prices and quantities use `rust_decimal` to eliminate floating-point drift
- **Nanosecond timestamps** — native `NanoTimestamp` newtype for microsecond-accurate event ordering
- **Composable risk rules** — chain multiple risk monitors; each breach is typed and detailed

## Quick start

```rust
use fin_primitives::position::{Fill, Position, PositionLedger};
use fin_primitives::risk::{MaxDrawdownRule, RiskMonitor};
use fin_primitives::types::{NanoTimestamp, Price, Quantity, Side, Symbol};
use rust_decimal_macros::dec;

let mut ledger = PositionLedger::new(dec!(100_000));
let mut monitor = RiskMonitor::new(dec!(100_000))
    .add_rule(MaxDrawdownRule { threshold_pct: dec!(10) });

// Apply a fill
ledger.apply_fill(Fill {
    symbol: Symbol::new("AAPL").unwrap(),
    side: Side::Bid,
    quantity: Quantity::new(dec!(100)).unwrap(),
    price: Price::new(dec!(175)).unwrap(),
    timestamp: NanoTimestamp(0),
    commission: dec!(1),
}).unwrap();

// Check risk after price move
let prices = [("AAPL".to_string(), Price::new(dec!(155)).unwrap())].into();
let equity = ledger.equity(&prices).unwrap();
let breaches = monitor.update(equity);
for b in &breaches {
    println!("Risk breach [{}]: {}", b.rule, b.detail);
}
```

## Add to your project

```toml
[dependencies]
fin-primitives = { git = "https://github.com/Mattbusel/fin-primitives" }
```

Or one-liner:

```ash
cargo add --git https://github.com/Mattbusel/fin-primitives
```

## Test coverage

242 tests across unit, integration, and property suites. Run with:

```bash
cargo test
```

---

> Used inside [tokio-prompt-orchestrator](https://github.com/Mattbusel/tokio-prompt-orchestrator) -- a production Rust orchestration layer for LLM pipelines. See the full [primitive library collection](https://github.com/Mattbusel/rust-crates).