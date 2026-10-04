# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [Unreleased]

---

## [2.15.0] - 2026-10-03

### Fixed
- **Normal distribution.** Nine copies of the Abramowitz-Stegun polynomial CDF (relative error 0.16% at x = -5 and 7% at x = -8; two copies returned exactly 0 below -8) and two probit approximations (error about 4e-4) are replaced by one `normal` module: `cdf` from `libm::erfc` (about 1e-15 relative error, tails included) and `inv_cdf` from `statrs`. A deep out-of-the-money put (S=100, K=60, 30% vol, 3 months) priced 0.00091894 before and 0.00092008 now (reference 0.000920079805, mpmath).
- `VarEngine::parametric_var` looked z up in a three-row table: 97.5% used 1.645 (true 1.960), 99.9% used 2.326 (3.090), 85% used 1.40 (1.036). It now uses the exact quantile. It also scaled the daily mean by `sqrt(h)` instead of `h` over an `h`-day horizon; fixed for VaR and expected shortfall.
- `LiquidityRiskEngine::portfolio_liquidity_var` used 1.645 for everything below 99%; now the exact quantile.
- `options::greeks::implied_vol` stopped at an absolute 1e-8 price error, which is coarse for options worth fractions of a cent. It is now a safeguarded Newton solver (Newton steps inside a shrinking bisection bracket): 6 times faster (1.22 us to 0.19 us) and exact to a 1e-12 vol step.
- `Timeframe::bucket_start` rounded timestamps before 1970 toward zero (into the wrong bucket); weekly bars started on Thursday (the epoch's weekday) and now start Monday 00:00 UTC; `Weeks(u32::MAX)` overflowed silently and is now `InvalidTimeframe`.
- `OhlcvAggregator::push_tick`: a tick older than the open bar closed that bar early and opened a bar back in time. Late ticks are now dropped and counted (`late_tick_count()`).
- `CusumPriceChange` never reset at zero crossings as documented (the check compared the new sum with itself).
- Deserializing `Price`, `Quantity` and `Symbol` skipped validation (a JSON `"-5"` became a negative `Price`). Deserialization now goes through `new`; the wire format is unchanged.
- `python` feature: did not compile (missing docs under `deny(missing_docs)`), `pyproject.toml` pointed maturin at a module folder that does not exist, and `PyPrice(0.1)` stored `0.1000000000000000055...`. All fixed; a smoke test builds the wheel and imports it.
- 252 `unwrap`/`expect` calls in library code are gone: invariant checks now return `FinError` (and `total_cmp` replaces `partial_cmp().unwrap()` where NaN input used to panic).
- The crate no longer forces `crate-type = ["cdylib"]` on every dependent (each one used to link an unused `fin_primitives.dll`/`.so`); maturin builds the extension itself.

### Added
- `normal` module: `cdf`, `pdf`, `inv_cdf`.
- Interop features, all off by default: `ta` (bars implement ta's `Open/High/Low/Close/Volume`, `BarInput::try_from(&ta::DataItem)`), `yata` (bars implement `yata::core::OHLCV`, `BarInput::try_from(&yata::core::Candle)`), `wickra` (`wickra_core::Candle` to and from bars), and `arrow` (`arrow::bars_to_record_batch` / `record_batch_to_bars` with exact `Decimal128` columns). New example: `with_ta`.
- `serde` and `async` cargo features (both on by default, so nothing changes unless you opt out). With `default-features = false` the crate builds without serde and tokio, and for `wasm32-unknown-unknown`.
- `TryFrom<Decimal>` for `Price` and `Quantity`, `From<Price>`/`From<Quantity>` for `Decimal`, `PartialEq` for `BarInput`.
- Example `value_at_risk`; property tests for the normal functions, implied vol, bucket alignment, the order book (never crossed under random deltas) and bar aggregation (ordered, volume conserved with late ticks).
- `bench/competitors`: criterion benchmarks and an agreement check against `ta`, `yata` and `black_scholes` on the same 100,000 closes. Honest result: the f64 crates are 40 to 160 times faster per indicator update; values agree to 1e-12 except `ta`'s RSI, which uses a different smoothing.

### Changed
- `Sma` keeps a running sum: 2 times faster, same values.
- `thiserror` 2, `pyo3` 0.29 (cargo-audit flagged RUSTSEC-2025-0020 and RUSTSEC-2026-0177 on 0.23), `rust_decimal` and `chrono` without default features. `rust_decimal_macros` is now only a dev-dependency.
- Clippy: the `pedantic` group is off (over 1,000 style warnings); the `unwrap_used`, `expect_used` and `panic` lints stay on, and `cargo clippy --all-targets -- -D warnings` is clean.
- CI runs clippy, docs with warnings denied, every feature combination, wasm32, MSRV 1.81, the Python wheel and the README code blocks.

---

## [2.14.3] - 2026-09-30

### Fixed
- All 5,541 unit tests pass (18 failed in 2.14.2) and CI now runs `cargo test --lib` and the integration tests. Real bugs found along the way:
  - `CorrelationClusterer::silhouette_score` returned NaN when a point had no other populated cluster; singleton points now score 0 (Rousseeuw's definition).
  - `EventStudy::significance` left the event-day abnormal return out of each event's CAR; it now uses the full-window CAR.
  - `DiscountCurve::from_par_rates` did not reprice its own input swaps when tenors had gaps (e.g. 3y then 5y): it now bootstraps each knot against the annual coupon schedule with the curve's own interpolation, so every input par swap has zero NPV.
  - `VaRCalculator::cornish_fisher_var` had the skewness term with the wrong sign, so negative skew lowered VaR instead of raising it (a fat left tail could give 0 VaR).
  - `Aroon::down()` broke ties toward the oldest low while `up()` used the newest high, so Aroon Down read 0 instead of 100 when the newest bar tied the window's oldest low. Found by the TA-Lib comparison below.
- 13 tests had wrong expectations and were corrected, not loosened: tolerance boundary (documented as inclusive), collinear factors in a regression test, ATM delta (N(0.35) = 0.637, not 0.5), a contradictory ADF assertion, sample vs population variance, Brinson-Fachler vs BHB totals (equal when weights sum to 1), Sharpe compared at two different risk-free rates, HMM emission likelihoods, Calmar with no drawdown, exact-zero volatility in floating point, a multi-timeframe test that fed a flat bar, and the duration of a single cash flow.

### Added
- `bench/talib_compare/`: a reproducible comparison with TA-Lib on 100,000 seeded bars (22 indicators): numerical agreement, definitional differences and per-bar throughput. Summary in the README under "Compared with TA-Lib".

---

## [2.14.2] - 2026-09-28

### Changed
- README cut from 85 KB to under 10 KB: install, animated "how it works" diagrams (ticks to bars to indicators, and a risk check), real example output, a 3-step first program. The module guides and API reference moved, unchanged, to `docs/REFERENCE.md`, `docs/ARCHITECTURE.md` and `docs/TESTING.md`.
- docs.rs front page shows the ticks-to-bars diagram. No code changes.
- Project site: How it works section, search title and description, canonical URL, Open Graph and Twitter cards, JSON-LD.
- Repaired garbled characters in older CHANGELOG entries.

---

## [2.14.1] - 2026-09-25

### Changed
- README and docs.rs front page lead with a one-sentence summary, a recorded demo GIF, an install table and a 3-step first program whose output is shown.
- Install line now adds `rust_decimal` too: the `dec!` macro does not compile without it.
- Package metadata: clearer description, keywords, categories and homepage.

### Fixed
- The library builds without compiler warnings (unused imports and variables removed, dead code in `CompositeSignal` Any mode dropped). No API changes.

---

## [2.13.0] - 2026-03-21

### Added
- `AvgUpReturn`: rolling mean of close-to-close returns on up bars only: typical gain magnitude per winning bar.
- `AvgDownReturn`: rolling mean of close-to-close returns on down bars only: typical loss magnitude per losing bar (negative).
- `TailAsymmetry`: EMA of `(high - close) / (close - low)`: upper shadow vs lower shadow ratio; > 1 = bearish close position, < 1 = bullish.
- `PrevHighBreakout`: rolling fraction of bars where `high > prev_high`: frequency of upside breakout attempts.

---

## [2.12.0] - 2026-03-21

### Added
- `CloseInRangePct`: rolling mean of `(close - low) / range * 100`: intrabar close bias on a 0-100 scale.
- `GapDirectionStreak`: streak of consecutive same-direction opening gaps: detects persistent overnight momentum.
- `VolumeCv`: rolling coefficient of variation of volume `std(vol) / mean(vol)`: measures volume erraticism.
- `BodyToWickRatio`: rolling mean of `body / total_wicks`: directional commitment vs rejection ratio.
- `GapVolatility`, `UpperWickStreak`, `SignedBodyRatio`, `VolumeDeltaEma`: previously registered in round-108 hook commit; tests all passing.
- `CloseMinusPrevHigh`, `HighMinusPrevClose`, `HigherTimeframeTrend`, `PriceSpreadRatio`, `VolumePriceTrend`, `OnBalanceVolumeMA`, `CloseToMidrange`, `LowMinusPrevClose`, `CloseAbovePrevLow`, `CloseSessionBias`: previously untracked files now committed (703+ indicators).

---

## [2.11.0] - 2026-03-21

### Added
- `CloseLocationEma`: EMA of the Close Location Value `(2*close - high - low) / range`: smoothed buy/sell pressure position.
- `HighLowPersistence`: rolling fraction of inside bars (bar range fully within prior bar): range compression detector.
- `RangeContractionIndex`: `current_range / min_range_in_window`: how far above the recent tightest range the market currently is.
- `OpenCloseBalance`: rolling `sum(close-open) / sum(|close-open|)`: directional body balance from -1 (bearish) to +1 (bullish).
- `PriceGapMomentum`, `CloseAboveOpenStreak`, `SwingPointDetector`, `VolumeSurgeDetector`, `CandleMomentumScore`, `ConsecutiveHigherHighs`, `RangePercentile`, `TrendExhaustion`, `PriceReversalIndex`: previously untracked files now registered (673+ indicators).

---

## [2.10.0] - 2026-03-21

### Added
- `HlMidpointDeviation`: EMA of `(close - bar_midpoint) / range`: measures persistent close bias above/below the bar midpoint.
- `TrendBiasIndex`: `mean(ret) / MAD(ret)`: signal-to-noise ratio for returns; high = trending, near 0 = choppy.
- `BodyCenterPosition`: EMA of body-centre position within the bar's range; near 1 = bullish commitment, near 0 = bearish.
- `PriceChangeMad`: rolling mean absolute deviation of raw close-to-close price changes in price units.
- `GapContinuationRatio`: rolling fraction of bars where the opening gap direction aligns with the intraday session direction.
- `BodyTrendStrength`, `DirectionalCandleRatio`, `KickingPattern`, `MomentumRegime`, `PriceActionIndex`, `RelativeRangeRank`, `ReturnConcentration`, `ShadowPressure`, `VolatilityAdjustedRange`, `VolumeTrendStrength`, `RangeToAtrRatio`, `TickMoveRatio`: previously untracked files now registered.

---

## [2.8.0] - 2026-03-20

### Added
- `OpenCloseVelocity`: rolling mean of `close - open` per bar: measures average intrabar directional conviction.
- `BarRangePercentile`: interpolated percentile rank of current bar range within the window.
- `VolumeMomentumRatio`: fast average volume / slow average volume: detects participation acceleration.
- `PriceReturnSkew`: rolling skewness of close-to-close returns: identifies tail risk asymmetry.
- `BarQualityScore`, `ThreeBlackCrows`, `ThreeWhiteSoldiers`, `TweezerBottom`, `TweezerTop`: previously untracked files now registered.

---

## [2.7.0] - 2026-03-20

### Added
- `BodyAcceleration`: average change in body size per bar: detects momentum building or fading.
- `HighLowSqueeze`: current range / max range in window * 100: measures compression toward breakout.
- `VolumeConsistency`: fraction of bars where volume increased vs prior bar: detects accumulation/distribution.
- `ReturnConsistency`: fraction of individual bar returns consistent with the N-period net direction: trend quality metric.
- `HighLowRatioMa`, `IntrabarMomentum`: previously untracked files now registered.

---

## [2.6.0] - 2026-03-20

### Added
- `RangeVolatilityRatio`: current bar range / rolling average range: measures single-bar volatility expansion/compression.
- `TrendStrengthIndex`: net displacement / total path * 100: measures directional efficiency (100 = straight trend, 0 = pure chop).
- `VolumePriceRank`: percentile rank of `volume * |close - open|`: identifies high-conviction moves vs volume-without-price-action.
- `PriceVolatilityRatio`: current return / rolling std dev of prior returns: a forward-looking return surprise metric.
- `MarubozuDetector`: committed (was untracked); fixed test with wrong bar wicks.

---

## [2.5.0] - 2026-03-20

### Added
- `CalmarRatio`: rolling cumulative return / max drawdown ratio.
- `RollingBeta`: serial autocorrelation beta (lagged OLS) for detecting momentum vs mean-reversion regimes.
- `OpenCloseGap`: rolling mean of percent gap between previous close and current open: measures overnight drift bias.
- `VolumeWeightedVolatility`: standard deviation of returns weighted by each bar's volume share.
- `VolumeZScore`: rolling z-score of volume (previously untracked, now registered).
- `BodyVolumeRatio`, `GapBodyRatio`: previously untracked files now committed.
- `Evwma` (Elastic Volume-Weighted MA), `FractalDimensionIndex`, `GarmanKlassVolatility`, `ParkinsonVolatility`, `RogersSatchellVolatility`, `YangZhangVolatility`: previously untracked files now registered and committed.
- Fixed `FractalDimensionIndex` test using price `0` (invalid): changed to start from 1.

---

## [2.4.0] - 2026-03-20

### Added
- `SharpeRatio`: rolling mean return / std dev of returns over `period` bars.
- `SortinoRatio`: rolling mean return / downside deviation (negative returns only) over `period` bars.
- `MaxFavorableExcursion`: rolling max % rally from the window's trough to any subsequent close: complement to `MaxAdverseExcursion`.
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
  `lib.rs`: enforced at compile time.
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
- **Tests**: flat -> long -> flat -> short lifecycle, and direct long-to-short flip in one oversized fill
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
