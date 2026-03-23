# fin-primitives

## Yield Curve Modeler

The `yield_curve` module provides a full yield curve construction and analytics toolkit.

### Key Types

| Type | Description |
|------|-------------|
| `YieldPoint` | A single `(maturity_years: f64, yield_rate: f64)` observation |
| `YieldCurve` | Collection of `YieldPoint`s sorted by maturity; primary analytics surface |
| `CurveShape` | `Normal`, `Inverted`, `Flat`, `Humped` — classified from first/last yields and interior peak |
| `NelsonSiegel` | Parametric model: `beta0`, `beta1`, `beta2`, `tau` |

### Interpolation

| Method | Description |
|--------|-------------|
| `YieldCurve::linear_interp(t)` | Piecewise-linear interpolation; clamps at endpoints |
| `YieldCurve::cubic_spline(t)` | Natural cubic spline via tridiagonal (Thomas) solver |

### Analytics

| Method | Formula |
|--------|---------|
| `forward_rate(t1, t2)` | `f = (r2·t2 − r1·t1) / (t2 − t1)` |
| `duration(cash_flows)` | Macaulay: `Σ(t · CF · e^(−r·t)) / Σ(CF · e^(−r·t))` |
| `convexity(cash_flows)` | `Σ(t² · CF · e^(−r·t)) / PV` |
| `shape()` | Classifies as `Normal / Inverted / Flat / Humped` |

### Nelson-Siegel Model

```
r(t) = β₀ + β₁·(1−e^(−t/τ))/(t/τ) + β₂·((1−e^(−t/τ))/(t/τ) − e^(−t/τ))
```

`NelsonSiegel::fit(points)` fits all four parameters via gradient descent (500 iterations).

---

## Event Study Framework

The `events` module implements a standard event-study methodology for measuring abnormal
returns around discrete market events (earnings releases, guidance, macro shocks).

### Key Types

| Type | Description |
|------|-------------|
| `MarketEvent` | `event_id`, `event_date` (Unix secs), `event_type`, `description` |
| `EventWindow` | `pre_days: i32`, `post_days: i32` — e.g. `(-10, +10)` |
| `AbnormalReturn` | Per-day: `day`, `raw_return`, `expected_return`, `abnormal_return`, `car` |
| `EventResult` | Full result: `car_pre`, `car_post`, `peak_day`, `trough_day`, `abnormal_returns` |

### Methods

| Method | Description |
|--------|-------------|
| `EventStudy::compute(event, prices, benchmark, window)` | Market-model abnormal returns; benchmark return = expected return |
| `EventStudy::significance(results)` | t-statistic: `mean_CAR / (std_CAR / √N)` |

### Formulas

```
AR(d)  = raw_return(d) − benchmark_return(d)
CAR(d) = Σ AR from window_start to d
t-stat = mean(CAR) / (std(CAR) / √N)
```

---


