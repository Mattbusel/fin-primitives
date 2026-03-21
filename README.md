# fin-primitives

[![CI](https://github.com/Mattbusel/fin-primitives/actions/workflows/ci.yml/badge.svg)](https://github.com/Mattbusel/fin-primitives/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/fin-primitives.svg)](https://crates.io/crates/fin-primitives)
[![docs.rs](https://docs.rs/fin-primitives/badge.svg)](https://docs.rs/fin-primitives)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Minimum Rust Version](https://img.shields.io/badge/rust-1.81%2B-orange.svg)](https://www.rust-lang.org)

A zero-panic, decimal-precise foundation for high-frequency trading and quantitative
systems in Rust. `fin-primitives` provides the building blocks: validated types,
order book, OHLCV aggregation, **597+ streaming technical indicators**, position ledger,
and composable risk monitoring — so that upstream crates and applications can focus on
strategy rather than infrastructure.

---

## What Is Included

| Module | What it provides | Key guarantee |
|--------|-----------------|---------------|
| [`types`] | `Price`, `Quantity`, `Symbol`, `NanoTimestamp`, `Side` newtypes | Validation at construction; no invalid value can exist at runtime |
| [`tick`] | `Tick`, `TickFilter`, `TickReplayer` | Filter is pure; replayer always yields ticks in ascending timestamp order |
| [`orderbook`] | L2 `OrderBook` with `apply_delta`, spread, mid-price, VWAP, top-N levels | Sequence validation; inverted spreads are detected and rolled back |
| [`ohlcv`] | `OhlcvBar`, `Timeframe`, `OhlcvAggregator`, `OhlcvSeries` (370+ analytics) | Bar invariants (`high >= low`, etc.) enforced on every push |
| [`signals`] | `Signal` trait, `SignalPipeline`, **597+ built-in indicators**, `SignalMap` (90+ methods) | Returns `Unavailable` until warm-up period is satisfied; no silent NaN |
| [`position`] | `Position`, `Fill`, `PositionLedger` (145+ methods) | VWAP average cost; realized and unrealized P&L net of commissions |
| [`risk`] | `DrawdownTracker` (120+ methods), `RiskRule` trait, `RiskMonitor` | All breaches returned as a typed `Vec<RiskBreach>`; never silently swallowed |

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
fin-primitives = "2.6"
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

## Technical Indicators (597+)

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
    fn update(&mut self, bar: &OhlcvBar) -> Result<SignalValue, FinError>;
    fn is_ready(&self) -> bool;
    fn period(&self) -> usize;
}

SignalPipeline::new()
  .add(signal)           // builder pattern; chainable
  .update(&bar)          -> Result<SignalMap, FinError>

SignalMap::get(name)     -> Option<&SignalValue>
// SignalValue: Scalar(Decimal) | Unavailable
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
        SignalPipeline
        (540+ indicators)
              |
         SignalMap (90+ methods)
              |
     PositionLedger (145+ methods)
              |
        DrawdownTracker (120+ methods)
              |
         RiskMonitor
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
