# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [Unreleased]

---

## [2.5.0] - 2026-03-20

### Added
- `CalmarRatio`: rolling cumulative return / max drawdown ratio.
- `RollingBeta`: serial autocorrelation beta (lagged OLS) for detecting momentum vs mean-reversion regimes.
- `OpenCloseGap`: rolling mean of percent gap between previous close and current open — measures overnight drift bias.
- `VolumeWeightedVolatility`: standard deviation of returns weighted by each bar's volume share.
- `VolumeZScore`: rolling z-score of volume (previously untracked, now registered).
- `BodyVolumeRatio`, `GapBodyRatio`: previously untracked files now committed.
- `Evwma` (Elastic Volume-Weighted MA), `FractalDimensionIndex`, `GarmanKlassVolatility`, `ParkinsonVolatility`, `RogersSatchellVolatility`, `YangZhangVolatility`: previously untracked files now registered and committed.
- Fixed `FractalDimensionIndex` test using price `0` (invalid) — changed to start from 1.

---

## [2.4.0] - 2026-03-20

### Added
- `SharpeRatio`: rolling mean return / std dev of returns over `period` bars.
- `SortinoRatio`: rolling mean return / downside deviation (negative returns only) over `period` bars.
- `MaxFavorableExcursion`: rolling max % rally from the window's trough to any subsequent close — complement to `MaxAdverseExcursion`.
- `PercentRankRange`: percentile rank of the current bar's range (`high - low`) within the rolling `period`-bar range window.

---

## [1.1.0] - 2026-03-18

### Added
- `[profile.release]`: `opt-level = 3`, `lto = "thin"`, `codegen-units = 1`,
  `strip = "debuginfo"`, `panic = "abort"` for maximum release performance.
- `[profile.bench]`: dedicated bench profile with `lto = "thin"`.
- `[lints.clippy]`: crate-level lint configuration (`unwrap_used`, `expect_used`,
  `panic`, `todo` as `warn`; full `pedantic` group enabled; common false-positive
  pedantic lints explicitly `allow`ed).
- `rust-version = "1.75"` declared in `Cargo.toml` (MSRV).
- `include` field in `Cargo.toml` to control published crate contents.
- CI `bench` job split into its own dedicated job (compile + `--sample-size 10` run).
- README: complete rewrite with mathematical definitions for all indicators and
  types, OHLCV invariant table, precision/accuracy notes, full API reference with
  examples for every module, and custom `Signal`/`RiskRule` implementation guides.

### Changed
- Version bumped to `1.1.0`.
- CI `bench` step promoted from the `test` job to a dedicated `bench` job.
- README restructured with "What Is Included" table, mathematical definitions
  section, full API reference, and precision/accuracy section.
- Production-readiness pass: doc comments, error handling, CI, tests, and README reviewed.
  All existing tests continue to pass (341 total across unit, integration, and property suites).

---

### Added (originally [Unreleased])
- **RSI implementation**: `src/signals/indicators/rsi.rs` fully implemented with Wilder
  smoothing. Seed phase uses SMA over `period` changes; subsequent bars apply Wilder
  smoothing. Returns `Unavailable` until `period + 1` bars; value always in `[0, 100]`.
- **Safety attributes**: `#![forbid(unsafe_code)]` and `#![deny(missing_docs)]` added to
  `lib.rs` — enforced at compile time.
- **CI `bench` job**: runs `cargo bench -- --sample-size 10` on every push/PR so
  benchmark compilation is always validated.
- **Release workflow**: `.github/workflows/release.yml` triggers on `v*.*.*` tags,
  validates (fmt/clippy/test/doc), verifies Cargo.toml version matches the tag,
  publishes to crates.io, and creates a GitHub release with changelog notes.

## [1.0.0] - 2026-03-18

### Added
- `deny.toml` with license allow-list (MIT, Apache-2.0, ISC, Unicode-DFS-2016, BSD-2-Clause, BSD-3-Clause).
- CI test matrix extended to include Windows and macOS runners.
- `cargo-deny` check step added to CI.
- All public items verified to carry `///` doc comments; every `Result`-returning function documents a `# Errors` section.

### Changed
- Version bumped to `1.0.0` (stable public API).
- CI test job expanded to a matrix of `ubuntu-latest`, `windows-latest`, and `macos-latest`.

## [0.2.0] - 2026-03-17

### Added
- **Tests**: partial fill sequence tests for `vwap_for_qty` (single-level fill, multi-level sweep, exact exhaustion, insufficient liquidity error)
- **Tests**: order cancellation (cancel best bid, cancel non-best level, cancel all levels, re-book after cancel)
- **Tests**: book reconstruction from a sequential delta stream (snapshot followed by incremental updates)
- **Tests**: RSI overbought (>= 70) and oversold (<= 30) boundary assertions
- **Tests**: SMA, EMA, RSI with single data point (period 1) and all-same price values
- **Tests**: average-cost basis across two and three buys at different prices
- **Tests**: short position unrealized PnL (profit when price falls, loss when price rises)
- **Tests**: flat → long → flat → short lifecycle, and direct long-to-short flip in one oversized fill
- **Tests**: SMA/EMA convergence rate (both converge to a stable price after 20 identical bars)
- **Tests**: `MaxDrawdownRule` and `MinEquityRule` exact-boundary assertions (at threshold = no breach; one unit over = breach)
- **Tests**: two-rule scenario where only one fires (equity between the two thresholds)
- **Tests**: three-rule scenario where all fire simultaneously
- **Property tests**: price arithmetic closure (sum of two positive prices is always positive)
- **Property tests**: OHLCV ordering invariant (H >= max(O,C) >= min(O,C) >= L for any valid bar)
- **Property tests**: position quantity non-negative after an arbitrary sequence of buy-only fills
- **CI**: `cargo test --release` step to verify numeric correctness at optimisation level
- **CI**: `PROPTEST_CASES=1000` environment variable for increased property test coverage
- **CI**: `cargo audit` security vulnerability scan
- **CI**: separate jobs for `rustfmt`, `clippy`, `test`, `doc`, and `coverage` (cargo-tarpaulin + Codecov upload)
- **Cargo.toml**: `description`, `authors`, `repository`, `documentation`, `readme`, `license`, `keywords`, `categories` metadata fields

### Changed
- Version bumped to `0.2.0`
- README expanded with code examples for price arithmetic, order book operations, and position tracking; performance characteristics; indicator formula notes; integration guide

---

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

[Unreleased]: https://github.com/Mattbusel/fin-primitives/compare/v1.1.0...HEAD
[1.1.0]: https://github.com/Mattbusel/fin-primitives/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/Mattbusel/fin-primitives/compare/v0.2.0...v1.0.0
[0.2.0]: https://github.com/Mattbusel/fin-primitives/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/Mattbusel/fin-primitives/releases/tag/v0.1.0
