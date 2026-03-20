//! # Module: ohlcv
//!
//! ## Responsibility
//! Provides OHLCV bar data structures, timeframe definitions, tick-to-bar aggregation,
//! and an ordered bar series with window queries.
//!
//! ## Guarantees
//! - `OhlcvBar::validate()` enforces: `high >= open`, `high >= close`, `low <= open`,
//!   `low <= close`, `high >= low`
//! - `OhlcvAggregator::push_tick` returns all completed bars including gap-fill bars
//!   when ticks skip multiple timeframe buckets
//! - `OhlcvSeries::push` maintains insertion order
//! - `OhlcvSeries` implements `IntoIterator` for `&OhlcvSeries`
//!
//! ## NOT Responsible For
//! - Persistence
//! - Cross-symbol aggregation

use crate::error::FinError;
use crate::tick::Tick;
use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
use rust_decimal::Decimal;

/// A completed OHLCV bar for a single symbol and timeframe bucket.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OhlcvBar {
    /// The instrument.
    pub symbol: Symbol,
    /// Opening price of the bar.
    pub open: Price,
    /// Highest price during the bar.
    pub high: Price,
    /// Lowest price during the bar.
    pub low: Price,
    /// Closing price of the bar.
    pub close: Price,
    /// Total traded volume during the bar.
    pub volume: Quantity,
    /// Timestamp of the first tick in the bar.
    pub ts_open: NanoTimestamp,
    /// Timestamp of the last tick in the bar.
    pub ts_close: NanoTimestamp,
    /// Number of ticks that contributed to this bar.
    pub tick_count: u64,
}

impl OhlcvBar {
    /// Constructs and validates an `OhlcvBar` from individual components.
    ///
    /// Equivalent to building the struct literal then calling `validate()`,
    /// but more convenient for test and user code that does not want to
    /// spell out all nine named fields.
    ///
    /// # Errors
    /// Returns [`FinError::BarInvariant`] if the OHLCV invariants are violated.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        symbol: Symbol,
        open: Price,
        high: Price,
        low: Price,
        close: Price,
        volume: Quantity,
        ts_open: NanoTimestamp,
        ts_close: NanoTimestamp,
        tick_count: u64,
    ) -> Result<Self, FinError> {
        let bar = Self {
            symbol,
            open,
            high,
            low,
            close,
            volume,
            ts_open,
            ts_close,
            tick_count,
        };
        bar.validate()?;
        Ok(bar)
    }

    /// Validates OHLCV invariants.
    ///
    /// # Errors
    /// Returns [`FinError::BarInvariant`] if any of these fail:
    /// - `high >= open`
    /// - `high >= close`
    /// - `low <= open`
    /// - `low <= close`
    /// - `high >= low`
    pub fn validate(&self) -> Result<(), FinError> {
        let h = self.high.value();
        let l = self.low.value();
        let o = self.open.value();
        let c = self.close.value();
        if h < o {
            return Err(FinError::BarInvariant(format!("high {h} < open {o}")));
        }
        if h < c {
            return Err(FinError::BarInvariant(format!("high {h} < close {c}")));
        }
        if l > o {
            return Err(FinError::BarInvariant(format!("low {l} > open {o}")));
        }
        if l > c {
            return Err(FinError::BarInvariant(format!("low {l} > close {c}")));
        }
        if h < l {
            return Err(FinError::BarInvariant(format!("high {h} < low {l}")));
        }
        Ok(())
    }

    /// Converts this bar to a [`crate::signals::BarInput`] for signal computation.
    pub fn to_bar_input(&self) -> crate::signals::BarInput {
        crate::signals::BarInput::from(self)
    }

    /// Returns the typical price: `(high + low + close) / 3`.
    pub fn typical_price(&self) -> Decimal {
        (self.high.value() + self.low.value() + self.close.value()) / Decimal::from(3u32)
    }

    /// Returns the price range: `high - low`.
    pub fn range(&self) -> Decimal {
        self.high.value() - self.low.value()
    }

    /// Returns the HLCC/4 price: `(high + low + close + close) / 4`.
    ///
    /// Weights the close price twice, giving it more significance than the
    /// typical price. Commonly used as a weighted price reference.
    pub fn hlcc4(&self) -> Decimal {
        (self.high.value() + self.low.value() + self.close.value() + self.close.value())
            / Decimal::from(4u32)
    }

    /// Returns the OHLC/4 price: `(open + high + low + close) / 4`.
    ///
    /// Equal weight for all four price components. Common in smoothed candlestick
    /// calculations and some custom charting systems.
    pub fn ohlc4(&self) -> Decimal {
        (self.open.value() + self.high.value() + self.low.value() + self.close.value())
            / Decimal::from(4u32)
    }

    /// Returns `true` if this bar is a gap-fill placeholder (zero ticks).
    ///
    /// Gap-fill bars are emitted by `OhlcvAggregator` when a tick arrives several
    /// buckets ahead of the current one. They have `tick_count == 0` and zero volume.
    pub fn is_gap_fill(&self) -> bool {
        self.tick_count == 0
    }

    /// Returns `true` if this bar is an inside bar relative to `prev`.
    ///
    /// An inside bar is fully contained within the previous bar's range:
    /// `self.high < prev.high && self.low > prev.low`. Commonly used in price
    /// action analysis to identify consolidation before a potential breakout.
    pub fn is_inside_bar(&self, prev: &OhlcvBar) -> bool {
        self.high.value() < prev.high.value() && self.low.value() > prev.low.value()
    }

    /// Returns `true` if this bar engulfs the previous bar (bullish or bearish engulfing).
    ///
    /// A bullish engulfing bar: `prev` is bearish and `self` is a bullish bar whose
    /// body completely contains `prev`'s body. Bearish is the mirror image.
    pub fn is_engulfing(&self, prev: &OhlcvBar) -> bool {
        let s_o = self.open.value();
        let s_c = self.close.value();
        let p_o = prev.open.value();
        let p_c = prev.close.value();
        let bullish = p_c < p_o && s_c > s_o && s_c >= p_o && s_o <= p_c;
        let bearish = p_c > p_o && s_c < s_o && s_c <= p_o && s_o >= p_c;
        bullish || bearish
    }

    /// Returns `true` if `close >= open`.
    pub fn is_bullish(&self) -> bool {
        self.close.value() >= self.open.value()
    }

    /// Returns `true` if `close < open`.
    pub fn is_bearish(&self) -> bool {
        self.close.value() < self.open.value()
    }

    /// Returns `true` if the bar has a hammer candlestick shape.
    ///
    /// Criteria: lower shadow ≥ 2 × body size, upper shadow ≤ body size, non-zero body.
    pub fn is_hammer(&self) -> bool {
        let body = self.body_size();
        if body.is_zero() {
            return false;
        }
        self.lower_shadow() >= body * Decimal::TWO && self.upper_shadow() <= body
    }

    /// Returns `true` if the bar has a shooting star candlestick shape.
    ///
    /// Criteria: upper shadow ≥ 2 × body size, lower shadow ≤ body size, non-zero body.
    pub fn is_shooting_star(&self) -> bool {
        let body = self.body_size();
        if body.is_zero() {
            return false;
        }
        self.upper_shadow() >= body * Decimal::TWO && self.lower_shadow() <= body
    }

    /// Returns the open-to-close return as a percentage: `(close - open) / open * 100`.
    ///
    /// Returns `None` when `open` is zero.
    pub fn bar_return(&self) -> Option<Decimal> {
        let o = self.open.value();
        if o.is_zero() {
            return None;
        }
        Some((self.close.value() - o) / o * Decimal::ONE_HUNDRED)
    }

    /// Returns the midpoint price: `(high + low) / 2` (HL2).
    pub fn midpoint(&self) -> Decimal {
        (self.high.value() + self.low.value()) / Decimal::TWO
    }

    /// Returns the absolute candlestick body size: `|close - open|`.
    pub fn body_size(&self) -> Decimal {
        (self.close.value() - self.open.value()).abs()
    }

    /// Returns `true` if the bar is a doji: `body_size / range < threshold`.
    ///
    /// A doji indicates indecision. Returns `false` when `range == 0` (flat bar)
    /// and `threshold == 0`; returns `true` for a flat bar with any positive threshold.
    pub fn is_doji(&self, threshold: Decimal) -> bool {
        let r = self.range();
        if r == Decimal::ZERO {
            return threshold > Decimal::ZERO;
        }
        self.body_size() / r < threshold
    }

    /// Returns the ratio of body to range: `body_size / range`.
    ///
    /// Returns `None` when `range == 0` (doji / flat bar) to avoid division by zero.
    /// Values close to `1` indicate a strong directional candle; values close to `0`
    /// indicate a spinning top or doji.
    pub fn body_ratio(&self) -> Option<Decimal> {
        let r = self.range();
        if r == Decimal::ZERO {
            return None;
        }
        Some(self.body_size() / r)
    }

    /// Returns the upper shadow length: `high - max(open, close)`.
    pub fn upper_shadow(&self) -> Decimal {
        let body_top = self.open.value().max(self.close.value());
        self.high.value() - body_top
    }

    /// Returns the lower shadow length: `min(open, close) - low`.
    pub fn lower_shadow(&self) -> Decimal {
        let body_bottom = self.open.value().min(self.close.value());
        body_bottom - self.low.value()
    }

    /// Returns the duration of this bar in nanoseconds: `ts_close - ts_open`.
    ///
    /// For gap-fill bars (no ticks), both timestamps are equal and this returns 0.
    pub fn duration_nanos(&self) -> i64 {
        self.ts_close.nanos() - self.ts_open.nanos()
    }

    /// Creates a single-tick OHLCV bar from a `Tick`.
    ///
    /// All price fields are set to the tick's price, volume to the tick's quantity,
    /// and both timestamps to the tick's timestamp.
    pub fn from_tick(tick: &Tick) -> Self {
        Self {
            symbol: tick.symbol.clone(),
            open: tick.price,
            high: tick.price,
            low: tick.price,
            close: tick.price,
            volume: tick.quantity,
            ts_open: tick.timestamp,
            ts_close: tick.timestamp,
            tick_count: 1,
        }
    }

    /// Merges `other` into `self`, producing a combined bar spanning both time ranges.
    ///
    /// - `open` comes from `self` (the earlier bar)
    /// - `close` comes from `other` (the later bar)
    /// - `high` / `low` are the extremes across both bars
    /// - `volume` and `tick_count` are summed
    /// - `ts_open` from `self`, `ts_close` from `other`
    ///
    /// # Errors
    /// Returns [`FinError::BarInvariant`] if the merged bar fails invariant checks (should not
    /// occur for well-formed inputs but is checked defensively).
    pub fn merge(&self, other: &OhlcvBar) -> Result<OhlcvBar, FinError> {
        let high = self.high.value().max(other.high.value());
        let low = self.low.value().min(other.low.value());
        let volume_sum = self.volume.value() + other.volume.value();
        let bar = OhlcvBar {
            symbol: self.symbol.clone(),
            open: self.open,
            high: Price::new(high)?,
            low: Price::new(low)?,
            close: other.close,
            volume: Quantity::new(volume_sum)?,
            ts_open: self.ts_open,
            ts_close: other.ts_close,
            tick_count: self.tick_count + other.tick_count,
        };
        bar.validate()?;
        Ok(bar)
    }

    /// Returns `true` if this bar is a bullish engulfing of `prev`.
    ///
    /// Conditions:
    /// - `prev` is bearish (`open > close`)
    /// - `self` is bullish (`close > open`)
    /// - `self.open <= prev.close` (opens at or below prev close)
    /// - `self.close >= prev.open` (closes at or above prev open)
    pub fn is_bullish_engulfing(&self, prev: &OhlcvBar) -> bool {
        let prev_bearish = prev.open.value() > prev.close.value();
        let self_bullish = self.close.value() > self.open.value();
        prev_bearish
            && self_bullish
            && self.open.value() <= prev.close.value()
            && self.close.value() >= prev.open.value()
    }

    /// Returns `true` if this bar is a bearish engulfing of `prev`.
    ///
    /// Conditions:
    /// - `prev` is bullish (`close > open`)
    /// - `self` is bearish (`open > close`)
    /// - `self.open >= prev.close` (opens at or above prev close)
    /// - `self.close <= prev.open` (closes at or below prev open)
    pub fn is_bearish_engulfing(&self, prev: &OhlcvBar) -> bool {
        let prev_bullish = prev.close.value() > prev.open.value();
        let self_bearish = self.open.value() > self.close.value();
        prev_bullish
            && self_bearish
            && self.open.value() >= prev.close.value()
            && self.close.value() <= prev.open.value()
    }
}

