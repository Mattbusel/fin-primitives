# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [Unreleased]

## [0.1.0] - 2026-03-17

### Added

- `types` module: validated newtypes `Price`, `Quantity`, `Symbol`, `NanoTimestamp`, `Side`.
  - `Price`: strictly positive `Decimal`; rejects zero and negative values with `FinError::InvalidPrice`.
  - `Quantity`: non-negative `Decimal`; rejects negative values with `FinError::InvalidQuantity`.
  - `Symbol`: non-empty, whitespace-free string with `FinError::InvalidSymbol` on violation.
  - `NanoTimestamp`: nanosecond-resolution UTC epoch integer with `now()` and `to_datetime()`.
  - `Side`: `Bid` / `Ask` market-side enum.
- `orderbook` module: level-2 order book backed by `BTreeMap`.
  - `apply_delta` with strict sequence-number validation (`FinError::SequenceMismatch`).
  - Inverted-spread guard: deltas that would cross the book are rejected and fully rolled back.
  - `vwap_for_qty`: walks book from best to worst, returns `FinError::InsufficientLiquidity` on shortfall.
  - `best_bid`, `best_ask`, `mid_price`, `spread`, `top_bids(n)`, `top_asks(n)`, `bid_count`, `ask_count`.
- `tick` module: `Tick`, `TickFilter` (symbol/side/min-quantity predicates ANDed together), `TickReplayer` (timestamp-sorted replay with `reset` and `remaining`).
- `ohlcv` module: `OhlcvBar` with `validate()`, `typical_price()`, `range()`, `is_bullish()`; `Timeframe` (Seconds/Minutes/Hours/Days) with `to_nanos()` and `bucket_start()`; `OhlcvAggregator` with `push_tick` and `flush`; `OhlcvSeries` with `window`, `closes`, `volumes`.
- `signals` module: `Signal` trait with `update`, `is_ready`, `period`; `SignalValue` enum (`Scalar` / `Unavailable`).
  - `Sma`: rolling window simple moving average.
  - `Ema`: exponential moving average using SMA seed for first `period` bars, then Wilder multiplier `2/(period+1)`.
  - `Rsi`: Wilder-smoothed RSI; returns `Unavailable` until `period + 1` bars; always in `[0, 100]`.
  - `SignalPipeline`: applies multiple signals per bar and collects results into a `SignalMap`.
- `position` module: `Fill`, `Position` (average-cost tracking, realized P&L via average-cost method, unrealized P&L, `is_flat`), `PositionLedger` (cash-balanced multi-symbol ledger, `equity` computation).
- `risk` module: `DrawdownTracker` (peak-tracking, drawdown-percentage), `RiskBreach`, `RiskRule` trait, `MaxDrawdownRule`, `MinEquityRule`, `RiskMonitor` (builder pattern, `update` returns all breaches).
- `error` module: `FinError` enum with `thiserror`-derived `Display` and structured fields for all failure modes.
- 242 tests across unit, integration, and property suites.
- Benchmark harness via Criterion (`benches/tick_bench.rs`).

[Unreleased]: https://github.com/Mattbusel/fin-primitives/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Mattbusel/fin-primitives/releases/tag/v0.1.0
