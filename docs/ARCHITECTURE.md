# fin-primitives architecture

<p><a href="../README.md">README</a> &middot; <a href="ARCHITECTURE.md">Architecture</a> &middot; <a href="REFERENCE.md">Module guides and API reference</a> &middot; <a href="TESTING.md">Tests and benchmarks</a> &middot; <a href="https://docs.rs/fin-primitives">docs.rs</a></p>

How the pieces connect, what each module is responsible for, and the rules the whole crate
follows. For the API of a specific type, see [docs.rs](https://docs.rs/fin-primitives).


<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://gitlab.com/mattbusel/fin-primitives/-/raw/main/assets/hero-dark.png">
    <img alt="fin-primitives: validated, decimal-precise building blocks for trading and quant systems. Shows 32 one-minute ETH-USD candles with EMA(9) aggregated from ticks, next to a BTC-USD level-2 order book." src="https://gitlab.com/mattbusel/fin-primitives/-/raw/main/assets/hero-light.png" width="100%">
  </picture>
</p>

## Architecture

<p align="center"><img alt="Data flow: ticks to TickFilter to OhlcvAggregator to Signal; BookDelta to OrderBook; Fill to PositionLedger to RiskMonitor; OptionSpec to BlackScholes; all sharing validated Price, Quantity, Symbol, NanoTimestamp and Side types" src="https://gitlab.com/mattbusel/fin-primitives/-/raw/main/assets/architecture.svg" width="100%"></p>

Each stage is plain data in and a typed value or `FinError` out, so every piece works on its
own: use the order book without the ledger, or the indicators without the aggregator. The
types are not internally synchronized; wrap a component in `Arc<Mutex<_>>` to share it
across threads.

## What is included

| Module | What it provides | Key guarantee |
|--------|-----------------|---------------|
| `types` | `Price`, `Quantity`, `Symbol`, `NanoTimestamp`, `Side` newtypes | Validation at construction; no invalid value can exist at runtime |
| `tick` | `Tick`, `TickFilter`, `TickReplayer` | Filter is pure; replayer always yields ticks in ascending timestamp order |
| `orderbook` | L2 `OrderBook` with `apply_delta`, spread, mid-price, VWAP, top-N levels | Sequence validation; inverted spreads are detected, logged, and rolled back |
| `ohlcv` | `OhlcvBar`, `Timeframe`, `OhlcvAggregator`, `OhlcvSeries` (360+ analytics) | Bar invariants (`high >= low`, etc.) enforced on every push |
| `signals` | `Signal` trait, `SignalPipeline`, **725+ built-in indicators**, `SignalMap` (60+ methods), `CompositeSignal`, **`SignalExpr` composition DSL**, **`WarmupGuard`** | Returns `Unavailable` until warm-up period is satisfied; no silent NaN |
| `position` | `Position`, `Fill`, `PositionLedger` (110+ methods) | VWAP average cost; realized and unrealized P&L net of commissions |
| `risk` | `DrawdownTracker` (90 methods), `RiskRule` trait, `RiskMonitor`, **`RiskAttributor`** (6-factor), **`BhbAttribution`** | All breaches returned as a typed `Vec<RiskBreach>`; never silently swallowed |
| `greeks` | `BlackScholes`, `OptionGreeks`, `OptionSpec`, `SpreadGreeks` | All math returns `Result<T, FinError>`; no panics on edge-case inputs |
| `backtest` | `Backtester`, `Strategy` trait, `BacktestResult`, `WalkForwardOptimizer`, `WfPeriod`, `ParamRange` | Bar-by-bar; no look-ahead; grid-search walk-forward with OOS stability score |
| `async_signals` | `StreamingSignalPipeline`, `SignalUpdate`, `spawn_signal_stream` | Tokio MPSC; pre-allocated output buffers on the hot path |
| `regime` | `RegimeDetector`, `MarketRegime`, `Garch11`, `CorrelationBreakdownDetector`, `RegimeConditionalSignal`, `RegimeHistory` | Hurst + GARCH(1,1) + cross-asset correlation breakdown; regime-adaptive RSI |

Additional modules in `src/` (see [docs.rs](https://docs.rs/fin-primitives) or the source for
their APIs): `pnl`, `correlation`, `latency`, `scenario`, `ml`, `ml_features`, `cross_asset`,
`attribution`, `options`, `volatility`, `impact`, `portfolio`, `factor`, `execution`,
`execution_cost`, `montecarlo`, `yield_curve`, `events`, `microstructure`, `crypto`,
`derivatives`, `technical`, `fixed_income`, `credit`, `liquidity`, `pairs_trading`,
`performance`, `clustering`, `tax`, `rebalancing`, `funding`, `alternative_data`,
`arbitrage`. Optional Python bindings are behind the `python` feature
(`maturin develop --features python`).

## Design principles

- **Typed errors.** Fallible operations return `Result<_, FinError>`. The crate's
  Clippy config warns on `unwrap`, `expect` and `panic`.
- **Decimal precision.** Prices and quantities use [`rust_decimal::Decimal`], so
  price arithmetic has no binary floating-point rounding. Statistical models
  (GARCH, Monte Carlo, portfolio optimization) work in `f64`.
- **Nanosecond timestamps.** `NanoTimestamp` is a newtype over `i64` nanoseconds
  since Unix epoch, suitable for microsecond-accurate event ordering and replay.
- **Composable by design.** `RiskRule`, `Signal`, and `TickFilter` are traits;
  plug in your own implementations without forking.
- **Separation of concerns.** Each module has a documented responsibility contract
  and an explicit "NOT Responsible For" section.

## Custom implementations

`RiskRule` and `Signal` are traits, so your own rules and indicators sit next to the
built-in ones.

```rust
use fin_primitives::risk::{RiskBreach, RiskRule};
use rust_decimal::Decimal;

struct HaltOnLoss {
    limit: Decimal,
}

impl RiskRule for HaltOnLoss {
    fn name(&self) -> &str {
        "halt_on_loss"
    }
    fn check(&self, equity: Decimal, _drawdown_pct: Decimal) -> Option<RiskBreach> {
        (equity < self.limit).then(|| RiskBreach {
            rule: self.name().into(),
            detail: format!("equity {equity} < halt limit {}", self.limit),
        })
    }
}
```

```rust
use fin_primitives::error::FinError;
use fin_primitives::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Close minus open of the latest bar.
struct BarBody {
    name: String,
    seen: bool,
}

impl Signal for BarBody {
    fn name(&self) -> &str {
        &self.name
    }
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.seen = true;
        Ok(SignalValue::Scalar(bar.close - bar.open))
    }
    fn is_ready(&self) -> bool {
        self.seen
    }
    fn period(&self) -> usize {
        1
    }
    fn reset(&mut self) {
        self.seen = false;
    }
}
```

## Performance notes

- **O(log n) order book mutations**: `apply_delta` performs a single `BTreeMap::insert`
  or `BTreeMap::remove`. Inverted-spread check reads two keys and does not allocate.
- **O(1) streaming indicators**: `Ema` and `Rsi` maintain constant-size state
  regardless of history length. `Sma` uses a `VecDeque` capped at `period` elements.
- **Zero-copy tick replay**: `TickReplayer` sorts once at construction and returns
  shared references on each call; no per-tick heap allocation.