/// A timeframe for bar aggregation.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub enum Timeframe {
    /// Aggregation by N seconds.
    Seconds(u32),
    /// Aggregation by N minutes.
    Minutes(u32),
    /// Aggregation by N hours.
    Hours(u32),
    /// Aggregation by N days.
    Days(u32),
    /// Aggregation by N weeks (7-day periods).
    Weeks(u32),
}

impl Timeframe {
    /// Returns the timeframe duration in nanoseconds.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidTimeframe`] if the duration is zero.
    pub fn to_nanos(&self) -> Result<i64, FinError> {
        let secs: u64 = match self {
            Timeframe::Seconds(n) => u64::from(*n),
            Timeframe::Minutes(n) => u64::from(*n) * 60,
            Timeframe::Hours(n) => u64::from(*n) * 3_600,
            Timeframe::Days(n) => u64::from(*n) * 86_400,
            Timeframe::Weeks(n) => u64::from(*n) * 7 * 86_400,
        };
        if secs == 0 {
            return Err(FinError::InvalidTimeframe);
        }
        #[allow(clippy::cast_possible_wrap)]
        Ok((secs * 1_000_000_000) as i64)
    }

    /// Returns the bucket start timestamp for a given tick timestamp.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidTimeframe`] if the timeframe duration is zero.
    pub fn bucket_start(&self, ts: NanoTimestamp) -> Result<NanoTimestamp, FinError> {
        let nanos = self.to_nanos()?;
        let bucket = (ts.nanos() / nanos) * nanos;
        Ok(NanoTimestamp::new(bucket))
    }
}

/// Aggregates ticks into OHLCV bars according to a fixed timeframe.
///
/// `push_tick` returns a `Vec<OhlcvBar>` — normally empty (tick absorbed into current
/// bar) or a single element (bar completed). When a tick arrives several buckets ahead
/// of the current one, gap-fill bars are emitted for the empty intervening buckets,
/// using the last bar's close for OHLC and zero volume.
pub struct OhlcvAggregator {
    symbol: Symbol,
    timeframe: Timeframe,
    current_bar: Option<OhlcvBar>,
    current_bucket_start: Option<NanoTimestamp>,
    /// Close price of the most recently completed bar, used for gap-filling.
    last_close: Option<Price>,
    /// Count of fully completed bars emitted (via push_tick or flush).
    bars_emitted: usize,
}