[![CI](https://github.com/Mattbusel/fin-primitives/actions/workflows/ci.yml/badge.svg)](https://github.com/Mattbusel/fin-primitives/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/fin-primitives.svg)](https://crates.io/crates/fin-primitives)
[![docs.rs](https://docs.rs/fin-primitives/badge.svg)](https://docs.rs/fin-primitives)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Minimum Rust Version](https://img.shields.io/badge/rust-1.81%2B-orange.svg)](https://www.rust-lang.org)

A zero-panic, decimal-precise foundation for high-frequency trading and quantitative
systems in Rust. `fin-primitives` provides the building blocks: validated types,
order book, OHLCV aggregation, **725+ streaming technical indicators**, position ledger,
and composable risk monitoring — so that upstream crates and applications can focus on
strategy rather than infrastructure.

---

## Backtesting Engine

The `backtest::engine` module provides an event-driven backtester with realistic fill simulation.

### Key Types

| Type | Description |
|------|-------------|
| `BacktestEngine` | Stateless engine; call `BacktestEngine::run(signals, config)` |
| `EngineConfig` | `initial_capital`, `commission`, `slippage_bps`, `data: Vec<OhlcvBar>`, `capital_fraction` |
| `EngineSignal` | `timestamp`, `symbol`, `direction: Direction`, `strength: f64` |
| `Direction` | `Long`, `Short`, `Flat` |
| `BacktestResult` | `equity_curve: Vec<f64>`, `trades: Vec<CompletedTrade>`, `metrics: BacktestMetrics` |
| `CompletedTrade` | `entry_ts`, `exit_ts`, `direction`, `entry_price`, `exit_price`, `pnl`, `pnl_pct` |
| `BacktestMetrics` | `total_return`, `annualized_return`, `sharpe`, `sortino`, `max_drawdown`, `calmar`, `win_rate`, `profit_factor`, `avg_trade_return`, `num_trades` |

### Fill Model

- Signals fire at bar N; fills execute at bar N+1 open
- Slippage is applied symmetrically: longs pay more on entry, receive less on exit
- Commission is deducted as a fraction of notional on every fill
- Position size = `strength * capital_fraction * current_equity / fill_price`

---

## Monte Carlo Simulator

The `montecarlo` module runs N Geometric Brownian Motion price-path simulations using a seeded LCG for reproducibility.

### Key Types

| Type | Description |
|------|-------------|
| `MonteCarloSimulator` | Stateless simulator |
| `GbmParams` | `mu` (annual drift), `sigma` (annual vol), `s0` (initial price) |
| `MonteCarloConfig` | `simulations`, `horizon_days`, `seed: Option<u64>` |
| `MonteCarloResult` | `paths`, `var_95`, `cvar_95`, `median_final`, `best_case_final`, `worst_case_final` |

### Methods

| Method | Description |
|--------|-------------|
| `simulate_paths(params, config)` | Run N GBM paths; returns `Vec<Vec<f64>>` |
| `var(paths, confidence)` | Value at Risk at given confidence level |
| `cvar(paths, confidence)` | Conditional VaR (Expected Shortfall) — always ≤ VaR |
| `percentile_paths(paths, percentiles)` | Extract paths at given percentiles (e.g. `[5, 50, 95]`) |
| `run(params, config)` | Convenience: simulate + compute all metrics |

### Implementation Notes

- RNG: Linear Congruential Generator (Numerical Recipes constants)
- Normal samples: Box-Muller transform
- Same seed always produces identical paths (reproducible)
- `sigma = 0` → all paths are pure drift (deterministic)

---

## Factor Model

The `factor` module provides Fama-French style multi-factor OLS regression.

### Structs

| Type | Description |
|------|-------------|
| `Factor { name, returns }` | Named return series for one risk factor (e.g. market, value, momentum) |
| `FactorExposure { asset, betas, alpha, r_squared, residual_variance }` | OLS regression output for one asset |
| `VarianceDecomposition { systematic_variance, idiosyncratic_variance, factor_contributions }` | Variance attribution |
| `FactorPortfolio` | Aggregates per-asset exposures weighted by portfolio weights |

### Formulas

**OLS via normal equations:**

```
β̂ = (X'X)⁻¹ X'y
```

where `X` is the `T × (K+1)` design matrix (first column = intercept ones).

Matrix inversion: analytic for 2×2 and 3×3, Gaussian elimination with partial pivoting for larger systems.

**Variance decomposition:**

```
factor_contributions[i] = β_i² σ_i² + 2 Σ_{j>i} β_i β_j cov(i,j)
systematic_variance     = β' Σ β
total_variance          ≈ systematic_variance + residual_variance
```

### Quick Example

```rust
use fin_primitives::factor::{Factor, FactorModel};

let market = Factor::new("MKT", vec![0.01, -0.02, 0.015, 0.005, -0.01]);
let asset  = vec![0.008, -0.018, 0.012, 0.003, -0.009];

let exposure = FactorModel::fit("AAPL", &asset, &[market]);
println!("alpha={:.4}  beta={:.4}  R²={:.4}",
    exposure.alpha, exposure.betas[0], exposure.r_squared);
```

---

## Execution Cost Model

The `execution` module estimates round-trip trading costs and finds optimal rebalancing trades.

### Structs

| Type | Description |
|------|-------------|
| `ExecutionCost { commission_usd, spread_cost_usd, market_impact_usd, total_cost_usd, cost_bps }` | Full cost breakdown |
| `CostParams { commission_per_share, spread_bps, impact_coefficient, avg_daily_volume }` | Model parameters |
| `Trade { symbol, direction, weight_change, estimated_cost_bps }` | A single rebalancing trade |

### Formulas

```
commission_usd    = commission_per_share × shares
spread_cost_usd   = (spread_bps / 10_000) × notional_usd
impact_bps        = impact_coefficient × sqrt(shares / avg_daily_volume) × 10_000
market_impact_usd = (impact_bps / 10_000) × notional_usd
total_cost_usd    = commission_usd + spread_cost_usd + market_impact_usd
cost_bps          = total_cost_usd / notional_usd × 10_000
```

### Quick Example

```rust
use fin_primitives::execution::{CostModel, CostParams, TurnoverOptimizer};
use std::collections::HashMap;

let params = CostParams {
    commission_per_share: 0.005,
    spread_bps: 5.0,
    impact_coefficient: 0.1,
    avg_daily_volume: 1_000_000.0,
};

let cost = CostModel::estimate(100_000.0, 10_000.0, 10.0, &params);
println!("Total cost: ${:.2} ({:.1} bps)", cost.total_cost_usd, cost.cost_bps);

let current: HashMap<String, f64> = [("SPY".into(), 0.6), ("TLT".into(), 0.4)].into();
let target:  HashMap<String, f64> = [("SPY".into(), 0.5), ("TLT".into(), 0.5)].into();
let trades = TurnoverOptimizer::optimize(&current, &target, &params, 0.005);
for t in &trades {
    println!("{}: {:?} {:.1}% @ {:.1} bps", t.symbol, t.direction, t.weight_change * 100.0, t.estimated_cost_bps);
}
```

---

## What's New

### v2.17.0 — Portfolio Optimization and Kelly Criterion Position Sizing

| Change | Module | Detail |
|--------|--------|--------|
| **Portfolio Optimizer** | `portfolio::optimizer` | Markowitz mean-variance optimization — `MinVariance`, `MaxSharpe`, `RiskParity`, `EqualWeight` via projected gradient descent (200 iters, simplex projection); `CovarianceMatrix` with `ledoit_wolf_shrinkage`; `MaxWeight`, `MinWeight`, `LongOnly`, `SectorConstraint` constraints; `effective_n` (inverse HHI) |
| **Kelly Criterion Sizer** | `position::kelly` | `full_kelly`, `fractional_kelly`, `KellyResult` with position size, max loss, and expected log growth; `KellyPortfolio::allocate` — multi-asset Kelly with correlation penalty and total-fraction cap |

#### Portfolio Optimization — quick example

```rust
use fin_primitives::portfolio::{Asset, CovarianceMatrix, OptimizationObjective, Constraint, PortfolioOptimizer};

let assets = vec![
    Asset { symbol: "SPY".into(),  expected_return: 0.10, variance: 0.04 },
    Asset { symbol: "TLT".into(),  expected_return: 0.05, variance: 0.01 },
    Asset { symbol: "GLD".into(),  expected_return: 0.07, variance: 0.025 },
];

let mut cov = CovarianceMatrix::new(vec!["SPY".into(), "TLT".into(), "GLD".into()]);
cov.set(0, 0, 0.04);  cov.set(0, 1, -0.01); cov.set(0, 2, 0.005);
cov.set(1, 1, 0.01);  cov.set(1, 2, 0.002);
cov.set(2, 2, 0.025);
cov.ledoit_wolf_shrinkage();  // analytical Ledoit-Wolf shrinkage toward scaled identity

let result = PortfolioOptimizer::optimize(
    &assets,
    &cov,
    &OptimizationObjective::MaxSharpe { risk_free_rate: 0.04 },
    &[Constraint::LongOnly, Constraint::MaxWeight(0.6)],
);

println!("Sharpe: {:.3}", result.sharpe_ratio);
println!("Effective N: {:.2}", result.effective_n);  // inverse HHI diversification measure
for (sym, w) in &result.weights {
    println!("  {sym}: {:.1}%", w * 100.0);
}
```

**Math:**
- Portfolio variance: `σ²_p = w' Σ w`
- Sharpe ratio: `S = (μ_p − r_f) / σ_p`
- Ledoit-Wolf shrinkage: `Σ* = (1−α)Σ + α·μ·I` where `α = Σ_{i≠j} Σ²_{ij} / ((n+2)·Σ_{i≠j} Σ²_{ij})`
- Effective N (inverse HHI): `N* = 1 / Σ_i w²_i`

#### Kelly Criterion Position Sizing — quick example

```rust
use fin_primitives::position::{KellyInput, full_kelly, fractional_kelly, KellyPortfolio};
use fin_primitives::portfolio::CovarianceMatrix;

let input = KellyInput {
    win_probability: 0.60,   // 60 % win rate
    win_return:      1.0,    // +100 % on win
    loss_return:     1.0,    // −100 % on loss (full loss)
    bankroll:        50_000.0,
};

let full = full_kelly(&input);
println!("Full Kelly fraction: {:.1}%",  full.fraction * 100.0);  // 20.0 %
println!("Position size: ${:.0}",        full.position_size_usd);  // $10,000
println!("Max loss: ${:.0}",             full.max_loss_usd);
println!("Expected log growth: {:.4}",   full.expected_growth);

let half = fractional_kelly(&input, 0.5);
println!("Half-Kelly fraction: {:.1}%",  half.fraction * 100.0);   // 10.0 %

// Multi-asset Kelly with correlation penalty
let assets = vec![input.clone(), KellyInput { win_probability: 0.55, win_return: 1.5, loss_return: 1.0, bankroll: 50_000.0 }];
let mut cov = CovarianceMatrix::new(vec!["BTC".into(), "ETH".into()]);
cov.set(0, 1, 0.8);  // high positive correlation → penalty applied
let allocs = KellyPortfolio::allocate(&assets, &cov, 0.5);  // cap total at 50 %
```

**Math:**
- Full Kelly: `f* = (b·p − q) / b` where `b = win_return`, `p = win_probability`, `q = 1−p`
- Expected log growth: `g = p·ln(1 + b·f) + q·ln(1 − f)`
- Correlation penalty: `f̃_i = f_i / (1 + Σ_{j≠i} |ρ_{ij}|·f_j)`

---

### v2.16.0 — Signal Warmup Contracts, Signal Composition Engine, Risk Attribution

| Change | Module | Detail |
|--------|--------|--------|
| **Signal Warmup Contracts** | `signals::warmup` | `WarmupContract` trait, `WarmupGuard` (returns `Err(NotReady)` instead of `Unavailable`), `WarmupReporter` (pipeline warmup snapshot) |
| **Signal Composition Engine** | `signals::compose` | `SignalExpr` DSL, `ComposedSignal` evaluator, fluent `SignalBuilder` API for lag/normalize/threshold chains |
| **Risk Attribution** | `risk::attribution` | `RiskAttributor` decomposes portfolio risk into 6 factors; `BhbAttribution` for Brinson-Hood-Beebower P&L attribution |
| **`RiskMonitor::attribution_report`** | `risk` | Convenience method on `RiskMonitor` to get an `AttributionReport` in one call |

### v2.15.0 — Composite Signal Builder, OrderBook Diagnostic Logging

| Change | Module | Detail |
|--------|--------|--------|
| **Multi-signal composite builder** | `signals::composite` | Combine N indicators with `WeightedSum`, `All` (AND), `Any` (OR), or `First` (priority fallback) strategies using a fluent builder API |
| **OrderBook inversion diagnostic log** | `orderbook` | Inverted-spread detection now emits a `WARN` log with symbol, prices, and sequence number before rolling back — operators can correlate bad feed data without instrumenting call sites |

#### Composite signal — quick example

```rust
use fin_primitives::signals::composite::{CompositeSignal, CompositeMode};
use fin_primitives::signals::indicators::{Sma, Rsi};
use rust_decimal_macros::dec;

// 50% SMA + 50% RSI blend: returns Unavailable until both indicators warm up.
let mut blend = CompositeSignal::builder("sma_rsi_blend")
    .add(Sma::new("sma20", 20)?, dec!(0.5))
    .add(Rsi::new("rsi14", 14)?, dec!(0.5))
    .mode(CompositeMode::WeightedSum)
    .build();

// AND gate: fire only when both are non-zero.
let mut gate = CompositeSignal::builder("trend_confirm")
    .add(Sma::new("sma50", 50)?, dec!(1))
    .add(Rsi::new("rsi14", 14)?, dec!(1))
    .mode(CompositeMode::All)
    .build();
```

---

## What Is Included

| Module | What it provides | Key guarantee |
|--------|-----------------|---------------|
| [`types`] | `Price`, `Quantity`, `Symbol`, `NanoTimestamp`, `Side` newtypes | Validation at construction; no invalid value can exist at runtime |
| [`tick`] | `Tick`, `TickFilter`, `TickReplayer` | Filter is pure; replayer always yields ticks in ascending timestamp order |
| [`orderbook`] | L2 `OrderBook` with `apply_delta`, spread, mid-price, VWAP, top-N levels | Sequence validation; inverted spreads are detected, logged, and rolled back |
| [`ohlcv`] | `OhlcvBar`, `Timeframe`, `OhlcvAggregator`, `OhlcvSeries` (370+ analytics) | Bar invariants (`high >= low`, etc.) enforced on every push |
| [`signals`] | `Signal` trait, `SignalPipeline`, **725+ built-in indicators**, `SignalMap` (90+ methods), `CompositeSignal`, **`SignalExpr` composition DSL**, **`WarmupGuard`** | Returns `Unavailable` until warm-up period is satisfied; no silent NaN |
| [`position`] | `Position`, `Fill`, `PositionLedger` (145+ methods) | VWAP average cost; realized and unrealized P&L net of commissions |
| [`risk`] | `DrawdownTracker` (120+ methods), `RiskRule` trait, `RiskMonitor`, **`RiskAttributor`** (6-factor), **`BhbAttribution`** | All breaches returned as a typed `Vec<RiskBreach>`; never silently swallowed |
| [`greeks`] | `BlackScholes`, `OptionGreeks`, `OptionSpec`, `SpreadGreeks` | All math returns `Result<T, FinError>`; no panics on edge-case inputs |
| [`backtest`] | `Backtester`, `Strategy` trait, `BacktestResult`, `WalkForwardOptimizer`, `WfPeriod`, `ParamRange` | Bar-by-bar; no look-ahead; grid-search walk-forward with OOS stability score |
| [`async_signals`] | `StreamingSignalPipeline`, `SignalUpdate`, `spawn_signal_stream` | Tokio MPSC; pre-allocated output buffers on the hot path |
| [`regime`] | `RegimeDetector`, `MarketRegime`, `Garch11`, `CorrelationBreakdownDetector`, `RegimeConditionalSignal`, `RegimeHistory` | Hurst + GARCH(1,1) + cross-asset correlation breakdown; regime-adaptive RSI |

---

## Why fin-primitives?

Most financial Rust crates solve one problem. `fin-primitives` solves the whole
stack — validated domain types through streaming indicators through risk monitoring
— with a single consistent design contract:

| Concern | How fin-primitives addresses it |
|---------|--------------------------------|
| **Correctness** | `Price`/`Quantity`/`Symbol` are validated newtypes; invalid values cannot exist at runtime |
| **Precision** | All prices use `rust_decimal::Decimal`; floating-point drift is structurally impossible |
| **No surprises** | Signals return `Unavailable` — never silent NaN — until warmup is complete; `WarmupGuard` converts that to a typed `Err` |
| **Composability** | `Signal`, `RiskRule`, `TickFilter` are traits; plug in your own without forking |
| **Expressiveness** | The `SignalExpr` DSL lets you write `rsi.lag(1).normalize(ZScore).threshold(2.0, Above)` instead of bespoke structs |
| **Attribution** | The 6-factor `RiskAttributor` and BHB P&L decomposition let you see *why* your portfolio is taking risk, not just *how much* |
| **Scale** | 725+ streaming indicators, 370+ OHLCV analytics, 145+ ledger methods, 120+ drawdown statistics — all in one coherent API |
| **Safety** | `#![forbid(unsafe_code)]`; zero `unwrap`/`expect` in production paths; every error is typed and propagatable |

```
"The goal is to make correctness the path of least resistance."
```

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

## Quickstart

Add to `Cargo.toml`:

```toml
[dependencies]
fin-primitives = "2.9"
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

    ledger.apply_fill(Fill {
        symbol: Symbol::new("AAPL")?,
        side: Side::Bid,
        quantity: Quantity::new(dec!(100))?,
        price: Price::new(dec!(175))?,
        timestamp: NanoTimestamp::now(),
        commission: dec!(1),
    })?;

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

### Example: RSI(14) computation

```rust
use fin_primitives::signals::indicators::Rsi;
use fin_primitives::signals::{Signal, SignalValue};
use fin_primitives::ohlcv::OhlcvBar;
use fin_primitives::types::{NanoTimestamp, Price, Quantity, Symbol};
use rust_decimal_macros::dec;

fn main() -> Result<(), fin_primitives::FinError> {
    let mut rsi = Rsi::new("rsi14", 14);
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

## Technical Indicators (725+)

All indicators implement the `Signal` trait and return `SignalValue::Unavailable`
until warm-up is satisfied. No silent NaN or panic.

**Trend / Moving Averages**

`Sma`, `Ema`, `Dema`, `Tema`, `Wma`, `HullMa`, `Alma`, `Smma`, `Zlema`, `T3`,
`Trima`, `Kama`, `Lsma`, `Vidya`, `Swma`, `McGinley`, `LinRegSlope`, `Frama`,
`DemaRatio`, `DemaCross`, `EmaCross`, `EmaSlope`, `EmaConvergence`, `TypicalPriceMa`,
`TrueRangeEma`, `CoralTrend`, `HalfTrend`, `MesaAdaptiveMa`, `JurikMa`,
`ChandeKrollStop`, `EmaRatio`, `EmaAlignment`, `EmaBandWidth`, `SmaDistancePct`,
`TrendMagic`, `AdaptiveSupertrend`, `RollingVwap`

**Momentum / Oscillators**

`Rsi`, `Macd`, `Cci`, `Roc`, `Momentum`, `Apo`, `Ppo`, `Cmo`, `Tsi`, `Rvi`,
`StochasticK`, `StochasticD`, `StochRsi`, `StochRsiSmoothed`, `WilliamsR`,
`UltimateOscillator`, `Coppock`, `Kst`, `Trix`, `Dpo`, `Pgo`, `Rmi`, `Cog`,
`Pfe`, `ConnorsRsi`, `DualRsi`, `RsiMa`, `RsiDivergence`, `SmoothedRsi`,
`AdaptiveRsi`, `RsiStochastic`, `VolumeWeightedRsi`, `Qqe`, `Pmo`, `Tii`,
`AwesomeOscillator`, `Smi`, `Ctm`, `PriceMomentumOscillator`, `MomentumOscillator`,
`DeltaMomentum`, `CumReturnMomentum`, `NormalizedMomentum`, `MomentumQuality`,
`MomentumReversal`, `MomentumStreak`, `MomentumDivergence`, `MomentumConsistency`,
`UpMomentumPct`, `BodyMomentum`, `SlopeOscillator`, `EhlersCyberCycle`,
`ChandeForecastOsc`, `ChandeMomentumSmoothed`, `DynamicMomentumIndex`

**Volatility**

`Atr`, `Natr`, `BollingerB`, `BollingerPctB`, `BollingerWidth`, `KeltnerChannel`,
`DonchianMidpoint`, `DonchianWidth`, `Vhf`, `ChoppinessIndex`, `HistoricalVolatility`,
`RelativeVolatility`, `ChaikinVolatility`, `VolatilityRatio`, `VolatilityBands`,
`VolatilityAdjustedMomentum`, `VolatilitySkew`, `StdDevChannel`, `LinRegChannel`,
`Inertia`, `Stiffness`, `TtmSqueeze`, `VolatilityOfVolatility`, `VolatilityBreak`,
`VolatilityMomentum`, `VolatilityPercentile`, `VolatilityRegimeDetector`,
`VolatilitySpike`, `VolatilityStop`, `RegimeVolatility`, `LogReturnVolatility`,
`WeightedCloseVolatility`, `AccelerationBands`, `AtrPercent`, `AtrNormalizedClose`,
`AtrRatio`, `DualATRRatio`, `WilderSmoothedRange`, `TrueRangeExpansion`,
`TrueRangePercentile`, `TrueRangeZScore`, `TrueRangeRatio`

**Volume**

`Cmf`, `Obv`, `Mfi`, `Vwap`, `Vwma`, `Pvo`, `Emv`, `Kvo`, `Vpt`, `Nvi`,
`ChaikinOsc`, `ForceIndex`, `NetVolume`, `VolumeRsi`, `VolumeSpike`,
`VolumeTrend`, `VolumeOscillator`, `VolumeImbalance`, `Vroc`, `ObvMomentum`,
`ClimaxVolume`, `BwMfi`, `Vzo`, `VwMomentum`, `VolumeBreadth`, `VolumeAcceleration`,
`VolumeWeightedClose`, `VolumeAccumulation`, `VolumeDeltaOscillator`,
`VolumeToRangeRatio`, `VolumeRateOfChange`, `VolumeSpikeRatio`, `VolumeSpikeScore`,
`VolumeReturnCorrelation`, `VolumeTrendSlope`, `VolumePriceEfficiency`,
`VolumePriceCorr`, `VolumePriceImpact`, `VolumeDirectionRatio`, `VolumeEnergy`,
`VolumeExhaustion`, `VolumeFlowRatio`, `VolumeDensity`, `VolumeDeviation`,
`VolumeClimaxRatio`, `VolumeMomentum`, `VolumeMomentumDivergence`,
`VolumeOpenBias`, `VolumePerRange`, `VolumeRatioSignal`, `VolumeSurge`,
`VolumeSurge2`, `VolumeUpDownRatio`, `VolumeWeightedAtr`, `VolumeWeightedRange`,
`VolumeWeightedStdDev`, `VolumeWeightedMomentum`, `UpVolumeFraction`,
`UpVolumeRatio`, `UpDownVolumeRatio`, `NegativeVolumeIndex`, `PositiveVolumeIndex`,
`RelativeVolumeRank`, `RelativeVolumeScore`, `NormalizedVolume`, `MedianVolume`,
`CumulativeVolume`, `CumulativeDelta`, `ConsecutiveVolumeGrowth`, `VolumeStreakCount`,
`RollingVolumeCV`, `DeltaVolume`

**Trend Direction / Multi-component**

`Adx`, `Dmi`, `Aroon`, `AroonOscillator`, `Ichimoku`, `ParabolicSar`, `SuperTrend`,
`ElderRay`, `ElderImpulse`, `ChandelierExit`, `Stc`, `Vortex`, `WilliamsAD`,
`GannHiLo`, `TrendFollowingFilter`, `TrendStrength`, `TrendAngle`, `TrendScore`,
`Alligator`, `Rwi`, `TrendAge`, `TrendConsistency`, `TrendConsistencyScore`,
`TrendPersistence`, `TrendPurity`, `MarketRegimeFilter`, `NetHighLowCount`,
`BullBearBalance`, `TdSequential`, `WilliamsFractal`, `KeyReversal`

**Price Structure / Pattern**

`PriceChannel`, `PriceCompression`, `PriceDistanceMa`, `PriceGap`, `PriceIntensity`,
`PriceOscillator`, `PriceOscillator2`, `PricePosition`, `PriceRangePct`,
`PriceAboveMa`, `PriceAboveMaPct`, `PriceAcceleration`, `PriceVelocity`,
`PriceVelocityRatio`, `PriceVelocityScore`, `PriceEnvelope`, `PriceReversal`,
`PriceReversalStrength`, `NormalizedPrice`, `DisparityIndex`, `DeviationFromMa`,
`LinearDeviation`, `PriceDensity`, `CandleBodySize`, `CandleColor`, `CandleMomentum`,
`CandlePattern`, `HeikinAshi`, `WickRatio`, `HighLowPct`, `HighLowPctRange`,
`HighLowSpread`, `HlRatio`, `OpenCloseRatio`, `CloseToOpen`, `CloseLocationValue`,
`WeightedClose`, `CloseToOpenGap`, `CloseToOpenReturn`, `HighLowReturnCorrelation`,
`UpperWickPct`, `LowerWickPct`, `HigherHighLowerLow`, `OpenHighLowCloseAvg`,
`CloseToLowDistance`, `ReturnMeanDeviation`, `PriceAboveRollingHigh`,
`OpenCloseSpread`, `GapFillRatio`, `PriceCompressionRatio`, `ShadowRatio`,
`PriceMeanDeviation`, `AbsReturnSum`, `AbsReturnMean`, `RollingMaxDrawdown`,
`PriceRelativeStrength`, `OpenLowRange`, `HighOpenRange`, `BodyAtrRatio`,
`GapStreak`, `BarEfficiency`, `MedianBodySize`, `WickAsymmetryStreak`,
`FibonacciRetrace`, `PriceEntropyScore`, `PriceCompressionIndex`,
`PriceCompressionBreakout`, `PriceSymmetry`, `PricePathEfficiency`,
`PriceEfficiencyRatio`, `PriceGravity`, `PriceImpulse`, `PriceBandwidth`,
`PriceLevelPct`, `PricePositionRank`, `PriceRangeExpansion`, `PriceRangeRank`,
`PriceToSmaRatio`, `PriceZScore`, `PriceOscillatorPct`, `PriceOscillatorSign`,
`PriceChangeCount`, `PriceChangePct`, `PriceChannelPosition`, `PriceChannelWidth`,
`PriceGapFrequency`, `OpenToHighRatio`, `RangeMomentum`, `RangePersistence`,
`RangeReturnRatio`, `RangeCompressionRatio`, `RangeContractionCount`,
`RangeExpansionIndex`, `RangeMidpointPosition`, `RangePctOfClose`,
`RangeTrendSlope`, `RangeZScore`, `RangeEfficiency`, `RangeBreakoutCount`,
`RangeReturnRatio`, `CloseMidpointDiff`, `CloseMidpointStrength`,
`CloseAboveMidpoint`, `CloseVsOpenRange`, `CloseVsPriorHigh`, `CloseVsVwap`,
`ClosePositionInRange`, `CloseRetracePct`, `CloseReturnAcceleration`,
`CloseReturnZ`, `CloseToHighRatio`, `CloseToMidRange`, `CloseToRangeTop`,
`CloseRelativeToEma`, `CloseRelativeToRange`, `ClosePctFromHigh`, `ClosePctFromLow`,
`CloseAboveEma`, `CloseAboveOpen`, `CloseAbovePrevClose`, `CloseAbovePrevClosePct`,
`CloseAbovePrevHigh`, `CloseAbovePriorClose`, `CloseAboveSmaStreak`,
`CloseAboveHighPrev`, `CloseBelowLowPrev`, `CloseDistanceFromEma`, `CloseDistanceFromOpen`, `CloseHighFrequency`,
`CloseMinusOpenMa`, `CloseOpenEma`, `CloseAcceleration`, `CloseAccelerationSign`,
`OpenAbovePrevClose`, `OpenCloseMomentum`, `OpenGapDirection`, `OpenGapPct`,
`OpenGapSize`, `OpenHighRatio`, `OpenRangeStrength`, `OpenToCloseRatio`,
`OpenToCloseReturn`, `OpenCloseSymmetry`, `OpenDrive`, `OpenMidpointDeviation`,
`OvernightReturn`, `IntrabarReturn`,
`HighBreakCount`, `HigherCloseStreak`, `HigherHighCount`, `HigherLowCount`,
`HigherLowStreak`, `HighLowCrossover`, `HighLowDivergence`, `HighLowMidpoint`,
`HighLowOscillator`, `HighOfPeriod`, `LowOfPeriod`, `LowerHighCount`,
`LowerHighStreak`, `LowerLowCount`, `LowerShadowRatio`, `UpperShadowRatio`,
`UpperToLowerWick`, `ShadowImbalance`, `WickImbalance`, `WickToAtrRatio`,
`WickToBodyRatio`, `WickRejectionScore`, `BodyDirectionRatio`, `BodyFillRatio`,
`BodyHeightRatio`, `BodySizeRank`, `BodyStreak`, `BodyToRangeRatio`,
`BarCloseRank`, `BarFollowThrough`, `BarMomentumIndex`, `BarMomentumScore`,
`BarOpenPosition`, `BarOverlapRatio`, `BarRangeConsistency`, `BarRangeExpansionPct`,
`BarRangeStdDev`, `BarStrengthIndex`, `BarType`, `BearishBarRatio`,
`BodyPosition`, `BodyToShadowRatio`, `HighVolumeBarRatio`,
`CandleEfficiency`, `CandleRangeMa`, `CandleSymmetry`, `FlatBarPct`,
`NarrowRangeBar`, `UpBarRatio`, `NetBarBias`, `ThreeBarPattern`,
`EngulfingDetector`, `EngulfingPattern`, `HammerDetector`, `HammerPattern`,
`DojiDetector`, `InsideBarCounter`, `InsideBarRatio`, `OutsideBarCount`

**Statistical / Adaptive**

`StdDev`, `PercentRank`, `Fisher`, `MassIndex`, `PsychologicalLine`, `KaufmanEr`,
`ZScore`, `Bop`, `Atrp`, `Envelope`, `Pivots`, `PivotDistance`, `PivotPoint`,
`PivotStrength`, `SupportResistanceDistance`, `AtrStop`, `ChangeFromHigh`,
`BarsSince`, `ConsecutiveBars`, `SwingIndex`, `Dsp`, `Usm`, `Vam`,
`LinRegR2`, `UlcerIndex`, `MeanReversionScore`, `MaxDrawdownWindow`,
`MaxAdverseExcursion`, `MaxDrawupWindow`, `RangeFilter`, `RangeRatio`,
`GapDetector`, `GapFillDetector`, `GapMomentum`, `GapRangeRatio`, `GapSignal`,
`SignedGapSum`, `AverageGap`, `AnchoredVwap`, `LaguerreRsi`, `BullBearPower`,
`BullPowerBearPower`, `VixFix`, `RocRatio`, `TypicalPrice`, `TypicalPriceDeviation`,
`MedianPrice`, `MedianCloseDev`, `MedianReturnDeviation`, `RollingMAD`,
`RollingKurtosis`, `RollingSkewness`, `RollingReturnKurtosis`, `RollingSkewReturns`,
`RollingMaxReturn`, `RollingMinReturn`, `RollingCorrelation`, `RollingHighLowPosition`,
`RollingHighLowRatio`, `RollingLowBreak`, `RollingOpenBias`, `RollingMaxDd`,
`AutoCorrelation1`, `ReturnAutoCorrelation`, `ReturnDispersion`, `ReturnIqr`,
`ReturnPersistence`, `ReturnSignChanges`, `ReturnSignSum`, `ReturnAboveZeroPct`,
`ReturnOverVolatility`, `ReturnPercentRank`, `CumulativeLogReturn`,
`DailyReturnSkew`, `DirectionChanges`, `DirectionalEfficiency`, `EfficiencyRatio`, `DownsideDeviation`,
`EaseOfMovement`, `FairValueGap`, `HurstExponent`, `AverageBarRange`,
`AverageGain`, `AverageLoss`, `AmplitudeRatio`, `Zscore`, `ZigZag`,
`ValueAtRisk5`, `ConditionalVar5`, `PayoffRatio`, `ProfitFactor`,
`VarianceRatio`, `ConsolidationScore`, `SupportTestCount`,
`CusumPriceChange`, `NewHighPct`, `NewHighStreak`, `NewLowPct`,
`RelativeBarRange`, `RelativeClose`, `TailRatio`, `TailRatioPct`,
`BreakoutSignal`, `MidpointOscillator`, `IntradaySpreadPct`,
`OhlcSpread`, `RobustZScore`, `RollingShadowBalance`, `AtrPercentile`

**Core formulas:**

| Indicator | Formula | Warm-up bars |
|-----------|---------|-------------|
| **SMA(n)** | `sum(close, n) / n` | n |
| **EMA(n)** | `close × k + prev × (1−k)`, `k = 2/(n+1)` | n |
| **RSI(n)** | `100 − 100 / (1 + avg_gain / avg_loss)` Wilder smoothing | n + 1 |
| **ATR(n)** | Wilder-smoothed true range | n |
| **MACD(f,s,sig)** | `EMA(f) − EMA(s)`; signal = `EMA(sig)` of MACD | slow + signal |
| **Fibonacci(n)** | Swing high/low over `n` bars; 0/23.6/38.2/50/61.8/100% levels | n |
| **VolumeReturnCorrelation(n)** | Pearson r between close return and volume | n + 1 |
| **PriceEntropyScore(n)** | Shannon entropy of up/flat/down bins, normalized to [0,1] | n + 1 |
| **VolatilityOfVolatility(n)** | Std dev of rolling ATR values | 2n − 1 |

---

## OhlcvSeries Analytics (370+)

`OhlcvSeries` ships an extensive built-in analytics library. A selection:

**Returns & Volatility**: `realized_volatility`, `rolling_sharpe`, `hurst_exponent`,
`ulcer_index`, `cvar`, `skewness`, `kurtosis`, `autocorrelation`, `std_dev`,
`close_returns`, `log_returns`, `drawdown_series`, `max_drawdown`, `max_drawdown_pct`

**Volume**: `vwap`, `vwap_deviation`, `volume_price_correlation`, `relative_volume`,
`volume_spike`, `up_down_volume_ratio`, `net_volume`, `volume_weighted_return`,
`close_above_vwap_pct`, `volume_coefficient_of_variation`, `avg_volume_on_up_bars`,
`avg_volume_on_down_bars`

**Momentum & Trend**: `close_momentum`, `price_velocity`, `price_acceleration`,
`close_momentum_ratio`, `recent_close_trend`, `trend_strength`, `trend_consistency`,
`momentum_score`, `close_above_ma_streak`, `bars_above_ma`, `bars_above_sma`

**Candle Patterns**: `count_doji`, `pct_doji`, `bullish_engulfing_count`,
`bearish_engulfing_count`, `is_hammer`, `is_shooting_star`, `is_marubozu`,
`inside_bar_count`, `outside_bar_count`, `candle_symmetry`, `candle_color_changes`

**Range & Structure**: `atr_series`, `true_range_series`, `high_low_range`,
`price_contraction`, `range_expansion_ratio`, `close_distance_from_high`,
`pct_from_low`, `is_breakout_up`, `reversal_count`, `open_gap_fill_rate`,
`pivot_highs`, `pivot_lows`

**Streaks**: `consecutive_higher_closes`, `consecutive_higher_highs`,
`consecutive_lower_lows`, `longest_winning_streak`, `longest_losing_streak`,
`longest_flat_streak`, `bars_since_new_high`, `bars_since_new_low`

---

## SignalValue Combinators (70+)

`SignalValue` carries a scalar or `Unavailable` and propagates unavailability
through every operation:

```rust
sv.abs() / sv.negate() / sv.signum()
sv.clamp(lo, hi)                   // clamp to [lo, hi]
sv.cap_at(max) / sv.floor_at(min)  // one-sided clamps
sv.lerp(other, t)                  // linear interpolation, t ∈ [0, 1]
sv.blend(other, weight)            // weighted blend
sv.quantize(step)                  // round to nearest multiple of step
sv.distance_to(other)              // absolute difference
sv.delta(prev)                     // signed change
sv.cross_above(prev, threshold)    // true on upward threshold cross
sv.within_range(lo, hi)            // boolean range test
sv.as_percent() / sv.pct_of(base)  // percentage helpers
sv.sign_match(other)               // true if same sign
sv.map(f) / sv.zip_with(other, f)  // functor / applicative style
```

---

## SignalMap Analytics (90+)

`SignalMap` is the output of `SignalPipeline::update`. Fleet-wide analytics:

```rust
map.average_scalar()          // mean of all scalar values
map.std_dev() / .variance()   // dispersion
map.z_scores()                // HashMap<String, f64> z-score per signal
map.entropy()                 // Shannon entropy of the distribution
map.gini_coefficient()        // Gini inequality coefficient
map.normalize_all()           // min-max normalize all scalars to [0, 1]
map.top_n(3) / .bottom_n(3)   // top/bottom signals by value
map.weighted_sum(&weights)    // dot product with weight map
map.scale_all(factor)         // multiply all scalars by factor
map.percentile_rank_of(name)  // percentile of one signal among all
map.signal_ratio(a, b)        // ratio of two named signals
map.count_positive() / .count_negative() / .count_zero()
map.all_positive() / .all_negative()
```

---

## Signal Warmup Contracts

Every indicator has an implicit warmup period. `signals::warmup` makes that
period queryable, enforceable, and reportable:

| Type | Purpose |
|------|---------|
| `WarmupContract` | Trait: `warmup_period()`, `is_ready()`, `bars_remaining()` — implemented by all `Signal` types |
| `WarmupGuard<S>` | Wraps any signal; returns `Err(NotReady)` until warmup completes instead of silent `Unavailable` |
| `WarmupReporter` | Tracks warmup progress for N signals and produces `WarmupReport` snapshots |
| `WarmupReport` | `all_ready()`, `pipeline_bars_remaining()`, `warming_signals()`, `display()` |

```rust
use fin_primitives::signals::indicators::Sma;
use fin_primitives::signals::{BarInput, Signal};
use fin_primitives::signals::warmup::{WarmupContract, WarmupGuard, WarmupReporter};
use rust_decimal_macros::dec;

// Guard: explicit error instead of silent Unavailable
let sma = Sma::new("sma5", 5).unwrap();
let mut guard = WarmupGuard::new(sma);

for _ in 0..4 {
    let bar = BarInput::from_close(dec!(100));
    // Err(NotReady { bars_remaining: 4, 3, 2, 1 })
    assert!(guard.update_checked(&bar).is_err());
}
// 5th bar — Ok(SignalValue::Scalar(...))
assert!(guard.update_checked(&BarInput::from_close(dec!(100))).is_ok());

// Reporter: pipeline-level warmup snapshot
let mut reporter = WarmupReporter::new(
    vec![5, 14, 20],
    vec!["sma5".into(), "rsi14".into(), "bb20".into()],
);
reporter.tick_n(5); // 5 bars consumed
let report = reporter.report(reporter.bars_consumed());
assert!(report.statuses[0].is_ready);   // sma5 done
assert!(!report.statuses[1].is_ready);  // rsi14 still warming
println!("{}", report.display());
// WarmupReport [bars_consumed=5, ready=1/3, pipeline_remaining=15]
//   [READY]   sma5  (period=5)
//   [WARMING] rsi14 (period=14, remaining=9)
//   [WARMING] bb20  (period=20, remaining=15)
```

---

## Signal Composition Engine

`signals::compose` provides a composable expression-tree DSL for building
derived signals from existing indicators without writing bespoke structs.

### Expression Nodes

| Node | Description |
|------|-------------|
| `Raw(name)` | Leaf: raw output of a named indicator |
| `Add(a, b)` | Element-wise sum; `Unavailable` if either is |
| `Sub(a, b)` | `a - b`; `Unavailable` if either is |
| `Mul(expr, f)` | Scale by constant `f` |
| `Lag(expr, n)` | Delay by `n` bars; `Unavailable` until buffer fills |
| `Normalize(expr, method, window)` | `MinMax`, `ZScore`, or `Percentile` normalisation |
| `Threshold(expr, level, dir)` | `Above` → `+1/0`, `Below` → `-1/0`, `Cross` → `+1/-1/0` |

### Fluent `SignalBuilder` API

```rust
use fin_primitives::signals::indicators::Rsi;
use fin_primitives::signals::{BarInput, Signal};
use fin_primitives::signals::compose::{SignalBuilder, NormMethod, Direction};
use rust_decimal_macros::dec;

// RSI(14) → lag(1) → ZScore(20) → threshold(+2σ, Above)
let rsi = Rsi::new("rsi14", 14).unwrap();
let mut momentum_signal = SignalBuilder::new(rsi)
    .lag(1)
    .normalize_window(NormMethod::ZScore, 20)
    .threshold(dec!(2), Direction::Above)
    .build_named("rsi_zscore_cross");

// Feed bars; signal returns Scalar(1) when RSI z-score crosses above +2σ
let bar = BarInput::from_close(dec!(100));
let _ = momentum_signal.update(&bar); // Unavailable during warmup

// Combine two signals manually
use fin_primitives::signals::compose::{SignalExpr, ComposedSignal};
use fin_primitives::signals::indicators::Sma;

let sma_fast = Sma::new("sma5", 5).unwrap();
let sma_slow = Sma::new("sma20", 20).unwrap();
let expr = SignalExpr::raw("sma5")
    .sub(SignalExpr::raw("sma20"))      // MACD-style crossover
    .threshold(dec!(0), Direction::Cross);
let leaves: Vec<Box<dyn Signal>> = vec![Box::new(sma_fast), Box::new(sma_slow)];
let mut cross = ComposedSignal::new("fast_slow_cross", expr, leaves).unwrap();
```

### Normalisation Methods

| Method | Formula | Output |
|--------|---------|--------|
| `MinMax` | `(v - min) / (max - min)` | `[0, 1]` |
| `ZScore` | `(v - mean) / std_dev` | Unbounded; typically `[-3, +3]` |
| `Percentile` | Rank within rolling window | `[0, 1]` |

---

## Risk Attribution

`risk::attribution` decomposes portfolio risk into six named factors and supports
Brinson-Hood-Beebower P&L attribution. Use `RiskMonitor::attribution_report` or
construct `RiskAttributor` directly.

### Six-Factor Risk Decomposition

| Factor | Driver | Estimation |
|--------|--------|------------|
| `Market` | Systematic beta exposure | `β² × σ²_market` |
| `Sector` | Industry concentration | HHI × `σ²_market × 0.5` |
| `Idiosyncratic` | Stock-specific residual | `Σ w_i² × σ²_idio_i` |
| `Leverage` | Borrowed capital amplification | `(L−1)² × σ²_market` |
| `Concentration` | Large single-position weight | HHI × `σ²_market × 0.3` |
| `Liquidity` | Illiquid exit risk | `(1 − avg_liquidity) × σ²_market` |

```rust
use fin_primitives::risk::RiskMonitor;
use fin_primitives::risk::attribution::{MarketData, RiskAttributor};
use fin_primitives::position::PositionLedger;
use rust_decimal_macros::dec;

let ledger = PositionLedger::new(dec!(100_000));
// Populate ledger with positions via ledger.apply_fill(...)

let market_data = MarketData::new(0.15)   // 15% annualised market vol
    .with_beta("AAPL", 1.2)
    .with_beta("MSFT", 0.9)
    .with_sector("AAPL", "Technology")
    .with_sector("MSFT", "Technology")
    .with_liquidity("AAPL", 0.98)
    .with_idio_vol("AAPL", 0.25);

// Via RiskMonitor (one-liner)
let monitor = RiskMonitor::new(dec!(100_000));
let report = monitor.attribution_report(&ledger, market_data.clone());

println!("{}", report.summary());
// AttributionReport [equity=..., total_risk=..., beta=1.05, hhi=0.5, leverage=1.0x]
//   Market (Beta)        42.3%  (...)
//   Sector               18.1%  (...)
//   Idiosyncratic        27.4%  (...)
//   ...

// Or directly via RiskAttributor
let attributor = RiskAttributor::new(&ledger, market_data);
let report = attributor.compute();
let dominant = report.dominant_factor().unwrap();
println!("Largest risk factor: {}", dominant.factor.name());
```

### Brinson-Hood-Beebower P&L Attribution

```rust
use fin_primitives::risk::attribution::{RiskAttributor, BhbInput, BhbSectorInput, MarketData};
use fin_primitives::position::PositionLedger;
use rust_decimal_macros::dec;

let ledger = PositionLedger::new(dec!(100_000));
let attributor = RiskAttributor::new(&ledger, MarketData::default());

let bhb = attributor.compute_bhb(&BhbInput {
    benchmark_total_return: 0.05,
    sectors: vec![
        BhbSectorInput {
            sector: "Technology".into(),
            portfolio_weight:   0.60,  // overweight vs benchmark
            benchmark_weight:   0.40,
            portfolio_sector_return: 0.08,
            benchmark_sector_return: 0.06,
        },
        BhbSectorInput {
            sector: "Energy".into(),
            portfolio_weight:   0.40,
            benchmark_weight:   0.60,
            portfolio_sector_return: 0.02,
            benchmark_sector_return: 0.04,
        },
    ],
});

println!("Active return: {:.2}%", bhb.total_active_return * 100.0);
println!("  Allocation:   {:.4}", bhb.total_allocation);
println!("  Selection:    {:.4}", bhb.total_selection);
println!("  Interaction:  {:.4}", bhb.total_interaction);
```

---

## PositionLedger Analytics (145+)

```rust
ledger.equity(&prices)                      // cash + unrealized P&L
ledger.total_unrealized_pnl(&prices)        // sum of all open position P&L
ledger.concentration_ratio()               // Herfindahl-Hirschman Index
ledger.long_exposure() / .short_exposure()  // directional gross exposure
ledger.avg_long_entry_price()               // VWAP of long entries
ledger.avg_short_entry_price()              // VWAP of short entries
ledger.pct_long() / .pct_short()            // directional balance
ledger.win_rate()                           // % of closed positions with positive P&L
ledger.largest_position() / .smallest_position()
ledger.symbols_with_unrealized_loss(&prices)
ledger.risk_reward_ratio()
ledger.kelly_fraction()
```

---

## DrawdownTracker Analytics (120+)

```rust
tracker.current_drawdown_pct()      // (peak − equity) / peak × 100
tracker.max_drawdown_pct()          // worst drawdown seen
tracker.calmar_ratio()              // annualized return / max drawdown
tracker.sharpe_ratio()              // using per-update equity changes
tracker.sortino_ratio()             // downside-deviation adjusted
tracker.win_rate()                  // fraction of updates that gained equity
tracker.avg_gain_pct()              // average gain per gaining update
tracker.avg_loss_pct()              // average loss per losing update
tracker.equity_change_std()         // std dev of per-update equity changes
tracker.gain_loss_asymmetry()       // ratio of avg gain magnitude to avg loss magnitude
tracker.recovery_factor()           // net return / max drawdown
tracker.omega_ratio()               // probability-weighted gain/loss ratio
tracker.equity_multiple()           // current / initial equity
tracker.return_drawdown_ratio()     // net return % / worst drawdown %
tracker.streak_win_rate()           // max_gain_streak / total streak length
tracker.time_to_recover_est()       // estimated updates to recover from current drawdown
```

---

## Options Greeks & Black-Scholes

The `greeks` module provides a zero-panic European option pricing engine.

```rust
use fin_primitives::greeks::{BlackScholes, OptionSpec, OptionType, SpreadGreeks};
use rust_decimal_macros::dec;

fn main() -> Result<(), fin_primitives::FinError> {
    let spec = OptionSpec {
        strike:          dec!(100),
        expiry_days:     30,
        spot:            dec!(100),
        risk_free_rate:  dec!(0.05),
        volatility:      dec!(0.20),
        option_type:     OptionType::Call,
    };

    // Theoretical price
    let price = BlackScholes::price(&spec)?;

    // All five Greeks
    let g = BlackScholes::greeks(&spec)?;
    println!("delta={} gamma={} theta={} vega={} rho={}", g.delta, g.gamma, g.theta, g.vega, g.rho);

    // Implied volatility from a market quote
    let iv = BlackScholes::implied_vol(price, &spec)?;

    // Multi-leg spreads
    let straddle = SpreadGreeks::straddle(dec!(100), dec!(100), 30, dec!(0.05), dec!(0.20));
    let net = straddle.net_greeks()?;
    println!("straddle net delta ≈ {}", net.delta);
    Ok(())
}
```

**Spread constructors:**

| Constructor | Description |
|---|---|
| `SpreadGreeks::bull_call_spread(…)` | Long low-strike call, short high-strike call |
| `SpreadGreeks::bear_put_spread(…)` | Long high-strike put, short low-strike put |
| `SpreadGreeks::straddle(…)` | Long ATM call + long ATM put |
| `SpreadGreeks::iron_condor(…)` | Short put spread + short call spread |
| `SpreadGreeks::new(legs)` | Arbitrary legs with signed quantities |

**Formulas:**

| Greek | Formula |
|---|---|
| delta | ∂V/∂S (`N(d₁)` call, `N(d₁)−1` put) |
| gamma | φ(d₁) / (S σ √T) |
| theta | −(S φ(d₁) σ) / (2√T) ± r K e^{−rT} N(±d₂), per day |
| vega  | S φ(d₁) √T / 100 (per 1 vol-point) |
| rho   | ±K T e^{−rT} N(±d₂) / 100 (per 1 rate-point) |

Implied vol is solved by bisection over `[1e-6, 5.0]` (up to 200 iterations, tolerance 1e-7).

---

## Regime Detection Engine

The `regime` module classifies the current market state using four complementary
quantitative signals, then adapts strategy parameters per regime.

### Regimes

| Regime | Primary Signal | Condition |
|--------|---------------|-----------|
| `Trending` | Hurst exponent | H > 0.6 (persistent process) |
| `MeanReverting` | Hurst exponent | H < 0.4 (anti-persistent) |
| `HighVolatility` | Realized vol / long-run mean | ratio > 2.0x; GARCH confirms |
| `LowVolatility` | Realized vol / long-run mean | ratio < 0.5x; BB width compressed |
| `Crisis` | Cross-asset correlation | > 60% of pairs decorrelated simultaneously |
| `Neutral` | — | No dominant signal |
| `Unknown` | — | Warm-up phase incomplete |

Priority order: `Crisis > HighVolatility > Trending > MeanReverting > LowVolatility > Neutral`

### GARCH(1,1) Persistent Volatility

The `Garch11` struct fits an online GARCH(1,1) model — ω + α·ε²ₜ₋₁ + β·σ²ₜ₋₁ — and flags when conditional vol exceeds the long-run level by a configurable multiplier. This catches regimes where volatility is structurally elevated (crisis, rate shock) rather than just transiently spiked.

```rust
use fin_primitives::regime::{Garch11, RegimeDetector, RegimeConfig, MarketRegime, RegimeConditionalSignal};
use fin_primitives::signals::BarInput;
use rust_decimal_macros::dec;

// ── Standalone GARCH ──────────────────────────────────────────────────────────
let mut garch = Garch11::new(0.1, 0.85, 1e-6).unwrap();
let log_returns = [-0.01_f64, 0.02, -0.03, 0.015, -0.025];
for ret in log_returns {
    let sigma = garch.update(ret);
    println!("σ = {sigma:.6}  elevated = {}", garch.is_vol_elevated(1.5));
}
println!("long-run σ = {:.6}", garch.long_run_sigma());

// ── Full regime detector ──────────────────────────────────────────────────────
let mut detector = RegimeDetector::new(14, RegimeConfig::default()).unwrap();

let bars = vec![
    BarInput::new(dec!(100), dec!(102), dec!(98), dec!(100), dec!(5_000)),
    // ... more bars
];

for bar in &bars {
    let (regime, confidence) = detector.update(bar, &[]).unwrap();
    // cross_returns: &[(asset_idx, log_return)] for multi-asset crisis detection
    // e.g. detector.update(bar, &[(1, -0.02), (2, 0.01)]).unwrap()

    println!("[{}] regime = {regime}  confidence = {confidence:.2}  risk_off = {}",
        bar.close, regime.short_code(), regime.is_risk_off());
}

// Inspect regime history
for epoch in detector.history() {
    println!("  {:?}  started_at={} confidence={:.2} duration={:?}",
        epoch.regime, epoch.started_at_bar, epoch.confidence, epoch.duration_bars());
}
```

### Regime-Conditional Signal Adaptation

`RegimeConditionalSignal` applies RSI with a short period in trending markets,
a longer period when mean-reverting, and suppresses the signal entirely in crisis:

```rust
use fin_primitives::regime::{RegimeConditionalSignal, MarketRegime};
use fin_primitives::signals::BarInput;
use rust_decimal_macros::dec;

let mut signal = RegimeConditionalSignal::new(
    14,  // RSI period in Trending regime
    21,  // RSI period in MeanReverting regime
    14,  // RSI period in all other non-risk-off regimes
).unwrap();

let bar = BarInput::new(dec!(100), dec!(102), dec!(98), dec!(100), dec!(1000));

match signal.update(&bar, MarketRegime::Trending) {
    Some(Ok(rsi)) => println!("RSI(14) in trending = {rsi:.2}"),
    Some(Err(e))  => eprintln!("error: {e}"),
    None          => println!("signal suppressed (warm-up or risk-off)"),
}
// Crisis/Unknown → None (flat signal, no trading)
assert!(signal.update(&bar, MarketRegime::Crisis).is_none());
```

### RegimeConfig Thresholds

```rust
RegimeConfig {
    hurst_trending:              0.6,   // H > 0.6 → Trending
    hurst_mean_reverting:        0.4,   // H < 0.4 → MeanReverting
    vol_high_multiplier:         2.0,   // realized vol > 2x long-run mean → HighVolatility
    vol_low_multiplier:          0.5,   // realized vol < 0.5x long-run mean → LowVolatility
    adx_trend_threshold:        25.0,   // ADX above this confirms trend
    bb_width_quiet:             0.02,   // BB width below this confirms LowVolatility
    crisis_correlation_threshold: 0.3, // |r| below this = decorrelated pair
    crisis_pair_fraction:        0.6,  // 60%+ of pairs decorrelated → Crisis
    garch_alpha:                 0.1,  // GARCH innovation weight
    garch_beta:                  0.85, // GARCH persistence weight
    garch_omega:                 1e-6, // GARCH long-run floor
    garch_vol_multiplier:        1.5,  // GARCH sigma multiplier for high-vol flag
}
```

---

## Walk-Forward Optimizer (Grid Search)

The `backtest::walk_forward` module provides proper out-of-sample validation
via a rolling train/test split with parameter grid search.

### Algorithm

```text
|────── train ──────|── test ──|
       step ──►
               |────── train ──────|── test ──|
```

For each window:
1. Grid search over all parameter combinations on the **training** slice.
2. Select parameters that maximize in-sample Sharpe ratio.
3. Evaluate those parameters on the **held-out test** slice.
4. Record `WfPeriod` with both IS and OOS metrics.

Aggregate: `aggregate_sharpe = mean(OOS Sharpe)`, `stability_score = fraction of OOS windows with positive Sharpe`.

### Example

```rust
use fin_primitives::backtest::walk_forward::{WalkForwardOptimizer, WalkForwardConfig, ParamRange};
use fin_primitives::backtest::{BacktestConfig, Signal, SignalDirection, Strategy};
use fin_primitives::ohlcv::OhlcvBar;
use std::collections::HashMap;
use rust_decimal_macros::dec;

// ── Define a parametric strategy ─────────────────────────────────────────────
struct SmaStrategy { period: usize, bar_count: usize, window: std::collections::VecDeque<rust_decimal::Decimal> }

impl Strategy for SmaStrategy {
    fn on_bar(&mut self, bar: &OhlcvBar) -> Option<Signal> {
        let close = bar.close.value();
        self.window.push_back(close);
        if self.window.len() > self.period { self.window.pop_front(); }
        if self.window.len() < self.period { return None; }
        let sma: rust_decimal::Decimal = self.window.iter().sum::<rust_decimal::Decimal>()
            / rust_decimal::Decimal::from(self.period);
        let dir = if close > sma { SignalDirection::Buy } else { SignalDirection::Sell };
        Some(Signal::new(dir, dec!(1)))
    }
}

// ── Configure the optimizer ───────────────────────────────────────────────────
let config = WalkForwardConfig {
    train_window: 120,  // 120 bars for in-sample fitting
    test_window:   30,  // 30 bars for out-of-sample evaluation
    step:          30,  // advance by 30 bars each iteration
    param_space: vec![
        ParamRange { name: "sma_period".to_owned(), min: 5.0, max: 25.0, step: 5.0 },
    ],
};

let bt_config = BacktestConfig::new(dec!(100_000), dec!(0.001)).unwrap();
let optimizer = WalkForwardOptimizer::new(config, bt_config).unwrap();

let bars: Vec<OhlcvBar> = vec![/* ... historical bars */];

let result = optimizer.run(&bars, |train_bars, params| {
    let period = params.get("sma_period").copied().unwrap_or(10.0) as usize;
    Box::new(SmaStrategy { period, bar_count: 0, window: Default::default() })
}).unwrap();

// ── Interpret results ─────────────────────────────────────────────────────────
println!("Periods evaluated:    {}", result.periods.len());
println!("Aggregate OOS Sharpe: {:.2}", result.aggregate_sharpe);
println!("Stability score:      {:.1}%", result.stability_score * 100.0);
println!("Mean OOS return:      {:.2}%", result.mean_oos_return * 100.0);
println!("Worst OOS drawdown:   {:.2}%", result.worst_oos_drawdown * 100.0);

// Robustness check: Sharpe > 0 and at least 65% of windows profitable
if result.is_robust(0.65) {
    println!("Strategy PASSED walk-forward robustness check");
}

// Per-period detail
for (i, period) in result.periods.iter().enumerate() {
    println!("  [WF {}] IS Sharpe={:.2}  OOS Sharpe={:.2}  best_params={:?}",
        i, period.in_sample_sharpe, period.out_of_sample_sharpe, period.best_params);
}

// Best / worst OOS windows
if let Some(best) = result.best_period() {
    println!("Best OOS window:  bars {}–{} (Sharpe {:.2})", best.test_start, best.test_end, best.out_of_sample_sharpe);
}
```

### `WalkForwardResult` Fields

| Field | Description |
|---|---|
| `periods` | `Vec<WfPeriod>` — one entry per rolling window |
| `aggregate_sharpe` | Mean out-of-sample Sharpe across all periods |
| `stability_score` | Fraction of OOS windows with positive Sharpe; `[0, 1]` |
| `mean_oos_return` | Mean OOS total return across all periods |
| `worst_oos_drawdown` | Maximum OOS drawdown seen across any single period |

### `WfPeriod` Fields

| Field | Description |
|---|---|
| `train_start / train_end` | Bar index range of the training window |
| `test_start / test_end` | Bar index range of the test window |
| `best_params` | `HashMap<String, f64>` — winning parameter combination |
| `in_sample_sharpe` | Best Sharpe achieved on training data |
| `out_of_sample_sharpe` | Sharpe achieved on held-out test data |
| `oos_result` | Full `BacktestResult` for the OOS period |

---

## Backtester with Walk-Forward Optimization

The `backtest` module provides a bar-by-bar event-driven backtester and a
rolling walk-forward optimizer.

```rust
use fin_primitives::backtest::{
    Backtester, BacktestConfig, Signal, SignalDirection, Strategy, WalkForwardOptimizer,
};
use fin_primitives::ohlcv::OhlcvBar;
use rust_decimal_macros::dec;

// 1. Implement the Strategy trait
struct MomentumStrategy { last_close: Option<rust_decimal::Decimal> }

impl Strategy for MomentumStrategy {
    fn on_bar(&mut self, bar: &OhlcvBar) -> Option<Signal> {
        let close = bar.close.value();
        let dir = match self.last_close {
            Some(prev) if close > prev => SignalDirection::Buy,
            Some(prev) if close < prev => SignalDirection::Sell,
            _ => SignalDirection::Hold,
        };
        self.last_close = Some(close);
        Some(Signal::new(dir, dec!(1)))
    }
}

fn main() -> Result<(), fin_primitives::FinError> {
    // 2. Configure and run
    let config = BacktestConfig::new(dec!(100_000), dec!(0.001))?;
    let bars: Vec<OhlcvBar> = vec![/* … */];
    let result = Backtester::new(config.clone()).run(&bars, &mut MomentumStrategy { last_close: None })?;

    println!("total_return={:.2}%  sharpe={:.2}  max_dd={:.2}%  trades={}",
        result.total_return * dec!(100),
        result.sharpe_ratio,
        result.max_drawdown * dec!(100),
        result.trade_count);

    // 3. Walk-forward optimization
    let wfo = WalkForwardOptimizer::new(200, 50, config)?;
    let wf = wfo.run(&bars, |_train| Box::new(MomentumStrategy { last_close: None }))?;
    println!("mean OOS return={:.2}%  worst dd={:.2}%",
        wf.mean_return * dec!(100), wf.worst_drawdown * dec!(100));
    Ok(())
}
```

**`BacktestResult` fields:**

| Field | Description |
|---|---|
| `total_return` | `(final_equity − initial_capital) / initial_capital` |
| `sharpe_ratio` | Annualised Sharpe (252-day, sample stddev), 0 if flat returns |
| `max_drawdown` | Peak-to-trough fraction, always in `[0, 1]` |
| `win_rate` | Fraction of closed trades with positive realized P&L |
| `trade_count` | Total fills executed |
| `equity_curve` | `Vec<Decimal>` sampled once per bar |

---

## Async Streaming Signals

The `async_signals` module wraps any `SignalPipeline` with Tokio MPSC channels
for non-blocking, zero-copy signal streaming.

```rust
use fin_primitives::async_signals::{StreamingSignalPipeline, spawn_signal_stream};
use fin_primitives::signals::pipeline::SignalPipeline;
use fin_primitives::signals::indicators::Sma;
use fin_primitives::ohlcv::OhlcvBar;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() {
    let pipeline = SignalPipeline::new().add(Sma::new("sma20", 20));

    // Option A: high-level wrapper
    let (tick_tx, mut update_rx) = StreamingSignalPipeline::new(pipeline).spawn();

    // Option B: convenience function with your own tick channel
    // let (tick_tx, tick_rx) = mpsc::channel::<OhlcvBar>(1024);
    // let mut update_rx = spawn_signal_stream(pipeline, tick_rx);

    // Push bars from any async producer
    tokio::spawn(async move {
        // tick_tx.send(bar).await.unwrap();
        drop(tick_tx); // closing sender shuts down the pipeline task
    });

    while let Some(update) = update_rx.recv().await {
        if update.is_ready() {
            println!("{} = {}", update.signal_name, update.value);
        }
    }
}
```

**Key properties:**
- Output buffers are pre-allocated at construction time (default: 4 096 slots).
- The background task shuts down cleanly when all tick senders are dropped.
- Multiple signals in the same pipeline each emit one `SignalUpdate` per bar.
- `SignalUpdate::timestamp` carries wall-clock `DateTime<Utc>` of computation.

---

## NanoTimestamp Utilities (120+)

```rust
NanoTimestamp::now()                // current UTC nanoseconds
ts.add_days(n) / .sub_days(n)
ts.add_months(n)                    // calendar-accurate month arithmetic
ts.start_of_week() / .end_of_month()
ts.start_of_quarter()               // Jan 1 / Apr 1 / Jul 1 / Oct 1
ts.end_of_quarter()                 // last nanosecond of the quarter
ts.is_same_quarter(other)           // same calendar quarter and year
ts.floor_to_hour() / .floor_to_minute() / .floor_to_second()
ts.is_market_hours()                // 09:30–16:00 ET (approximate)
ts.is_weekend()
ts.quarter()                        // 1–4
ts.elapsed_days() / .elapsed_hours() / .elapsed_minutes()
ts.nanoseconds_between(other)
ts.lerp(other, t)                   // interpolate two timestamps
```

---

## Mathematical Definitions

### Price and Quantity Types

| Type | Invariant | Backing type |
|------|-----------|-------------|
| `Price` | `d > 0` (strictly positive) | `rust_decimal::Decimal` |
| `Quantity` | `d >= 0` (non-negative) | `rust_decimal::Decimal` |
| `NanoTimestamp` | any `i64`; nanoseconds since Unix epoch (UTC) | `i64` |
| `Symbol` | non-empty, no whitespace | `String` |

### OHLCV Invariants

Every `OhlcvBar` that enters an `OhlcvSeries` has been validated to satisfy:

```
high >= open    high >= close
low  <= open    low  <= close
high >= low
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

- **Realized P&L** (on reduce/close): `closed_qty × (fill_price − avg_cost)` for long.
- **Unrealized P&L**: `position_qty × (current_price − avg_cost)`.
- Both are **net of commissions**.

---

## API Reference

### `types` module

```rust
Price::new(d)        -> Result<Price, FinError>       // d > 0
Quantity::new(d)     -> Result<Quantity, FinError>    // d >= 0
Quantity::zero()     -> Quantity
Symbol::new(s)       -> Result<Symbol, FinError>      // non-empty, no whitespace
NanoTimestamp::now() -> NanoTimestamp                 // current UTC nanoseconds
```

### `orderbook` module

```rust
OrderBook::new(symbol)
  .apply_delta(delta)          -> Result<(), FinError>
  .best_bid() / .best_ask()    -> Option<PriceLevel>
  .spread()                    -> Option<Decimal>       // best_ask - best_bid
  .mid_price()                 -> Option<Decimal>
  .vwap_for_qty(side, qty)     -> Result<Decimal, FinError>
  .top_bids(n) / .top_asks(n)  -> Vec<PriceLevel>
```

### `ohlcv` module

```rust
OhlcvAggregator::new(symbol, tf) -> Result<Self, FinError>
  .push_tick(&tick)            -> Result<Option<OhlcvBar>, FinError>
  .flush()                     -> Option<OhlcvBar>

OhlcvSeries::new()
  .push(bar)                   -> Result<(), FinError>
  .closes()                    -> Vec<Decimal>
  .window(n)                   -> &[OhlcvBar]
  // ...370+ analytics methods
```

### `signals` module

```rust
// Signal trait
trait Signal {
    fn name(&self)   -> &str;
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError>;
    fn is_ready(&self) -> bool;
    fn period(&self) -> usize;
    fn reset(&mut self);
}

SignalPipeline::new()
  .add(signal)           // builder pattern; chainable
  .update(&bar)          -> Result<SignalMap, FinError>

SignalMap::get(name)     -> Option<&SignalValue>
// SignalValue: Scalar(Decimal) | Unavailable

// Warmup contracts (signals::warmup)
WarmupGuard::new(signal)
  .update_checked(&bar)  -> Result<SignalValue, WarmupError>
  .is_ready()            -> bool
  .bars_remaining()      -> usize
  .bars_seen()           -> usize

WarmupReporter::new(periods, names)
  .tick() / .tick_n(n)
  .report(bars_consumed) -> WarmupReport

WarmupReport::all_ready()              -> bool
  .pipeline_bars_remaining()           -> usize
  .warming_signals()                   -> impl Iterator
  .display()                           -> String

// Signal composition (signals::compose)
SignalBuilder::new(signal)
  .lag(n)
  .normalize(NormMethod)
  .normalize_window(NormMethod, window)
  .threshold(level, Direction)
  .scale(factor)
  .build()               -> ComposedSignal  // implements Signal
  .build_named(name)     -> ComposedSignal

ComposedSignal::new(name, expr, leaves) -> Result<Self, FinError>
  // implements Signal: update / is_ready / period / reset
```

### `position` module

```rust
PositionLedger::new(initial_cash)
  .apply_fill(fill)               -> Result<(), FinError>
  .equity(&prices)                -> Result<Decimal, FinError>
  .unrealized_pnl_total(&prices)  -> Result<Decimal, FinError>
  .realized_pnl_total()           -> Decimal
  // ...145+ portfolio analytics methods
```

### `risk` module

```rust
DrawdownTracker::new(initial_equity)
  .update(equity)
  .current_drawdown_pct()   -> Decimal
  .calmar_ratio()           -> Option<Decimal>
  // ...120+ risk/statistics methods

RiskMonitor::new(initial_equity)
  .add_rule(rule)           -> Self     // builder pattern
  .update(equity)           -> Vec<RiskBreach>
  .attribution_report(&ledger, market_data) -> AttributionReport

// Risk attribution (risk::attribution)
MarketData::new(market_vol)
  .with_beta(symbol, beta)
  .with_sector(symbol, sector)
  .with_liquidity(symbol, score)
  .with_idio_vol(symbol, vol)

RiskAttributor::new(&ledger, market_data)
  .compute()                -> AttributionReport
  .compute_bhb(&input)      -> BhbAttribution

AttributionReport::get(factor)          -> Option<&RiskAttribution>
  .dominant_factor()                    -> Option<&RiskAttribution>
  .factors_above(threshold_pct)         -> Vec<&RiskAttribution>
  .summary()                            -> String
  // Fields: attributions, total_risk, portfolio_beta, concentration_hhi, leverage_ratio

BhbAttribution                          // P&L decomposition
  .total_active_return: f64
  .total_allocation: f64
  .total_selection: f64
  .total_interaction: f64
  .best_allocation_sector()             -> Option<&SectorEffect>
```

### `greeks` module

```rust
BlackScholes::price(&spec)              -> Result<Decimal, FinError>
BlackScholes::greeks(&spec)             -> Result<OptionGreeks, FinError>
BlackScholes::implied_vol(price, &spec) -> Result<Decimal, FinError>

// OptionGreeks fields: delta, gamma, theta, vega, rho (all Decimal)

SpreadGreeks::bull_call_spread(spot, low_k, high_k, days, r, vol) -> SpreadGreeks
SpreadGreeks::bear_put_spread(spot, low_k, high_k, days, r, vol)  -> SpreadGreeks
SpreadGreeks::straddle(spot, strike, days, r, vol)                  -> SpreadGreeks
SpreadGreeks::iron_condor(spot, p_lo, p_hi, c_lo, c_hi, days, r, vol) -> SpreadGreeks
SpreadGreeks::new(legs)                                             -> SpreadGreeks
  .net_greeks()                         -> Result<OptionGreeks, FinError>
  .leg_count()                          -> usize
```

### `backtest` module

```rust
BacktestConfig::new(initial_capital, commission_rate) -> Result<Self, FinError>

Backtester::new(config)
  .run(bars, &mut strategy)             -> Result<BacktestResult, FinError>

// Strategy trait
trait Strategy {
    fn on_bar(&mut self, bar: &OhlcvBar) -> Option<Signal>;
}

WalkForwardOptimizer::new(train_size, test_size, config) -> Result<Self, FinError>
  .run(bars, make_strategy_fn)          -> Result<WalkForwardResult, FinError>

// WalkForwardResult fields: windows, mean_return, mean_sharpe, worst_drawdown

// Grid-search walk-forward (backtest::walk_forward)
WalkForwardOptimizer::new(config, bt_config)  -> Result<Self, FinError>
  .run(bars, make_strategy_fn)                -> Result<WalkForwardResult, FinError>

// WalkForwardResult fields: periods, aggregate_sharpe, stability_score, mean_oos_return, worst_oos_drawdown
WalkForwardResult::is_robust(min_stability)   -> bool
  .best_period() / .worst_period()            -> Option<&WfPeriod>

// WfPeriod fields: train_start, train_end, test_start, test_end,
//                  best_params, in_sample_sharpe, out_of_sample_sharpe, oos_result
```

### `regime` module

```rust
RegimeDetector::new(period, config)     -> Result<Self, FinError>
  .update(&bar, cross_returns)          -> Result<(MarketRegime, f64), FinError>
  .current_regime()                     -> MarketRegime
  .history()                            -> &[RegimeHistory]
  .is_ready()                           -> bool
  .garch()                              -> &Garch11
  .correlation_detector()               -> &CorrelationBreakdownDetector
  .reset()

// MarketRegime variants
MarketRegime::Trending | MeanReverting | HighVolatility | LowVolatility | Crisis | Neutral | Unknown
MarketRegime::is_risk_off()             -> bool   // Crisis | Unknown
MarketRegime::short_code()              -> &str   // "TRD", "MRV", "HVL", etc.

// GARCH(1,1)
Garch11::new(alpha, beta, omega)        -> Result<Self, FinError>
  .update(log_return)                   -> f64    // returns σₜ
  .sigma() / .variance()               -> f64
  .long_run_sigma()                     -> f64
  .is_vol_elevated(multiplier)          -> bool

// Correlation breakdown
CorrelationBreakdownDetector::new(window, threshold, crisis_fraction) -> Result<Self, FinError>
  .update(asset_idx, log_return)
  .is_crisis()                          -> bool

// RegimeHistory
RegimeHistory { regime, started_at_bar, confidence, ended_at_bar }
  .duration_bars()                      -> Option<usize>
  .is_active()                          -> bool

// Regime-conditional RSI
RegimeConditionalSignal::new(trending_period, mean_reverting_period, neutral_period)
  .update(&bar, regime)                 -> Option<Result<f64, FinError>>
  .is_ready()                           -> bool
```

### `async_signals` module

```rust
StreamingSignalPipeline::new(pipeline)
  .spawn()                              -> (mpsc::Sender<OhlcvBar>, mpsc::Receiver<SignalUpdate>)

spawn_signal_stream(pipeline, tick_rx) -> mpsc::Receiver<SignalUpdate>

// SignalUpdate fields: signal_name: String, value: SignalValue, timestamp: DateTime<Utc>
```

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
         (370+ analytics)   vwap_for_qty / spread
              |
        SignalPipeline ─────── CompositeSignal
        (725+ indicators)            │
              │                SignalExpr DSL
        WarmupGuard / WarmupReporter │
              │              (compose.rs)
         SignalMap (90+ methods)
              |
     PositionLedger (145+ methods)
              |          │
        DrawdownTracker  └──── RiskAttributor ──── BhbAttribution
        (120+ methods)         (6-factor model)   (BHB P&L split)
              |
         RiskMonitor ──── attribution_report()
              |
       Vec<RiskBreach>
```

All arrows represent pure data flow. No shared mutable state crosses module
boundaries. Wrap any component in `Arc<Mutex<_>>` for multi-threaded use.

---

## Performance Notes

- **O(1) order book mutations**: `apply_delta` performs a single `BTreeMap::insert`
  or `BTreeMap::remove`. Inverted-spread check reads two keys and does not allocate.
- **O(1) streaming indicators**: `Ema` and `Rsi` maintain constant-size state
  regardless of history length. `Sma` uses a `VecDeque` capped at `period` elements.
- **Zero-copy tick replay**: `TickReplayer` sorts once at construction and returns
  shared references on each call; no per-tick heap allocation.

---

## Running Tests

```bash
cargo test
cargo test --release
cargo clippy --all-features -- -D warnings
cargo doc --no-deps --open
```

The test suite includes unit tests in every module and property-based tests using `proptest`.

---

## Market Microstructure Anomaly Detection

The `microstructure` module detects three illegal order-book manipulation
patterns in real time: spoofing, layering, and quote stuffing.  All detection
runs locally with no external calls.

```rust
use fin_primitives::microstructure::{
    MicrostructureDetector, DetectorConfig, OrderEvent, OrderAction, AlertKind,
};
use fin_primitives::types::{Price, Quantity, Side};
use rust_decimal::Decimal;

let mut detector = MicrostructureDetector::new(DetectorConfig {
    spoof_min_quantity: Decimal::from(1_000),   // flag orders > 1000 qty
    spoof_cancel_window_ns: 500_000_000,        // cancelled within 500 ms
    layer_min_levels: 3,                         // 3+ distinct price levels
    layer_window_ns: 200_000_000,               // within 200 ms
    stuff_rate_threshold: 100,                  // 100+ cancels per second
    stuff_window_ns: 1_000_000_000,
});

// Feed events from your exchange adapter.
detector.on_event(OrderEvent {
    order_id: 1,
    action: OrderAction::Add,
    price: Price::new("100.00").unwrap(),
    quantity: Quantity::new("5000").unwrap(),
    side: Side::Bid,
    timestamp_ns: 0,
});
detector.on_event(OrderEvent {
    order_id: 1,
    action: OrderAction::Cancel,
    price: Price::new("100.00").unwrap(),
    quantity: Quantity::new("5000").unwrap(),
    side: Side::Bid,
    timestamp_ns: 200_000_000, // 200 ms — within spoof window
});

for alert in detector.drain_alerts() {
    match alert.kind {
        AlertKind::Spoofing      => println!("SPOOF: {}", alert.detail),
        AlertKind::Layering      => println!("LAYER: {}", alert.detail),
        AlertKind::QuoteStuffing => println!("STUFF: {}", alert.detail),
    }
}

// Aggregate stats.
let s = detector.stats();
println!("Events: {}, Cancels: {}, Spoof: {}, Layer: {}, Stuff: {}",
    s.events_total, s.cancels_total, s.spoof_alerts, s.layer_alerts, s.stuff_alerts);
```

Detection heuristics are based on public CFTC/SEC regulatory guidance and
academic literature (Comerton-Forde & Putniņš, 2015).  All thresholds are
configurable via [`DetectorConfig`].

---

## Contributing

1. Fork the repository and create a branch from `main`.
2. All public items must have `///` doc comments with purpose, arguments, return values, and errors.
3. All fallible operations must return `Result`; no `unwrap`, `expect`, or `panic!` in non-test code.
4. Every new behavior must have at least one happy-path test and one edge-case test.
5. Run `cargo fmt`, `cargo clippy -- -D warnings`, and `cargo test` before opening a PR.

---

## License

MIT. See [LICENSE](LICENSE).

> Also used inside [tokio-prompt-orchestrator](https://github.com/Mattbusel/tokio-prompt-orchestrator),
> a production Rust orchestration layer for LLM pipelines.