impl OhlcvAggregator {
    /// Constructs a new `OhlcvAggregator`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidTimeframe`] if the timeframe is zero-duration.
    pub fn new(symbol: Symbol, timeframe: Timeframe) -> Result<Self, FinError> {
        timeframe.to_nanos()?;
        Ok(Self {
            symbol,
            timeframe,
            current_bar: None,
            current_bucket_start: None,
            last_close: None,
            bars_emitted: 0,
        })
    }

    /// Processes a single tick, returning all completed bars.
    ///
    /// # Returns
    /// - `Ok(vec![])`: tick was absorbed into the current bar (same bucket)
    /// - `Ok(vec![bar])`: one bar completed (tick starts the next bucket)
    /// - `Ok(vec![bar, gap1, gap2, ..., gap_n])`: the completed bar followed by
    ///   gap-fill bars for any empty intervening buckets
    ///
    /// Ticks for a different symbol are silently ignored and return `Ok(vec![])`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidTimeframe`] if `timeframe.bucket_start` fails.
    pub fn push_tick(&mut self, tick: &Tick) -> Result<Vec<OhlcvBar>, FinError> {
        if tick.symbol != self.symbol {
            return Ok(vec![]);
        }
        let bucket = self.timeframe.bucket_start(tick.timestamp)?;
        match self.current_bucket_start {
            None => {
                self.current_bucket_start = Some(bucket);
                self.current_bar = Some(self.new_bar(tick));
                Ok(vec![])
            }
            Some(current_bucket) if bucket == current_bucket => {
                self.update_bar(tick);
                Ok(vec![])
            }
            Some(_) => {
                let completed = self.current_bar.take().expect("current bar must be Some here");
                self.last_close = Some(completed.close);

                // Emit gap-fill bars for any buckets between the completed bar and the new one.
                let mut out = vec![completed];
                let nanos = self.timeframe.to_nanos()?;
                let prev_bucket = self.current_bucket_start.expect("set above");
                let mut gap_bucket = NanoTimestamp::new(prev_bucket.nanos() + nanos);
                while gap_bucket < bucket {
                    if let Some(close) = self.last_close {
                        out.push(OhlcvBar {
                            symbol: self.symbol.clone(),
                            open: close,
                            high: close,
                            low: close,
                            close,
                            volume: Quantity::zero(),
                            ts_open: gap_bucket,
                            ts_close: gap_bucket,
                            tick_count: 0,
                        });
                    }
                    gap_bucket = NanoTimestamp::new(gap_bucket.nanos() + nanos);
                }

                self.bars_emitted += out.len();
                self.current_bucket_start = Some(bucket);
                self.current_bar = Some(self.new_bar(tick));
                Ok(out)
            }
        }
    }

    /// Flushes the current partial bar, returning it if one exists.
    pub fn flush(&mut self) -> Option<OhlcvBar> {
        self.current_bucket_start = None;
        let bar = self.current_bar.take();
        if let Some(ref b) = bar {
            self.last_close = Some(b.close);
            self.bars_emitted += 1;
        }
        bar
    }

    /// Returns the symbol this aggregator is tracking.
    pub fn symbol(&self) -> &Symbol {
        &self.symbol
    }

    /// Returns the timeframe this aggregator is configured for.
    pub fn timeframe(&self) -> Timeframe {
        self.timeframe
    }

    /// Resets the aggregator, discarding any partial bar and last-close state.
    ///
    /// After calling `reset()` the aggregator behaves as if freshly constructed.
    /// Useful for walk-forward backtesting without recreating the aggregator.
    pub fn reset(&mut self) {
        self.current_bar = None;
        self.current_bucket_start = None;
        self.last_close = None;
        self.bars_emitted = 0;
    }

    /// Returns the number of fully completed bars emitted so far (via `push_tick` or `flush`).
    pub fn bar_count(&self) -> usize {
        self.bars_emitted
    }

    /// Returns a reference to the current (incomplete) bar, if any.
    pub fn current_bar(&self) -> Option<&OhlcvBar> {
        self.current_bar.as_ref()
    }

    fn new_bar(&self, tick: &Tick) -> OhlcvBar {
        OhlcvBar {
            symbol: self.symbol.clone(),
            open: tick.price,
            high: tick.price,
            low: tick.price,
            close: tick.price,
            volume: tick.quantity,
            ts_open: tick.timestamp,
            ts_close: tick.timestamp,
            tick_count: 1,
        }
    }

    fn update_bar(&mut self, tick: &Tick) {
        if let Some(ref mut bar) = self.current_bar {
            if tick.price > bar.high {
                bar.high = tick.price;
            }
            if tick.price < bar.low {
                bar.low = tick.price;
            }
            bar.close = tick.price;
            bar.volume =
                Quantity::new(bar.volume.value() + tick.quantity.value()).unwrap_or(bar.volume);
            bar.ts_close = tick.timestamp;
            bar.tick_count += 1;
        }
    }
}

/// An ordered collection of completed OHLCV bars.
pub struct OhlcvSeries {
    bars: Vec<OhlcvBar>,
}

impl OhlcvSeries {
    /// Creates an empty `OhlcvSeries`.
    pub fn new() -> Self {
        Self { bars: Vec::new() }
    }

    /// Creates an empty `OhlcvSeries` with a pre-allocated capacity.
    ///
    /// Avoids reallocations when the approximate number of bars is known in advance.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            bars: Vec::with_capacity(capacity),
        }
    }

    /// Appends a bar to the series after validating its invariants.
    ///
    /// # Errors
    /// Returns [`FinError::BarInvariant`] if `bar.validate()` fails.
    pub fn push(&mut self, bar: OhlcvBar) -> Result<(), FinError> {
        bar.validate()?;
        self.bars.push(bar);
        Ok(())
    }

    /// Returns the number of bars in the series.
    pub fn len(&self) -> usize {
        self.bars.len()
    }

    /// Returns `true` if there are no bars.
    pub fn is_empty(&self) -> bool {
        self.bars.is_empty()
    }

    /// Removes all bars from the series, retaining allocated capacity.
    pub fn clear(&mut self) {
        self.bars.clear();
    }

    /// Retains only the bars for which `predicate` returns `true`, removing the rest in-place.
    ///
    /// Order is preserved. Useful for filtering out gap-fill bars or bars outside a time range.
    pub fn retain(&mut self, mut predicate: impl FnMut(&OhlcvBar) -> bool) {
        self.bars.retain(|b| predicate(b));
    }

    /// Returns the bar at `index`, or `None` if out of bounds.
    pub fn get(&self, index: usize) -> Option<&OhlcvBar> {
        self.bars.get(index)
    }

    /// Returns the oldest (first inserted) bar, or `None` if empty.
    pub fn first(&self) -> Option<&OhlcvBar> {
        self.bars.first()
    }

    /// Returns the most recent bar, or `None` if empty.
    pub fn last(&self) -> Option<&OhlcvBar> {
        self.bars.last()
    }

    /// Returns the last `n` bars as a slice (fewer if series has fewer than `n`).
    pub fn window(&self, n: usize) -> &[OhlcvBar] {
        let len = self.bars.len();
        if n >= len {
            &self.bars
        } else {
            &self.bars[len - n..]
        }
    }

    /// Returns an iterator over the bars in insertion order.
    pub fn iter(&self) -> std::slice::Iter<'_, OhlcvBar> {
        self.bars.iter()
    }

    /// Returns a `Vec` of open prices in series order.
    pub fn opens(&self) -> Vec<Decimal> {
        self.bars.iter().map(|b| b.open.value()).collect()
    }

    /// Returns a `Vec` of high prices in series order.
    pub fn highs(&self) -> Vec<Decimal> {
        self.bars.iter().map(|b| b.high.value()).collect()
    }

    /// Returns a `Vec` of low prices in series order.
    pub fn lows(&self) -> Vec<Decimal> {
        self.bars.iter().map(|b| b.low.value()).collect()
    }

    /// Returns a `Vec` of close prices in series order.
    pub fn closes(&self) -> Vec<Decimal> {
        self.bars.iter().map(|b| b.close.value()).collect()
    }

    /// Returns a `Vec` of volumes in series order.
    pub fn volumes(&self) -> Vec<Decimal> {
        self.bars.iter().map(|b| b.volume.value()).collect()
    }

    /// Returns a `Vec` of typical prices `(high + low + close) / 3` in series order.
    pub fn typical_prices(&self) -> Vec<Decimal> {
        self.bars.iter().map(|b| b.typical_price()).collect()
    }

    /// Returns a direct slice of all bars in insertion order.
    pub fn bars(&self) -> &[OhlcvBar] {
        &self.bars
    }

    /// Returns the maximum high price across all bars, or `None` if empty.
    pub fn max_high(&self) -> Option<Decimal> {
        self.bars.iter().map(|b| b.high.value()).reduce(Decimal::max)
    }

    /// Returns the minimum low price across all bars, or `None` if empty.
    pub fn min_low(&self) -> Option<Decimal> {
        self.bars.iter().map(|b| b.low.value()).reduce(Decimal::min)
    }

    /// Returns the highest high price among the last `n` bars, or `None` if empty.
    ///
    /// If `n > self.len()`, considers all bars.
    pub fn highest_high(&self, n: usize) -> Option<Decimal> {
        let start = self.bars.len().saturating_sub(n);
        self.bars[start..].iter().map(|b| b.high.value()).reduce(Decimal::max)
    }

    /// Returns the lowest low price among the last `n` bars, or `None` if empty.
    ///
    /// If `n > self.len()`, considers all bars.
    pub fn lowest_low(&self, n: usize) -> Option<Decimal> {
        let start = self.bars.len().saturating_sub(n);
        self.bars[start..].iter().map(|b| b.low.value()).reduce(Decimal::min)
    }

    /// Returns the volume-weighted average price (VWAP) across all bars, or `None` if empty
    /// or if total volume is zero.
    ///
    /// `VWAP = Σ(typical_price × volume) / Σ(volume)`
    pub fn vwap(&self) -> Option<Decimal> {
        if self.bars.is_empty() {
            return None;
        }
        let total_vol: Decimal = self.bars.iter().map(|b| b.volume.value()).sum();
        if total_vol == Decimal::ZERO {
            return None;
        }
        let weighted_sum: Decimal = self
            .bars
            .iter()
            .map(|b| b.typical_price() * b.volume.value())
            .sum();
        Some(weighted_sum / total_vol)
    }

    /// Returns the total traded volume across all bars in the series.
    pub fn sum_volume(&self) -> Decimal {
        self.bars.iter().map(|b| b.volume.value()).sum()
    }

    /// Returns a sub-slice `bars[from..to]`, or `None` if the range is out of bounds.
    pub fn slice(&self, from: usize, to: usize) -> Option<&[OhlcvBar]> {
        if from > to || to > self.bars.len() {
            return None;
        }
        Some(&self.bars[from..to])
    }

    /// Retains only the last `n` bars, dropping older ones.
    ///
    /// If `n >= self.len()`, this is a no-op.
    pub fn truncate(&mut self, n: usize) {
        let len = self.bars.len();
        if n < len {
            self.bars.drain(0..len - n);
        }
    }

    /// Pushes multiple bars into the series, validating each one.
    ///
    /// Stops and returns the first error encountered; bars added before the error remain.
    ///
    /// # Errors
    /// Returns [`FinError::BarInvariant`] if any bar fails OHLCV invariant checks.
    pub fn extend(&mut self, bars: impl IntoIterator<Item = OhlcvBar>) -> Result<(), FinError> {
        for bar in bars {
            self.push(bar)?;
        }
        Ok(())
    }

    /// Appends all bars from `other` into this series, validating each one.
    ///
    /// # Errors
    /// Returns [`FinError::BarInvariant`] if any bar from `other` fails validation.
    pub fn extend_from_series(&mut self, other: &OhlcvSeries) -> Result<(), FinError> {
        for bar in &other.bars {
            self.push(bar.clone())?;
        }
        Ok(())
    }

    /// Converts the series into a `Vec<BarInput>` for batch signal processing.
    ///
    /// Allows feeding an entire historical series into indicators without manually
    /// iterating and converting each bar.
    pub fn to_bar_inputs(&self) -> Vec<crate::signals::BarInput> {
        self.bars
            .iter()
            .map(crate::signals::BarInput::from)
            .collect()
    }

    /// Feeds every bar in the series into `signal` and collects the results.
    ///
    /// Errors from individual bars are propagated immediately (fail-fast).
    /// Use this for batch back-testing where you want to apply one signal to
    /// an entire historical dataset in one call.
    ///
    /// # Errors
    /// Returns [`FinError`] if any call to `signal.update_bar()` fails.
    pub fn apply_signal(
        &self,
        signal: &mut dyn crate::signals::Signal,
    ) -> Result<Vec<crate::signals::SignalValue>, FinError> {
        self.bars.iter().map(|b| signal.update_bar(b)).collect()
    }

    /// Returns close-to-close percentage returns: `(close[i] - close[i-1]) / close[i-1]`.
    ///
    /// Returns an empty `Vec` when the series has fewer than 2 bars.
    /// Skips any bar where `close[i-1]` is zero to avoid division by zero.
    pub fn returns(&self) -> Vec<Decimal> {
        if self.bars.len() < 2 {
            return Vec::new();
        }
        self.bars
            .windows(2)
            .filter_map(|w| {
                let prev = w[0].close.value();
                if prev.is_zero() {
                    return None;
                }
                Some((w[1].close.value() - prev) / prev)
            })
            .collect()
    }

    /// Returns the highest close price among the last `n` bars, or `None` if empty.
    ///
    /// If `n > self.len()`, considers all bars.
    pub fn highest_close(&self, n: usize) -> Option<Decimal> {
        let start = self.bars.len().saturating_sub(n);
        self.bars[start..].iter().map(|b| b.close.value()).reduce(Decimal::max)
    }

    /// Returns the lowest close price among the last `n` bars, or `None` if empty.
    ///
    /// If `n > self.len()`, considers all bars.
    pub fn lowest_close(&self, n: usize) -> Option<Decimal> {
        let start = self.bars.len().saturating_sub(n);
        self.bars[start..].iter().map(|b| b.close.value()).reduce(Decimal::min)
    }

    /// Computes Pearson correlation between this series' close prices and `other`'s.
    ///
    /// Uses only the overlapping suffix: `min(self.len(), other.len())` bars from the end.
    /// Returns `None` when fewer than 2 overlapping bars exist or standard deviation is zero.
    pub fn correlation(&self, other: &OhlcvSeries) -> Option<Decimal> {
        let n = self.bars.len().min(other.bars.len());
        if n < 2 {
            return None;
        }
        let xs: Vec<Decimal> = self.bars[self.bars.len() - n..].iter().map(|b| b.close.value()).collect();
        let ys: Vec<Decimal> = other.bars[other.bars.len() - n..].iter().map(|b| b.close.value()).collect();
        let n_dec = Decimal::from(n);
        let mean_x: Decimal = xs.iter().copied().sum::<Decimal>() / n_dec;
        let mean_y: Decimal = ys.iter().copied().sum::<Decimal>() / n_dec;
        let cov: Decimal = xs.iter().zip(ys.iter())
            .map(|(x, y)| (*x - mean_x) * (*y - mean_y))
            .sum::<Decimal>() / n_dec;
        let var_x: Decimal = xs.iter().map(|x| (*x - mean_x) * (*x - mean_x)).sum::<Decimal>() / n_dec;
        let var_y: Decimal = ys.iter().map(|y| (*y - mean_y) * (*y - mean_y)).sum::<Decimal>() / n_dec;
        if var_x.is_zero() || var_y.is_zero() {
            return None;
        }
        // sqrt via Newton-Raphson (same approach as BollingerB)
        let std_x = decimal_sqrt(var_x).ok()?;
        let std_y = decimal_sqrt(var_y).ok()?;
        Some(cov / (std_x * std_y))
    }

    /// Returns rolling SMA of close prices with the given `period`.
    ///
    /// The output `Vec` has the same length as the series. Positions where fewer than
    /// `period` bars have been seen contain `None`; the rest contain `Some(sma)`.
    pub fn rolling_sma(&self, period: usize) -> Vec<Option<Decimal>> {
        if period == 0 {
            return self.bars.iter().map(|_| None).collect();
        }
        let closes: Vec<Decimal> = self.bars.iter().map(|b| b.close.value()).collect();
        closes
            .windows(period)
            .enumerate()
            .fold(vec![None; closes.len()], |mut acc, (i, window)| {
                let sum: Decimal = window.iter().copied().sum();
                acc[i + period - 1] = Some(sum / Decimal::from(period as u64));
                acc
            })
    }

    /// Returns rolling z-score of close prices using a window of `period` bars.
    ///
    /// `z = (close - SMA) / stddev`. Positions with insufficient data or zero stddev
    /// yield `None`.
    pub fn zscore(&self, period: usize) -> Vec<Option<Decimal>> {
        if period < 2 {
            return self.bars.iter().map(|_| None).collect();
        }
        let closes: Vec<Decimal> = self.bars.iter().map(|b| b.close.value()).collect();
        let n = closes.len();
        let mut result = vec![None; n];
        let period_dec = Decimal::from(period as u64);
        for i in (period - 1)..n {
            let window = &closes[(i + 1 - period)..=i];
            let mean: Decimal = window.iter().copied().sum::<Decimal>() / period_dec;
            let variance: Decimal = window
                .iter()
                .map(|x| (*x - mean) * (*x - mean))
                .sum::<Decimal>()
                / period_dec;
            if let Ok(std_dev) = decimal_sqrt(variance) {
                if !std_dev.is_zero() {
                    result[i] = Some((closes[i] - mean) / std_dev);
                }
            }
        }
        result
    }

    /// Returns log returns: `ln(close[i] / close[i-1])` for each consecutive bar pair.
    ///
    /// Returns an empty `Vec` when fewer than 2 bars are present.
    /// Bars where `close[i-1]` is zero are skipped (yielding no entry at that position).
    ///
    /// Uses `f64` arithmetic since `rust_decimal` does not provide a logarithm function.
    #[allow(clippy::cast_precision_loss)]
    pub fn log_returns(&self) -> Vec<f64> {
        if self.bars.len() < 2 {
            return Vec::new();
        }
        self.bars
            .windows(2)
            .filter_map(|w| {
                let prev = w[0].close.value();
                if prev.is_zero() {
                    return None;
                }
                let ratio = w[1].close.value().checked_div(prev)?;
                use rust_decimal::prelude::ToPrimitive;
                let ratio_f64 = ratio.to_f64()?;
                if ratio_f64 > 0.0 {
                    Some(ratio_f64.ln())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Returns compounded cumulative returns relative to the first bar's close.
    ///
    /// `cumret[i] = (close[i] / close[0]) - 1`
    ///
    /// Returns an empty `Vec` when the series is empty or the first close is zero.
    pub fn cumulative_returns(&self) -> Vec<Decimal> {
        let first = match self.bars.first() {
            Some(b) => b.close.value(),
            None => return Vec::new(),
        };
        if first.is_zero() {
            return Vec::new();
        }
        self.bars
            .iter()
            .map(|b| b.close.value() / first - Decimal::ONE)
            .collect()
    }

    /// Resamples the series by merging every `n` consecutive bars into one.
    ///
    /// Trailing bars that don't fill a full group of `n` are merged into the last output bar.
    /// Returns an empty `Vec` when `n == 0` or the series is empty.
    ///
    /// # Errors
    /// Returns [`FinError::BarInvariant`] if any merged bar fails invariant checks.
    pub fn resample(&self, n: usize) -> Result<Vec<OhlcvBar>, FinError> {
        if n == 0 || self.bars.is_empty() {
            return Ok(Vec::new());
        }
        let mut result = Vec::new();
        let mut chunks = self.bars.chunks(n);
        for chunk in &mut chunks {
            let mut merged = chunk[0].clone();
            for b in &chunk[1..] {
                merged = merged.merge(b)?;
            }
            result.push(merged);
        }
        Ok(result)
    }
}

impl Default for OhlcvSeries {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> IntoIterator for &'a OhlcvSeries {
    type Item = &'a OhlcvBar;
    type IntoIter = std::slice::Iter<'a, OhlcvBar>;

    fn into_iter(self) -> Self::IntoIter {
        self.bars.iter()
    }
}

fn decimal_sqrt(n: Decimal) -> Result<Decimal, FinError> {
    if n.is_zero() {
        return Ok(Decimal::ZERO);
    }
    if n.is_sign_negative() {
        return Err(FinError::ArithmeticOverflow);
    }
    let mut x = n;
    for _ in 0..20 {
        let next = (x + n / x) / Decimal::TWO;
        let diff = if next > x { next - x } else { x - next };
        x = next;
        if diff < Decimal::new(1, 10) {
            break;
        }
    }
    Ok(x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Side;
    use rust_decimal_macros::dec;

    fn make_price(s: &str) -> Price {
        Price::new(s.parse().unwrap()).unwrap()
    }

    fn make_qty(s: &str) -> Quantity {
        Quantity::new(s.parse().unwrap()).unwrap()
    }

    fn make_bar(o: &str, h: &str, l: &str, c: &str) -> OhlcvBar {
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: make_price(o),
            high: make_price(h),
            low: make_price(l),
            close: make_price(c),
            volume: make_qty("100"),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    fn make_tick(sym: &str, price: &str, qty: &str, ts: i64) -> Tick {
        Tick::new(
            Symbol::new(sym).unwrap(),
            make_price(price),
            make_qty(qty),
            Side::Ask,
            NanoTimestamp::new(ts),
        )
    }

    // --- OhlcvBar ---

    #[test]
    fn test_ohlcv_bar_validate_ok() {
        let bar = make_bar("100", "110", "90", "105");
        assert!(bar.validate().is_ok());
    }

    #[test]
    fn test_ohlcv_bar_validate_high_less_than_close_fails() {
        let bar = make_bar("100", "104", "90", "110");
        assert!(matches!(bar.validate(), Err(FinError::BarInvariant(_))));
    }

    #[test]
    fn test_ohlcv_bar_validate_low_greater_than_open_fails() {
        let bar = make_bar("80", "110", "90", "105");
        assert!(matches!(bar.validate(), Err(FinError::BarInvariant(_))));
    }

    #[test]
    fn test_ohlcv_bar_validate_high_less_than_open_fails() {
        let bar = make_bar("115", "110", "90", "105");
        assert!(matches!(bar.validate(), Err(FinError::BarInvariant(_))));
    }

    #[test]
    fn test_ohlcv_bar_typical_price() {
        let bar = make_bar("100", "120", "80", "110");
        let expected = dec!(310) / Decimal::from(3u32);
        assert_eq!(bar.typical_price(), expected);
    }

    #[test]
    fn test_ohlcv_bar_range() {
        let bar = make_bar("100", "120", "80", "110");
        assert_eq!(bar.range(), dec!(40));
    }

    #[test]
    fn test_ohlcv_bar_is_bullish_true() {
        let bar = make_bar("100", "110", "95", "105");
        assert!(bar.is_bullish());
    }

    #[test]
    fn test_ohlcv_bar_is_bullish_false() {
        let bar = make_bar("105", "110", "95", "100");
        assert!(!bar.is_bullish());
    }

    #[test]
    fn test_ohlcv_bar_midpoint() {
        let bar = make_bar("100", "120", "80", "110");
        assert_eq!(bar.midpoint(), dec!(100)); // (120 + 80) / 2
    }

    #[test]
    fn test_ohlcv_bar_body_size_bullish() {
        let bar = make_bar("100", "120", "80", "110");
        assert_eq!(bar.body_size(), dec!(10)); // |110 - 100|
    }

    #[test]
    fn test_ohlcv_bar_body_size_bearish() {
        let bar = make_bar("110", "120", "80", "100");
        assert_eq!(bar.body_size(), dec!(10)); // |100 - 110|
    }

    #[test]
    fn test_ohlcv_bar_is_doji_flat_range() {
        let bar = make_bar("100", "100", "100", "100");
        assert!(bar.is_doji(dec!(0.1)));
        assert!(!bar.is_doji(dec!(0)));
    }

    #[test]
    fn test_ohlcv_bar_is_doji_small_body() {
        // range = 20, body = 1 → body/range = 0.05 < 0.1 threshold
        let bar = make_bar("100", "110", "90", "101");
        assert!(bar.is_doji(dec!(0.1)));
        assert!(!bar.is_doji(dec!(0.04)));
    }

    #[test]
    fn test_ohlcv_bar_partial_eq() {
        let a = make_bar("100", "110", "90", "105");
        let b = make_bar("100", "110", "90", "105");
        assert_eq!(a, b);
        let c = make_bar("100", "110", "90", "106");
        assert_ne!(a, c);
    }

    // --- Timeframe ---

    #[test]
    fn test_timeframe_seconds_to_nanos() {
        let tf = Timeframe::Seconds(5);
        assert_eq!(tf.to_nanos().unwrap(), 5_000_000_000);
    }

    #[test]
    fn test_timeframe_minutes_to_nanos() {
        let tf = Timeframe::Minutes(1);
        assert_eq!(tf.to_nanos().unwrap(), 60_000_000_000);
    }

    #[test]
    fn test_timeframe_zero_seconds_fails() {
        let tf = Timeframe::Seconds(0);
        assert!(matches!(tf.to_nanos(), Err(FinError::InvalidTimeframe)));
    }

    #[test]
    fn test_timeframe_weeks_to_nanos() {
        let tf = Timeframe::Weeks(1);
        assert_eq!(tf.to_nanos().unwrap(), 7 * 86_400 * 1_000_000_000_i64);
    }

    #[test]
    fn test_timeframe_bucket_start() {
        let tf = Timeframe::Seconds(60);
        let nanos_per_min = 60_000_000_000_i64;
        let ts = NanoTimestamp::new(nanos_per_min + 500_000_000);
        let bucket = tf.bucket_start(ts).unwrap();
        assert_eq!(bucket.nanos(), nanos_per_min);
    }

    // --- OhlcvAggregator ---

    #[test]
    fn test_ohlcv_aggregator_new_invalid_timeframe_fails() {
        let sym = Symbol::new("X").unwrap();
        let result = OhlcvAggregator::new(sym, Timeframe::Seconds(0));
        assert!(matches!(result, Err(FinError::InvalidTimeframe)));
    }

    #[test]
    fn test_ohlcv_aggregator_completes_bar_on_boundary() {
        let sym = Symbol::new("X").unwrap();
        let mut agg = OhlcvAggregator::new(sym, Timeframe::Seconds(60)).unwrap();
        let nanos_per_min = 60_000_000_000_i64;

        let t1 = make_tick("X", "100", "1", 0);
        let t2 = make_tick("X", "105", "2", nanos_per_min / 2);
        let t3 = make_tick("X", "110", "1", nanos_per_min + 1);

        let r1 = agg.push_tick(&t1).unwrap();
        assert!(r1.is_empty());
        let r2 = agg.push_tick(&t2).unwrap();
        assert!(r2.is_empty());
        let r3 = agg.push_tick(&t3).unwrap();
        assert_eq!(r3.len(), 1);
        let bar = &r3[0];
        assert_eq!(bar.open.value(), dec!(100));
        assert_eq!(bar.high.value(), dec!(105));
        assert_eq!(bar.close.value(), dec!(105));
        assert_eq!(bar.tick_count, 2);
    }

    #[test]
    fn test_ohlcv_aggregator_gap_fills_empty_buckets() {
        let sym = Symbol::new("X").unwrap();
        let mut agg = OhlcvAggregator::new(sym, Timeframe::Seconds(60)).unwrap();
        let nanos_per_min = 60_000_000_000_i64;

        // First bar in minute 0.
        agg.push_tick(&make_tick("X", "100", "1", 0)).unwrap();
        // Tick jumps 3 minutes ahead: should emit bar for min 0 + gap bars for min 1, min 2.
        let out = agg
            .push_tick(&make_tick("X", "200", "1", 3 * nanos_per_min + 1))
            .unwrap();
        // 1 completed bar + 2 gap bars
        assert_eq!(out.len(), 3, "expected 1 completed + 2 gap bars, got {}", out.len());
        // Completed bar has real data.
        assert_eq!(out[0].tick_count, 1);
        // Gap bars have zero volume and tick_count.
        assert_eq!(out[1].tick_count, 0);
        assert_eq!(out[1].volume.value(), dec!(0));
        assert_eq!(out[2].tick_count, 0);
        // Gap bars use the last close.
        assert_eq!(out[1].close, out[0].close);
    }

    #[test]
    fn test_ohlcv_aggregator_flush_returns_partial() {
        let sym = Symbol::new("X").unwrap();
        let mut agg = OhlcvAggregator::new(sym, Timeframe::Seconds(60)).unwrap();
        let t1 = make_tick("X", "100", "1", 0);
        agg.push_tick(&t1).unwrap();
        let bar = agg.flush().unwrap();
        assert_eq!(bar.open.value(), dec!(100));
        assert!(agg.flush().is_none());
    }

    #[test]
    fn test_ohlcv_aggregator_symbol_getter() {
        let sym = Symbol::new("BTC").unwrap();
        let agg = OhlcvAggregator::new(sym.clone(), Timeframe::Seconds(60)).unwrap();
        assert_eq!(agg.symbol(), &sym);
    }

    #[test]
    fn test_ohlcv_aggregator_ignores_different_symbol() {
        let sym = Symbol::new("X").unwrap();
        let mut agg = OhlcvAggregator::new(sym, Timeframe::Seconds(60)).unwrap();
        let t = make_tick("Y", "100", "1", 0);
        let result = agg.push_tick(&t).unwrap();
        assert!(result.is_empty());
        assert!(agg.current_bar().is_none());
    }

    // --- OhlcvSeries ---

    #[test]
    fn test_ohlcv_series_push_valid() {
        let mut series = OhlcvSeries::new();
        let bar = make_bar("100", "110", "90", "105");
        assert!(series.push(bar).is_ok());
        assert_eq!(series.len(), 1);
    }

    #[test]
    fn test_ohlcv_series_push_invalid_fails() {
        let mut series = OhlcvSeries::new();
        let bar = make_bar("100", "95", "90", "105");
        assert!(matches!(series.push(bar), Err(FinError::BarInvariant(_))));
    }

    #[test]
    fn test_ohlcv_series_window_returns_last_n() {
        let mut series = OhlcvSeries::new();
        for i in 1u32..=5 {
            let p = format!("{}", 100 + i);
            let h = format!("{}", 110 + i);
            let l = format!("{}", 90 + i);
            let c = format!("{}", 105 + i);
            series.push(make_bar(&p, &h, &l, &c)).unwrap();
        }
        let w = series.window(3);
        assert_eq!(w.len(), 3);
        assert_eq!(w[0].open.value(), dec!(103));
    }

    #[test]
    fn test_ohlcv_series_window_larger_than_len() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "105")).unwrap();
        let w = series.window(10);
        assert_eq!(w.len(), 1);
    }

    #[test]
    fn test_ohlcv_series_opens() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "105")).unwrap();
        series.push(make_bar("105", "115", "95", "110")).unwrap();
        assert_eq!(series.opens(), vec![dec!(100), dec!(105)]);
    }

    #[test]
    fn test_ohlcv_series_highs() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "105")).unwrap();
        series.push(make_bar("105", "115", "95", "110")).unwrap();
        assert_eq!(series.highs(), vec![dec!(110), dec!(115)]);
    }

    #[test]
    fn test_ohlcv_series_lows() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "105")).unwrap();
        series.push(make_bar("105", "115", "95", "110")).unwrap();
        assert_eq!(series.lows(), vec![dec!(90), dec!(95)]);
    }

    #[test]
    fn test_ohlcv_series_closes() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "105")).unwrap();
        series.push(make_bar("105", "115", "95", "110")).unwrap();
        let closes = series.closes();
        assert_eq!(closes, vec![dec!(105), dec!(110)]);
    }

    #[test]
    fn test_ohlcv_series_is_empty() {
        let series = OhlcvSeries::new();
        assert!(series.is_empty());
    }

    #[test]
    fn test_ohlcv_series_into_iterator() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "105")).unwrap();
        series.push(make_bar("105", "115", "95", "110")).unwrap();
        let count = (&series).into_iter().count();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_ohlcv_series_iter() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "105")).unwrap();
        let bar = series.iter().next().unwrap();
        assert_eq!(bar.open.value(), dec!(100));
    }

    #[test]
    fn test_ohlcv_bar_upper_shadow() {
        // bullish: open=100, close=108, high=115 → upper = 115-108 = 7
        let b = make_bar("100", "115", "90", "108");
        assert_eq!(b.upper_shadow(), dec!(7));
    }

    #[test]
    fn test_ohlcv_bar_lower_shadow() {
        // bullish: open=100, close=108, low=90 → lower = 100-90 = 10
        let b = make_bar("100", "115", "90", "108");
        assert_eq!(b.lower_shadow(), dec!(10));
    }

    #[test]
    fn test_ohlcv_bar_from_tick() {
        let tick = make_tick("AAPL", "150", "5", 1_000);
        let bar = OhlcvBar::from_tick(&tick);
        assert_eq!(bar.open.value(), dec!(150));
        assert_eq!(bar.high.value(), dec!(150));
        assert_eq!(bar.low.value(), dec!(150));
        assert_eq!(bar.close.value(), dec!(150));
        assert_eq!(bar.volume.value(), dec!(5));
        assert_eq!(bar.tick_count, 1);
        assert_eq!(bar.ts_open.nanos(), 1_000);
    }

    #[test]
    fn test_ohlcv_series_bars_slice() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "105")).unwrap();
        series.push(make_bar("105", "115", "95", "110")).unwrap();
        assert_eq!(series.bars().len(), 2);
    }

    #[test]
    fn test_ohlcv_series_max_high_min_low() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "105")).unwrap();
        series.push(make_bar("105", "120", "85", "110")).unwrap();
        assert_eq!(series.max_high().unwrap(), dec!(120));
        assert_eq!(series.min_low().unwrap(), dec!(85));
    }

    #[test]
    fn test_ohlcv_series_max_high_empty() {
        let series = OhlcvSeries::new();
        assert!(series.max_high().is_none());
        assert!(series.min_low().is_none());
    }

    #[test]
    fn test_ohlcv_series_slice() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "105")).unwrap();
        series.push(make_bar("105", "115", "95", "110")).unwrap();
        series.push(make_bar("110", "120", "100", "115")).unwrap();
        let s = series.slice(1, 3).unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].open.value(), dec!(105));
    }

    #[test]
    fn test_ohlcv_series_slice_out_of_bounds() {
        let series = OhlcvSeries::new();
        assert!(series.slice(0, 1).is_none());
    }

    #[test]
    fn test_ohlcv_series_truncate_keeps_last_n() {
        let mut series = OhlcvSeries::new();
        for _ in 0..5 {
            series.push(make_bar("100", "110", "90", "105")).unwrap();
        }
        series.truncate(3);
        assert_eq!(series.len(), 3);
    }

    #[test]
    fn test_ohlcv_series_truncate_noop_when_n_ge_len() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "105")).unwrap();
        series.push(make_bar("105", "115", "95", "110")).unwrap();
        series.truncate(5);
        assert_eq!(series.len(), 2);
    }

    #[test]
    fn test_ohlcv_series_truncate_to_zero() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "105")).unwrap();
        series.push(make_bar("105", "115", "95", "110")).unwrap();
        series.truncate(0);
        assert!(series.is_empty());
    }

    #[test]
    fn test_ohlcv_bar_serde_roundtrip() {
        let bar = make_bar("100", "110", "90", "105");
        let json = serde_json::to_string(&bar).unwrap();
        let back: OhlcvBar = serde_json::from_str(&json).unwrap();
        assert_eq!(back.open, bar.open);
        assert_eq!(back.high, bar.high);
        assert_eq!(back.low, bar.low);
        assert_eq!(back.close, bar.close);
        assert_eq!(back.tick_count, bar.tick_count);
    }

    #[test]
    fn test_ohlcv_bar_duration_nanos() {
        let mut bar = make_bar("100", "110", "90", "105");
        bar.ts_open = NanoTimestamp::new(1_000_000_000);
        bar.ts_close = NanoTimestamp::new(1_060_000_000_000);
        assert_eq!(bar.duration_nanos(), 1_059_000_000_000);
    }

    #[test]
    fn test_ohlcv_bar_duration_nanos_same_timestamps() {
        let mut bar = make_bar("100", "110", "90", "100");
        bar.ts_open = NanoTimestamp::new(5_000);
        bar.ts_close = NanoTimestamp::new(5_000);
        assert_eq!(bar.duration_nanos(), 0);
    }

    #[test]
    fn test_ohlcv_series_extend_valid() {
        let mut series = OhlcvSeries::new();
        let bars = vec![
            make_bar("100", "110", "90", "105"),
            make_bar("105", "115", "95", "110"),
        ];
        series.extend(bars).unwrap();
        assert_eq!(series.len(), 2);
    }

    #[test]
    fn test_ohlcv_series_extend_stops_on_invalid_bar() {
        let mut series = OhlcvSeries::new();
        let valid = make_bar("100", "110", "90", "105");
        let mut invalid = make_bar("100", "110", "90", "105");
        // Make bar invalid: high < low
        invalid.high = make_price("80");
        invalid.low = make_price("110");
        let result = series.extend([valid, invalid]);
        assert!(result.is_err());
        assert_eq!(series.len(), 1, "valid bar added before error");
    }

    #[test]
    fn test_ohlcv_bar_to_bar_input_fields_match() {
        let bar = make_bar("100", "110", "90", "105");
        let input = bar.to_bar_input();
        assert_eq!(input.open, bar.open.value());
        assert_eq!(input.high, bar.high.value());
        assert_eq!(input.low, bar.low.value());
        assert_eq!(input.close, bar.close.value());
        assert_eq!(input.volume, bar.volume.value());
    }

    #[test]
    fn test_ohlcv_series_retain_removes_gap_fills() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "105")).unwrap();
        // add a gap-fill bar (tick_count == 0)
        let mut gap = make_bar("105", "105", "105", "105");
        gap.tick_count = 0;
        series.push(gap).unwrap();
        series.push(make_bar("105", "115", "95", "110")).unwrap();
        series.retain(|b| !b.is_gap_fill());
        assert_eq!(series.len(), 2);
    }

    #[test]
    fn test_ohlcv_series_retain_keeps_all() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "105")).unwrap();
        series.push(make_bar("105", "115", "95", "110")).unwrap();
        series.retain(|_| true);
        assert_eq!(series.len(), 2);
    }

    #[test]
    fn test_ohlcv_bar_is_bearish() {
        let bar = make_bar("110", "115", "95", "100");
        assert!(bar.is_bearish());
        assert!(!bar.is_bullish());
    }

    #[test]
    fn test_ohlcv_bar_is_hammer() {
        // body = 5 (100→105), lower shadow = 20 (80→100), upper shadow = 6 (105→111) → NOT hammer (upper > body)
        let not_hammer = make_bar("100", "111", "80", "105");
        assert!(!not_hammer.is_hammer());
        // body = 5, lower shadow = 20 (75→95), upper shadow = 0 → IS hammer
        let hammer = make_bar("95", "100", "75", "100");
        assert!(hammer.is_hammer());
    }

    #[test]
    fn test_ohlcv_bar_is_shooting_star() {
        // body = 5, upper shadow = 20, lower shadow = 0 → IS shooting star
        let star = make_bar("100", "125", "100", "105");
        assert!(star.is_shooting_star());
        // body = 5, upper shadow = 5, lower shadow = 20 → NOT shooting star
        let not_star = make_bar("100", "110", "80", "105");
        assert!(!not_star.is_shooting_star());
    }

    #[test]
    fn test_ohlcv_bar_bar_return_positive() {
        let bar = make_bar("100", "110", "90", "110");
        assert_eq!(bar.bar_return().unwrap(), dec!(10));
    }

    #[test]
    fn test_ohlcv_bar_bar_return_negative() {
        let bar = make_bar("100", "105", "85", "90");
        assert_eq!(bar.bar_return().unwrap(), dec!(-10));
    }

    #[test]
    fn test_ohlcv_series_highest_high() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "150", "90", "105")).unwrap();
        series.push(make_bar("105", "130", "95", "110")).unwrap();
        series.push(make_bar("110", "120", "100", "115")).unwrap();
        assert_eq!(series.highest_high(2).unwrap(), dec!(130));
        assert_eq!(series.highest_high(10).unwrap(), dec!(150));
    }

    #[test]
    fn test_ohlcv_series_lowest_low() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "70", "105")).unwrap();
        series.push(make_bar("105", "115", "85", "110")).unwrap();
        series.push(make_bar("110", "120", "90", "115")).unwrap();
        assert_eq!(series.lowest_low(2).unwrap(), dec!(85));
        assert_eq!(series.lowest_low(10).unwrap(), dec!(70));
    }

    #[test]
    fn test_ohlcv_series_extend_from_series() {
        let mut a = OhlcvSeries::new();
        a.push(make_bar("100", "110", "90", "105")).unwrap();
        let mut b = OhlcvSeries::new();
        b.push(make_bar("105", "115", "95", "110")).unwrap();
        b.push(make_bar("110", "120", "100", "115")).unwrap();
        a.extend_from_series(&b).unwrap();
        assert_eq!(a.len(), 3);
    }

    #[test]
    fn test_ohlcv_aggregator_bar_count() {
        let sym = Symbol::new("AAPL").unwrap();
        let mut agg = OhlcvAggregator::new(sym, Timeframe::Seconds(1)).unwrap();
        assert_eq!(agg.bar_count(), 0);
        agg.push_tick(&make_tick("AAPL", "100", "1", 0)).unwrap();
        // t=2s lands in bucket [2s,3s): completes [0s,1s) + gap fills [1s,2s) = 2 bars emitted
        agg.push_tick(&make_tick("AAPL", "101", "1", 2_000_000_000))
            .unwrap();
        assert_eq!(agg.bar_count(), 2);
        agg.flush();
        assert_eq!(agg.bar_count(), 3);
        agg.reset();
        assert_eq!(agg.bar_count(), 0);
    }

    #[test]
    fn test_ohlcv_series_vwap_empty_returns_none() {
        assert!(OhlcvSeries::new().vwap().is_none());
    }

    #[test]
    fn test_ohlcv_series_vwap_zero_volume_returns_none() {
        let mut series = OhlcvSeries::new();
        let mut bar = make_bar("100", "110", "90", "100");
        bar.volume = Quantity::zero();
        series.push(bar).unwrap();
        assert!(series.vwap().is_none());
    }

    #[test]
    fn test_ohlcv_series_vwap_constant_price() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "100", "100", "100")).unwrap();
        series.push(make_bar("100", "100", "100", "100")).unwrap();
        assert_eq!(series.vwap().unwrap(), dec!(100));
    }

    #[test]
    fn test_ohlcv_series_sum_volume_empty() {
        assert_eq!(OhlcvSeries::new().sum_volume(), dec!(0));
    }

    #[test]
    fn test_ohlcv_series_sum_volume_multiple_bars() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "105")).unwrap();
        series.push(make_bar("105", "115", "95", "110")).unwrap();
        series.push(make_bar("110", "120", "100", "115")).unwrap();
        // make_bar sets volume = 100 per bar
        assert_eq!(series.sum_volume(), dec!(300));
    }
}
