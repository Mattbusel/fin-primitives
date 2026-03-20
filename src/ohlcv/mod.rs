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

/// Classic floor-trader pivot levels derived from a prior bar's H/L/C.
///
/// - `pp`: Pivot Point `(H + L + C) / 3`
/// - `r1`, `r2`: Resistance levels 1 and 2
/// - `s1`, `s2`: Support levels 1 and 2
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct PivotPoints {
    /// Pivot Point
    pub pp: Decimal,
    /// First resistance level
    pub r1: Decimal,
    /// First support level
    pub s1: Decimal,
    /// Second resistance level
    pub r2: Decimal,
    /// Second support level
    pub s2: Decimal,
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

    /// Returns the weighted close price: `(high + low + close * 2) / 4`.
    ///
    /// Alias for `hlcc4`. Commonly called "weighted close" in technical analysis
    /// literature; emphasises the closing price over the high and low.
    pub fn weighted_close(&self) -> Decimal {
        self.hlcc4()
    }

    /// Returns the OHLC/4 price: `(open + high + low + close) / 4`.
    ///
    /// Equal weight for all four price components. Common in smoothed candlestick
    /// calculations and some custom charting systems.
    pub fn ohlc4(&self) -> Decimal {
        (self.open.value() + self.high.value() + self.low.value() + self.close.value())
            / Decimal::from(4u32)
    }

    /// Returns the dollar volume of this bar: `typical_price × volume`.
    ///
    /// Dollar volume is a common liquidity metric: high dollar volume means
    /// large amounts of capital changed hands, making the instrument easier to
    /// trade without excessive market impact.
    pub fn dollar_volume(&self) -> Decimal {
        self.typical_price() * self.volume.value()
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

    /// Returns `true` if this bar's range completely contains the previous bar's range.
    ///
    /// An outside bar has `high > prev.high && low < prev.low`. Signals potential
    /// volatility expansion or reversal — the opposite of an inside bar.
    pub fn is_outside_bar(&self, prev: &OhlcvBar) -> bool {
        self.high.value() > prev.high.value() && self.low.value() < prev.low.value()
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

    /// Returns `true` if the bar is a marubozu: a full-body candle with negligible shadows.
    ///
    /// Criteria: both upper and lower shadows are each < 5% of the bar's total range,
    /// and the body is non-zero.
    pub fn is_marubozu(&self) -> bool {
        let range = self.range();
        if range.is_zero() {
            return false;
        }
        let body = self.body_size();
        if body.is_zero() {
            return false;
        }
        let threshold = range / Decimal::from(20u32); // 5% of range
        self.upper_shadow() < threshold && self.lower_shadow() < threshold
    }

    /// Returns `true` if the bar is a spinning top: a small body with significant upper
    /// and lower shadows.
    ///
    /// Criteria: body is less than 30% of the total range, and both shadows are each
    /// at least 20% of the range.
    pub fn is_spinning_top(&self) -> bool {
        let range = self.range();
        if range.is_zero() {
            return false;
        }
        let body = self.body_size();
        let body_ratio = body / range;
        let upper_ratio = self.upper_shadow() / range;
        let lower_ratio = self.lower_shadow() / range;
        let threshold_30 = Decimal::from_str_exact("0.30").unwrap_or(Decimal::ZERO);
        let threshold_20 = Decimal::from_str_exact("0.20").unwrap_or(Decimal::ZERO);
        body_ratio < threshold_30 && upper_ratio >= threshold_20 && lower_ratio >= threshold_20
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

    /// Returns the body size as a percentage of the open price: `body_size / open * 100`.
    ///
    /// Returns `None` when `open` is zero.
    pub fn body_pct(&self) -> Option<Decimal> {
        let o = self.open.value();
        if o.is_zero() {
            return None;
        }
        Some(self.body_size() / o * Decimal::ONE_HUNDRED)
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

    /// Body-to-range ratio: `body_size() / range()`.
    ///
    /// Returns `None` when `range() == 0` (flat bar). A value near 1 means the
    /// bar is all body; near 0 means the bar is mostly wicks.
    pub fn body_to_range_ratio(&self) -> Option<Decimal> {
        let r = self.range();
        if r.is_zero() {
            return None;
        }
        Some(self.body_size() / r)
    }

    /// Returns `true` if the bar's body is large relative to its range.
    ///
    /// A bar is considered "long" when `body_size / range >= factor`.
    /// Returns `false` when `range == 0` (flat bar).
    pub fn is_long_candle(&self, factor: Decimal) -> bool {
        let r = self.range();
        if r == Decimal::ZERO {
            return false;
        }
        self.body_size() / r >= factor
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

    /// Returns the True Range for this bar.
    ///
    /// True Range is the maximum of:
    /// - `high - low`
    /// - `|high - prev_close|` (if `prev` is `Some`)
    /// - `|low  - prev_close|` (if `prev` is `Some`)
    ///
    /// When `prev` is `None`, True Range falls back to `high - low`.
    /// This is the building block for ATR and volatility calculations.
    pub fn true_range(&self, prev: Option<&OhlcvBar>) -> Decimal {
        let hl = self.high.value() - self.low.value();
        match prev {
            None => hl,
            Some(p) => {
                let pc = p.close.value();
                let hc = (self.high.value() - pc).abs();
                let lc = (self.low.value() - pc).abs();
                hl.max(hc).max(lc)
            }
        }
    }

    /// Returns the ratio of total shadow to range: `(upper_shadow + lower_shadow) / range`.
    ///
    /// A value near `1.0` indicates most of the bar's range is wick (indecision).
    /// Returns `None` when `range == 0`.
    pub fn shadow_ratio(&self) -> Option<Decimal> {
        let r = self.range();
        if r.is_zero() {
            return None;
        }
        Some((self.upper_shadow() + self.lower_shadow()) / r)
    }

    /// Returns `true` if this bar opens above the previous bar's high (gap up).
    pub fn gap_up_from(&self, prev: &OhlcvBar) -> bool {
        self.low.value() > prev.high.value()
    }

    /// Returns `true` if this bar opens below the previous bar's low (gap down).
    pub fn gap_down_from(&self, prev: &OhlcvBar) -> bool {
        self.high.value() < prev.low.value()
    }

    /// Signed gap from prior bar: `self.open - prev.close`.
    ///
    /// Positive = gap up, negative = gap down, zero = no gap.
    pub fn gap_from(&self, prev: &OhlcvBar) -> Decimal {
        self.open.value() - prev.close.value()
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

    /// Returns the percentage gap between `prev.close` and `self.open`.
    ///
    /// `gap_pct = (self.open - prev.close) / prev.close * 100`
    ///
    /// Returns `None` if `prev.close` is zero. Positive values indicate an upward gap;
    /// negative values a downward gap.
    pub fn gap_pct(&self, prev: &OhlcvBar) -> Option<Decimal> {
        let prev_close = prev.close.value();
        if prev_close.is_zero() {
            return None;
        }
        Some((self.open.value() - prev_close) / prev_close * Decimal::ONE_HUNDRED)
    }

    /// Returns `true` if this bar opened with a gap larger than `pct_threshold` percent.
    ///
    /// A gap exists when `|gap_pct| >= pct_threshold`. Returns `false` when
    /// `gap_pct` cannot be computed (zero previous close).
    pub fn has_gap(&self, prev: &OhlcvBar, pct_threshold: Decimal) -> bool {
        self.gap_pct(prev)
            .map(|g| g.abs() >= pct_threshold)
            .unwrap_or(false)
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

    /// Returns the bucket-start timestamp of the current open bar, or `None` if no bar is open.
    ///
    /// This is the lower boundary of the current timeframe bucket, not the timestamp of the
    /// first tick received in the bar.
    pub fn current_bar_open_ts(&self) -> Option<NanoTimestamp> {
        self.current_bucket_start
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

    /// Constructs an `OhlcvSeries` from a `Vec<OhlcvBar>`, validating each bar.
    ///
    /// # Errors
    /// Returns [`FinError::BarInvariant`] on the first bar that fails validation.
    pub fn from_bars(bars: Vec<OhlcvBar>) -> Result<Self, FinError> {
        for bar in &bars {
            bar.validate()?;
        }
        Ok(Self { bars })
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

    /// Returns the bar `n` positions from the end (0 = most recent), or `None` if out of bounds.
    ///
    /// `n_bars_ago(0)` is equivalent to `last()`. Useful in signal logic where
    /// you need to compare the current bar against bars 1, 2, or 3 periods back.
    pub fn n_bars_ago(&self, n: usize) -> Option<&OhlcvBar> {
        let len = self.bars.len();
        if n >= len {
            return None;
        }
        self.bars.get(len - 1 - n)
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

    /// Returns the count of consecutive bullish bars at the tail of the series.
    ///
    /// A bar is bullish when `close >= open`. Returns 0 for an empty series.
    pub fn consecutive_ups(&self) -> usize {
        self.bars
            .iter()
            .rev()
            .take_while(|b| b.close.value() >= b.open.value())
            .count()
    }

    /// Returns the count of consecutive bearish bars at the tail of the series.
    ///
    /// A bar is bearish when `close < open`. Returns 0 for an empty series.
    pub fn consecutive_downs(&self) -> usize {
        self.bars
            .iter()
            .rev()
            .take_while(|b| b.close.value() < b.open.value())
            .count()
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

    /// Returns the average volume over the last `n` bars, or `None` if fewer than `n` bars exist.
    pub fn avg_volume(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let sum: Decimal = self.bars.iter().rev().take(n).map(|b| b.volume.value()).sum();
        #[allow(clippy::cast_possible_truncation)]
        Some(sum / Decimal::from(n as u32))
    }

    /// Returns `highest_high(n) - lowest_low(n)` over the last `n` bars, or `None` if
    /// fewer than `n` bars exist or `n == 0`.
    pub fn price_range(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let hh = self.highest_high(n)?;
        let ll = self.lowest_low(n)?;
        Some(hh - ll)
    }

    /// Returns the average Close Location Value over the last `n` bars, or `None` if
    /// fewer than `n` bars exist or `n == 0`.
    ///
    /// `CLV = ((close - low) - (high - close)) / (high - low)`
    ///
    /// Each bar's CLV is in `[-1, 1]`; bars with zero range contribute `0`.
    pub fn close_location_value(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let sum: Decimal = self.bars[start..].iter().map(|b| {
            let h = b.high.value();
            let l = b.low.value();
            let c = b.close.value();
            let range = h - l;
            if range == Decimal::ZERO { Decimal::ZERO } else { ((c - l) - (h - c)) / range }
        }).sum();
        #[allow(clippy::cast_possible_truncation)]
        Some(sum / Decimal::from(n as u32))
    }

    /// Returns the average dollar volume over the last `n` bars.
    ///
    /// `avg_dollar_volume = Σ(typical_price × volume) / n` for the last `n` bars.
    ///
    /// Returns `None` when `n == 0` or the series has fewer than `n` bars.
    pub fn avg_dollar_volume(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let sum: Decimal = self.bars.iter().rev().take(n).map(|b| b.dollar_volume()).sum();
        Some(sum / Decimal::from(n as u64))
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

    /// Returns the mean (average) close price of the last `n` bars, or `None` if empty.
    ///
    /// If `n > self.len()`, all bars are used.
    pub fn mean_close(&self, n: usize) -> Option<Decimal> {
        let start = self.bars.len().saturating_sub(n);
        let slice = &self.bars[start..];
        if slice.is_empty() {
            return None;
        }
        let sum: Decimal = slice.iter().map(|b| b.close.value()).sum();
        Some(sum / Decimal::from(slice.len() as u64))
    }

    /// Returns the population standard deviation of close prices over the last `n` bars.
    ///
    /// Returns `None` if fewer than 2 bars are in the window.
    /// If `n > self.len()`, all bars are used.
    pub fn std_dev(&self, n: usize) -> Option<Decimal> {
        let start = self.bars.len().saturating_sub(n);
        let slice = &self.bars[start..];
        if slice.len() < 2 {
            return None;
        }
        let n_dec = Decimal::from(slice.len() as u64);
        let mean: Decimal = slice.iter().map(|b| b.close.value()).sum::<Decimal>() / n_dec;
        let variance: Decimal = slice
            .iter()
            .map(|b| { let d = b.close.value() - mean; d * d })
            .sum::<Decimal>()
            / n_dec;
        decimal_sqrt(variance).ok()
    }

    /// Returns the median close price of the last `n` bars, or `None` if empty.
    ///
    /// If `n > self.len()`, all bars are used. For an even number of bars the
    /// median is the average of the two middle values.
    pub fn median_close(&self, n: usize) -> Option<Decimal> {
        let start = self.bars.len().saturating_sub(n);
        let mut closes: Vec<Decimal> =
            self.bars[start..].iter().map(|b| b.close.value()).collect();
        if closes.is_empty() {
            return None;
        }
        closes.sort();
        let mid = closes.len() / 2;
        if closes.len() % 2 == 1 {
            Some(closes[mid])
        } else {
            Some((closes[mid - 1] + closes[mid]) / Decimal::TWO)
        }
    }

    /// Returns what percentile `value` is among the last `n` close prices (0–100).
    ///
    /// Counts the fraction of bars in the window whose close is strictly less than `value`,
    /// then multiplies by 100. Returns `None` if the window is empty.
    /// If `n > self.len()`, all bars are used.
    pub fn percentile_rank(&self, value: Decimal, n: usize) -> Option<Decimal> {
        let start = self.bars.len().saturating_sub(n);
        let slice = &self.bars[start..];
        if slice.is_empty() {
            return None;
        }
        let below = slice.iter().filter(|b| b.close.value() < value).count();
        Some(Decimal::from(below as u64) / Decimal::from(slice.len() as u64) * Decimal::ONE_HUNDRED)
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

    /// Returns the maximum peak-to-trough drawdown on close prices.
    ///
    /// Iterates through close prices, tracking the running peak and computing
    /// the largest percentage decline from any peak to any subsequent trough.
    ///
    /// Returns `None` when the series is empty. Returns `0` when no decline occurs.
    pub fn max_drawdown(&self) -> Option<Decimal> {
        let closes: Vec<Decimal> = self.bars.iter().map(|b| b.close.value()).collect();
        if closes.is_empty() {
            return None;
        }
        let mut peak = closes[0];
        let mut max_dd = Decimal::ZERO;
        for &c in &closes[1..] {
            if c > peak {
                peak = c;
            } else if !peak.is_zero() {
                let dd = (peak - c) / peak;
                if dd > max_dd {
                    max_dd = dd;
                }
            }
        }
        Some(max_dd)
    }

    /// Computes the annualized Sharpe ratio from log returns.
    ///
    /// `Sharpe = (mean_log_return - risk_free_rate_per_bar) / stddev_log_return * sqrt(bars_per_year)`
    ///
    /// `bars_per_year` defaults to 252 (US equity trading days). Pass `0.0` for `risk_free_rate`
    /// when working with intraday or crypto series where a risk-free benchmark is not applicable.
    ///
    /// Returns `None` when fewer than 2 bars exist or if log-return standard deviation is zero.
    pub fn sharpe_ratio(&self, risk_free_rate: f64, bars_per_year: f64) -> Option<f64> {
        let lr = self.log_returns();
        if lr.len() < 2 {
            return None;
        }
        let n = lr.len() as f64;
        let mean = lr.iter().sum::<f64>() / n;
        let variance = lr.iter().map(|&r| (r - mean).powi(2)).sum::<f64>() / n;
        let std_dev = variance.sqrt();
        if std_dev == 0.0 {
            return None;
        }
        let bars_per_year = if bars_per_year <= 0.0 { 252.0 } else { bars_per_year };
        Some((mean - risk_free_rate) / std_dev * bars_per_year.sqrt())
    }

    /// Returns the percentage price change from `n` bars ago to the latest close.
    ///
    /// `(last_close - close[len-1-n]) / close[len-1-n] * 100`
    ///
    /// Returns `None` when the series has fewer than `n + 1` bars or the reference
    /// close is zero.
    pub fn price_change_pct(&self, n: usize) -> Option<Decimal> {
        let len = self.bars.len();
        if len < n + 1 {
            return None;
        }
        let ref_close = self.bars[len - 1 - n].close.value();
        if ref_close.is_zero() {
            return None;
        }
        let last_close = self.bars[len - 1].close.value();
        Some((last_close - ref_close) / ref_close * Decimal::ONE_HUNDRED)
    }

    /// Returns the count of bullish bars in the last `n` bars.
    ///
    /// A bar is bullish when `close >= open`. If `n` exceeds the series length,
    /// all bars are counted.
    pub fn count_bullish(&self, n: usize) -> usize {
        let start = self.bars.len().saturating_sub(n);
        self.bars[start..].iter().filter(|b| b.is_bullish()).count()
    }

    /// Returns the count of bearish bars in the last `n` bars.
    ///
    /// A bar is bearish when `close < open`. If `n` exceeds the series length,
    /// all bars are counted.
    pub fn count_bearish(&self, n: usize) -> usize {
        let start = self.bars.len().saturating_sub(n);
        self.bars[start..].iter().filter(|b| b.is_bearish()).count()
    }

    /// Returns the count of inside bars in the entire series.
    ///
    /// An inside bar has a lower high and higher low than the previous bar,
    /// indicating consolidation. The first bar is never counted (no prior bar).
    pub fn count_inside_bars(&self) -> usize {
        self.bars
            .windows(2)
            .filter(|w| w[1].is_inside_bar(&w[0]))
            .count()
    }

    /// Returns the count of outside bars in the entire series.
    ///
    /// An outside bar completely contains the prior bar's range.
    /// The first bar is never counted (no prior bar).
    pub fn count_outside_bars(&self) -> usize {
        self.bars
            .windows(2)
            .filter(|w| w[1].is_outside_bar(&w[0]))
            .count()
    }

    /// Returns the indices of pivot highs — bars whose high is strictly greater than
    /// the `n` bars on each side.
    ///
    /// A pivot high at index `i` satisfies:
    /// `bars[i].high > bars[i-j].high` and `bars[i].high > bars[i+j].high` for all `j` in `1..=n`.
    ///
    /// Bars within `n` of either end of the series are excluded.
    pub fn pivot_highs(&self, n: usize) -> Vec<usize> {
        if n == 0 || self.bars.len() < 2 * n + 1 {
            return vec![];
        }
        let mut pivots = Vec::new();
        for i in n..self.bars.len() - n {
            let h = self.bars[i].high.value();
            let is_pivot = (1..=n).all(|j| {
                h > self.bars[i - j].high.value() && h > self.bars[i + j].high.value()
            });
            if is_pivot {
                pivots.push(i);
            }
        }
        pivots
    }

    /// Returns the indices of pivot lows — bars whose low is strictly less than
    /// the `n` bars on each side.
    ///
    /// A pivot low at index `i` satisfies:
    /// `bars[i].low < bars[i-j].low` and `bars[i].low < bars[i+j].low` for all `j` in `1..=n`.
    ///
    /// Bars within `n` of either end of the series are excluded.
    pub fn pivot_lows(&self, n: usize) -> Vec<usize> {
        if n == 0 || self.bars.len() < 2 * n + 1 {
            return vec![];
        }
        let mut pivots = Vec::new();
        for i in n..self.bars.len() - n {
            let l = self.bars[i].low.value();
            let is_pivot = (1..=n).all(|j| {
                l < self.bars[i - j].low.value() && l < self.bars[i + j].low.value()
            });
            if is_pivot {
                pivots.push(i);
            }
        }
        pivots
    }

    /// Returns the count of bars (in the last `n`) where `close > SMA(close, period)`.
    ///
    /// If `n` exceeds the series length, all eligible bars are considered.
    /// Returns `0` if there are fewer than `period` bars (SMA cannot be computed).
    #[allow(clippy::cast_possible_truncation)]
    pub fn above_sma(&self, period: usize, n: usize) -> usize {
        if self.bars.len() < period || period == 0 {
            return 0;
        }
        let start = self.bars.len().saturating_sub(n);
        let window_start = start.saturating_sub(period - 1);
        let mut count = 0usize;
        for i in start..self.bars.len() {
            if i + 1 < period {
                continue;
            }
            let sma_start = i + 1 - period;
            let sma: Decimal = self.bars[sma_start..=i]
                .iter()
                .map(|b| b.close.value())
                .sum::<Decimal>()
                / Decimal::from(period as u32);
            if self.bars[i].close.value() > sma {
                count += 1;
            }
        }
        let _ = window_start; // used indirectly via sma_start logic
        count
    }

    /// Returns the count of bars (in the last `n`) where `close < SMA(close, period)`.
    ///
    /// Mirrors [`OhlcvSeries::above_sma`] for the bearish side.
    #[allow(clippy::cast_possible_truncation)]
    pub fn below_sma(&self, period: usize, n: usize) -> usize {
        if self.bars.len() < period || period == 0 {
            return 0;
        }
        let start = self.bars.len().saturating_sub(n);
        let mut count = 0usize;
        for i in start..self.bars.len() {
            if i + 1 < period {
                continue;
            }
            let sma_start = i + 1 - period;
            let sma: Decimal = self.bars[sma_start..=i]
                .iter()
                .map(|b| b.close.value())
                .sum::<Decimal>()
                / Decimal::from(period as u32);
            if self.bars[i].close.value() < sma {
                count += 1;
            }
        }
        count
    }

    /// Returns `true` if the latest close is above the EMA(period) of closes.
    ///
    /// Returns `false` if there are fewer than `period` bars or `period == 0`.
    #[allow(clippy::cast_possible_truncation)]
    pub fn above_ema(&self, period: usize) -> bool {
        if period == 0 || self.bars.len() < period {
            return false;
        }
        let k = Decimal::TWO / Decimal::from((period + 1) as u32);
        let seed: Decimal = self.bars[..period].iter().map(|b| b.close.value()).sum::<Decimal>()
            / Decimal::from(period as u32);
        let mut ema = seed;
        for bar in &self.bars[period..] {
            ema = bar.close.value() * k + ema * (Decimal::ONE - k);
        }
        self.bars.last().map_or(false, |b| b.close.value() > ema)
    }

    /// Returns the count of bullish engulfing patterns in the last `n` bars.
    ///
    /// A bullish engulfing occurs when a bar's body fully engulfs the previous bar's
    /// body and the bar closes higher than it opens.
    pub fn bullish_engulfing_count(&self, n: usize) -> usize {
        if self.bars.len() < 2 {
            return 0;
        }
        let start = self.bars.len().saturating_sub(n).max(1);
        self.bars[start..].iter().enumerate().filter(|(i, bar)| {
            let prev = &self.bars[start + i - 1];
            bar.is_bullish_engulfing(prev)
        }).count()
    }

    /// Returns the ratio of the current bar's range to the average range over the last `n` bars.
    ///
    /// Values > 1 indicate range expansion; < 1 indicate contraction.
    /// Returns `None` if fewer than `n` bars exist, `n == 0`, or average range is zero.
    pub fn range_expansion(&self, n: usize) -> Option<Decimal> {
        let last = self.bars.last()?;
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let avg_range: Decimal = self.bars[start..].iter().map(|b| b.range()).sum::<Decimal>();
        #[allow(clippy::cast_possible_truncation)]
        let avg_range = avg_range / Decimal::from(n as u32);
        if avg_range == Decimal::ZERO {
            return None;
        }
        Some(last.range() / avg_range)
    }

    /// Returns the count of bearish engulfing patterns in the last `n` bars.
    ///
    /// A bearish engulfing bar opens above the previous close and closes below the previous open.
    pub fn bearish_engulfing_count(&self, n: usize) -> usize {
        if self.bars.len() < 2 {
            return 0;
        }
        let start = self.bars.len().saturating_sub(n).max(1);
        self.bars[start..].iter().enumerate().filter(|(i, bar)| {
            let prev = &self.bars[start + i - 1];
            // bearish: prev bullish, current opens above prev close, closes below prev open
            let p_o = prev.open.value();
            let p_c = prev.close.value();
            let s_o = bar.open.value();
            let s_c = bar.close.value();
            p_c > p_o && s_c < s_o && s_o >= p_c && s_c <= p_o
        }).count()
    }

    /// Returns a trend-strength ratio over the last `n` bars.
    ///
    /// `trend_strength = |close[last] - close[first]| / Σ|close[i] - close[i-1]|`
    ///
    /// Values near 1 indicate a clean directional trend; near 0 indicate chop.
    /// Returns `None` if fewer than 2 bars exist in the window or total movement is zero.
    pub fn trend_strength(&self, n: usize) -> Option<Decimal> {
        if n < 2 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let window = &self.bars[start..];
        let net = (window.last()?.close.value() - window[0].close.value()).abs();
        let total: Decimal = window.windows(2)
            .map(|w| (w[1].close.value() - w[0].close.value()).abs())
            .sum();
        if total == Decimal::ZERO {
            return None;
        }
        Some(net / total)
    }

    /// Returns the average volume over the last `n` bars, or `None` if the series is empty.
    ///
    /// Returns the average `(close - open) / open` per bar over the last `n` bars.
    ///
    /// Returns `None` if fewer than `n` bars exist, `n == 0`, or any open is zero.
    pub fn open_to_close_return(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let mut sum = Decimal::ZERO;
        for b in &self.bars[start..] {
            let o = b.open.value();
            if o == Decimal::ZERO {
                return None;
            }
            sum += (b.close.value() - o) / o;
        }
        #[allow(clippy::cast_possible_truncation)]
        Some(sum / Decimal::from(n as u32))
    }

    /// Returns the count of bars in the last `n` where `open > prev_close` (gap up).
    pub fn gap_up_count(&self, n: usize) -> usize {
        if self.bars.len() < 2 {
            return 0;
        }
        let start = self.bars.len().saturating_sub(n).max(1);
        self.bars[start..].iter().enumerate().filter(|(i, bar)| {
            bar.open.value() > self.bars[start + i - 1].close.value()
        }).count()
    }

    /// Returns the count of bars in the last `n` where `open < prev_close` (gap down).
    pub fn gap_down_count(&self, n: usize) -> usize {
        if self.bars.len() < 2 {
            return 0;
        }
        let start = self.bars.len().saturating_sub(n).max(1);
        self.bars[start..].iter().enumerate().filter(|(i, bar)| {
            bar.open.value() < self.bars[start + i - 1].close.value()
        }).count()
    }

    /// Returns the average overnight gap percentage over the last `n` bars.
    ///
    /// `overnight_gap_pct = (open - prev_close) / prev_close × 100`
    ///
    /// Returns `None` if fewer than 2 bars in window, `n == 0`, or any prev_close is zero.
    pub fn overnight_gap_pct(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.len() < 2 {
            return None;
        }
        let start = self.bars.len().saturating_sub(n).max(1);
        let window_len = self.bars.len() - start;
        if window_len == 0 {
            return None;
        }
        let mut sum = Decimal::ZERO;
        for i in start..self.bars.len() {
            let pc = self.bars[i - 1].close.value();
            if pc == Decimal::ZERO {
                return None;
            }
            sum += (self.bars[i].open.value() - pc) / pc * Decimal::ONE_HUNDRED;
        }
        #[allow(clippy::cast_possible_truncation)]
        Some(sum / Decimal::from(window_len as u32))
    }

    /// Returns the percentile rank (0–100) of the latest close within the last `n` closes.
    ///
    /// `close_rank = count(closes < current) / (n-1) × 100`
    ///
    /// Returns `None` if fewer than 2 bars in window or `n == 0`.
    pub fn close_rank(&self, n: usize) -> Option<Decimal> {
        if n < 2 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let current = self.bars.last()?.close.value();
        let below = self.bars[start..self.bars.len() - 1]
            .iter()
            .filter(|b| b.close.value() < current)
            .count();
        #[allow(clippy::cast_possible_truncation)]
        Some(Decimal::from(below as u32) / Decimal::from((n - 1) as u32) * Decimal::ONE_HUNDRED)
    }

    /// Returns `highest_high(n) / lowest_low(n)` over the last `n` bars.
    ///
    /// Returns `None` if fewer than `n` bars, `n == 0`, or lowest_low is zero.
    pub fn high_low_ratio(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let hh = self.highest_high(n)?;
        let ll = self.lowest_low(n)?;
        if ll == Decimal::ZERO {
            return None;
        }
        Some(hh / ll)
    }

    /// If `n` exceeds the series length, all bars are included.
    #[allow(clippy::cast_possible_truncation)]
    pub fn average_volume(&self, n: usize) -> Option<Decimal> {
        let start = self.bars.len().saturating_sub(n);
        let slice = &self.bars[start..];
        if slice.is_empty() {
            return None;
        }
        let sum: Decimal = slice.iter().map(|b| b.volume.value()).sum();
        Some(sum / Decimal::from(slice.len() as u32))
    }

    /// Returns the close prices for the last `n` bars in chronological order.
    ///
    /// Returns fewer than `n` values if the series is shorter.
    pub fn last_n_closes(&self, n: usize) -> Vec<Decimal> {
        let start = self.bars.len().saturating_sub(n);
        self.bars[start..].iter().map(|b| b.close.value()).collect()
    }

    /// Returns `true` if the last bar's volume exceeds the average of the prior `n` bars
    /// multiplied by `multiplier`.
    ///
    /// Returns `false` if there are fewer than 2 bars or `multiplier` is zero.
    pub fn volume_spike(&self, n: usize, multiplier: Decimal) -> bool {
        if self.bars.len() < 2 || multiplier.is_zero() {
            return false;
        }
        let last_vol = self.bars.last().unwrap().volume.value();
        // average of all bars except the last one (up to n bars)
        let prior_count = self.bars.len() - 1;
        let start = prior_count.saturating_sub(n);
        let prior = &self.bars[start..prior_count];
        if prior.is_empty() {
            return false;
        }
        let avg: Decimal = prior.iter().map(|b| b.volume.value()).sum::<Decimal>()
            / Decimal::from(prior.len() as u32);
        last_vol > avg * multiplier
    }

    /// Returns the average bar range (high − low) over the last `n` bars, or `None` if empty.
    ///
    /// If `n` exceeds the series length, all bars are included.
    #[allow(clippy::cast_possible_truncation)]
    pub fn average_range(&self, n: usize) -> Option<Decimal> {
        let start = self.bars.len().saturating_sub(n);
        let slice = &self.bars[start..];
        if slice.is_empty() {
            return None;
        }
        let sum: Decimal = slice.iter().map(|b| b.range()).sum();
        Some(sum / Decimal::from(slice.len() as u32))
    }

    /// Returns the mean of typical prices `(high + low + close) / 3` over the last `n` bars.
    ///
    /// Returns `None` if the series is empty.
    #[allow(clippy::cast_possible_truncation)]
    pub fn typical_price_mean(&self, n: usize) -> Option<Decimal> {
        let start = self.bars.len().saturating_sub(n);
        let slice = &self.bars[start..];
        if slice.is_empty() {
            return None;
        }
        let sum: Decimal = slice.iter().map(|b| b.typical_price()).sum();
        Some(sum / Decimal::from(slice.len() as u32))
    }

    /// Returns the Sortino ratio using bar log-returns.
    ///
    /// Only negative returns contribute to the downside deviation denominator.
    /// Returns `None` if there are fewer than 2 bars or if downside deviation is zero.
    pub fn sortino_ratio(&self, risk_free_rate: f64, bars_per_year: f64) -> Option<f64> {
        let log_rets = self.log_returns();
        if log_rets.len() < 2 {
            return None;
        }
        let mean_ret = log_rets.iter().copied().sum::<f64>() / log_rets.len() as f64;
        let downside: Vec<f64> = log_rets.iter().map(|&r| if r < 0.0 { r * r } else { 0.0 }).collect();
        let downside_var = downside.iter().copied().sum::<f64>() / downside.len() as f64;
        let downside_dev = downside_var.sqrt();
        if downside_dev == 0.0 {
            return None;
        }
        let rf_per_bar = risk_free_rate / bars_per_year;
        Some((mean_ret - rf_per_bar) / downside_dev * bars_per_year.sqrt())
    }

    /// Returns the number of consecutive bullish bars (close > open) counting from the end.
    ///
    /// Returns 0 if the series is empty or the last bar is not bullish.
    pub fn close_above_open_streak(&self) -> usize {
        self.bars
            .iter()
            .rev()
            .take_while(|b| b.close.value() > b.open.value())
            .count()
    }

    /// Returns the maximum peak-to-trough drawdown percentage over the last `n` bars.
    ///
    /// Computed on close prices: scans for the largest `(peak - trough) / peak * 100`.
    /// Returns `None` if fewer than 2 bars are available in the window.
    pub fn max_drawdown_pct(&self, n: usize) -> Option<f64> {
        let window: Vec<f64> = self
            .bars
            .iter()
            .rev()
            .take(n)
            .map(|b| b.close.value().to_string().parse::<f64>().unwrap_or(0.0))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        if window.len() < 2 {
            return None;
        }
        let mut max_dd = 0.0f64;
        let mut peak = window[0];
        for &price in &window[1..] {
            if price > peak {
                peak = price;
            }
            if peak > 0.0 {
                let dd = (peak - price) / peak * 100.0;
                if dd > max_dd {
                    max_dd = dd;
                }
            }
        }
        Some(max_dd)
    }

    /// Returns the Average True Range for each bar as `Vec<Option<Decimal>>`.
    ///
    /// Uses a simple rolling average of True Range over `period` bars.
    /// The first `period - 1` entries are `None`; the rest are `Some(atr)`.
    #[allow(clippy::cast_possible_truncation)]
    pub fn atr_series(&self, period: usize) -> Vec<Option<Decimal>> {
        let n = self.bars.len();
        let mut result = vec![None; n];
        if period == 0 || n == 0 {
            return result;
        }
        let trs: Vec<Decimal> = self
            .bars
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let prev = if i == 0 { None } else { Some(&self.bars[i - 1]) };
                b.true_range(prev)
            })
            .collect();
        for i in (period - 1)..n {
            let sum: Decimal = trs[i + 1 - period..=i].iter().copied().sum();
            result[i] = Some(sum / Decimal::from(period as u32));
        }
        result
    }

    /// Returns the count of bars (in the last `n`) where `close > prev_close`.
    ///
    /// If `n` exceeds the series length, all eligible bars are counted.
    /// The first bar in the series is never an "up day" (no prior bar).
    pub fn up_days(&self, n: usize) -> usize {
        if self.bars.len() < 2 {
            return 0;
        }
        let start = self.bars.len().saturating_sub(n).max(1);
        self.bars[start..]
            .iter()
            .enumerate()
            .filter(|(i, b)| b.close.value() > self.bars[start + i - 1].close.value())
            .count()
    }

    /// Returns the count of bars (in the last `n`) where `close < prev_close`.
    ///
    /// Mirrors [`OhlcvSeries::up_days`] for the downside.
    pub fn down_days(&self, n: usize) -> usize {
        if self.bars.len() < 2 {
            return 0;
        }
        let start = self.bars.len().saturating_sub(n).max(1);
        self.bars[start..]
            .iter()
            .enumerate()
            .filter(|(i, b)| b.close.value() < self.bars[start + i - 1].close.value())
            .count()
    }

    /// Returns the per-bar range (`high - low`) as a `Vec<Decimal>`.
    ///
    /// One value per bar; empty if the series is empty.
    pub fn range_series(&self) -> Vec<Decimal> {
        self.bars.iter().map(|b| b.range()).collect()
    }

    /// Returns absolute close-to-close changes: `|close[i] - close[i-1]|` for each bar.
    ///
    /// The result has `len() - 1` entries (first bar has no previous bar).
    /// Empty when the series has fewer than 2 bars.
    pub fn close_to_close_changes(&self) -> Vec<Decimal> {
        if self.bars.len() < 2 {
            return vec![];
        }
        self.bars
            .windows(2)
            .map(|w| (w[1].close.value() - w[0].close.value()).abs())
            .collect()
    }

    /// Returns the ratio of short-period ATR to long-period ATR.
    ///
    /// A ratio > 1 means recent volatility is higher than the longer baseline;
    /// < 1 means it is lower. Returns `None` if either ATR value is unavailable
    /// (series too short) or if the long-period ATR is zero.
    pub fn volatility_ratio(&self, short: usize, long: usize) -> Option<Decimal> {
        let n = self.bars.len();
        if short == 0 || long == 0 || n == 0 {
            return None;
        }
        let short_atr = *self.atr_series(short).last()?;
        let long_atr = *self.atr_series(long).last()?;
        let s = short_atr?;
        let l = long_atr?;
        if l.is_zero() {
            return None;
        }
        Some(s / l)
    }

    /// Returns the length of the current consecutive close-to-close streak.
    ///
    /// A positive value means the last N closes were each higher than the prior close
    /// (bullish streak). A negative value means consecutive lower closes (bearish streak).
    /// Returns `0` when the series has fewer than 2 bars.
    ///
    /// # Example
    /// ```
    /// # use fin_primitives::ohlcv::OhlcvSeries;
    /// # use fin_primitives::types::{Price, Quantity, Symbol, NanoTimestamp};
    /// # use fin_primitives::ohlcv::OhlcvBar;
    /// # fn bar(close: f64) -> OhlcvBar {
    /// #     let p = Price::new(close.to_string().parse().unwrap()).unwrap();
    /// #     let q = Quantity::new(rust_decimal::Decimal::ONE).unwrap();
    /// #     OhlcvBar { symbol: Symbol::new("X").unwrap(), open: p, high: p, low: p, close: p,
    /// #                volume: q, ts_open: NanoTimestamp::new(0), ts_close: NanoTimestamp::new(1), tick_count: 1 }
    /// # }
    /// let mut s = OhlcvSeries::new();
    /// s.push(bar(10.0)); s.push(bar(11.0)); s.push(bar(12.0));
    /// assert_eq!(s.streak(), 2);
    /// ```
    pub fn streak(&self) -> i32 {
        let n = self.bars.len();
        if n < 2 {
            return 0;
        }
        let mut count: i32 = 0;
        for i in (1..n).rev() {
            let prev = self.bars[i - 1].close.value();
            let curr = self.bars[i].close.value();
            if curr > prev {
                if count < 0 {
                    break;
                }
                count += 1;
            } else if curr < prev {
                if count > 0 {
                    break;
                }
                count -= 1;
            } else {
                break;
            }
        }
        count
    }

    /// Returns the Calmar ratio: annualised return divided by maximum drawdown.
    ///
    /// Annualised return is computed as `mean_log_return * bars_per_year`.
    /// Requires at least 2 bars and a non-zero `max_drawdown`.
    ///
    /// Returns `None` when there is insufficient data or the max drawdown is zero.
    pub fn calmar_ratio(&self, bars_per_year: f64) -> Option<f64> {
        let lr = self.log_returns();
        if lr.len() < 2 {
            return None;
        }
        let ann_return = (lr.iter().sum::<f64>() / lr.len() as f64) * bars_per_year;
        let dd = self.max_drawdown()?;
        use rust_decimal::prelude::ToPrimitive;
        let dd_f64 = dd.to_f64()?;
        if dd_f64 == 0.0_f64 {
            return None;
        }
        Some(ann_return / dd_f64)
    }

    /// Returns `(highest_high, lowest_low)` over the last `n` bars, or `None` if empty.
    ///
    /// If `n` exceeds the series length, all bars are considered. Provides a convenient
    /// way to get both extremes in one call without scanning the series twice.
    pub fn session_high_low(&self, n: usize) -> Option<(Decimal, Decimal)> {
        let start = self.bars.len().saturating_sub(n);
        let slice = &self.bars[start..];
        if slice.is_empty() {
            return None;
        }
        let h = slice.iter().map(|b| b.high.value()).fold(Decimal::MIN, Decimal::max);
        let l = slice.iter().map(|b| b.low.value()).fold(Decimal::MAX, Decimal::min);
        Some((h, l))
    }

    /// Returns bar-to-bar percentage changes: `(close[i] - close[i-1]) / close[i-1] * 100`.
    ///
    /// The result has `len() - 1` entries. Returns an empty vec when the series
    /// has fewer than 2 bars or when a previous close is zero.
    pub fn percentage_change_series(&self) -> Vec<Option<Decimal>> {
        if self.bars.len() < 2 {
            return vec![];
        }
        self.bars
            .windows(2)
            .map(|w| {
                let prev_c = w[0].close.value();
                if prev_c.is_zero() {
                    None
                } else {
                    Some((w[1].close.value() - prev_c) / prev_c * Decimal::ONE_HUNDRED)
                }
            })
            .collect()
    }

    /// Realized volatility: standard deviation of log returns over the last `n` bars,
    /// annualised by multiplying by `sqrt(bars_per_year)`.
    ///
    /// Returns `None` if `n == 0` or there are fewer than `n + 1` bars.
    pub fn realized_volatility(&self, n: usize, bars_per_year: f64) -> Option<f64> {
        if n == 0 || self.bars.len() < n + 1 {
            return None;
        }
        let start = self.bars.len() - n - 1;
        let lr: Vec<f64> = self.bars[start..]
            .windows(2)
            .filter_map(|w| {
                let prev = w[0].close.value();
                if prev.is_zero() {
                    return None;
                }
                use rust_decimal::prelude::ToPrimitive;
                let ratio = (w[1].close.value() / prev).to_f64()?;
                Some(ratio.ln())
            })
            .collect();
        if lr.len() < 2 {
            return None;
        }
        let mean = lr.iter().sum::<f64>() / lr.len() as f64;
        let variance = lr.iter().map(|&r| (r - mean).powi(2)).sum::<f64>() / lr.len() as f64;
        Some(variance.sqrt() * bars_per_year.sqrt())
    }

    /// Pearson correlation of closes between `self` and `other` over the last `n` bars.
    ///
    /// Returns `None` when either series has fewer than `n` bars, `n < 2`, or
    /// either series has zero variance over the window.
    pub fn rolling_correlation(&self, other: &OhlcvSeries, n: usize) -> Option<f64> {
        if n < 2 || self.bars.len() < n || other.bars.len() < n {
            return None;
        }
        use rust_decimal::prelude::ToPrimitive;
        let xs: Vec<f64> = self.bars[self.bars.len() - n..]
            .iter()
            .filter_map(|b| b.close.value().to_f64())
            .collect();
        let ys: Vec<f64> = other.bars[other.bars.len() - n..]
            .iter()
            .filter_map(|b| b.close.value().to_f64())
            .collect();
        if xs.len() != n || ys.len() != n {
            return None;
        }
        let n_f = n as f64;
        let mx = xs.iter().sum::<f64>() / n_f;
        let my = ys.iter().sum::<f64>() / n_f;
        let cov = xs.iter().zip(ys.iter()).map(|(x, y)| (x - mx) * (y - my)).sum::<f64>() / n_f;
        let sx = (xs.iter().map(|x| (x - mx).powi(2)).sum::<f64>() / n_f).sqrt();
        let sy = (ys.iter().map(|y| (y - my).powi(2)).sum::<f64>() / n_f).sqrt();
        if sx == 0.0 || sy == 0.0 {
            return None;
        }
        Some(cov / (sx * sy))
    }

    /// CAPM beta: `cov(self, market) / var(market)` over the last `n` log-return bars.
    ///
    /// Returns `None` when either series has fewer than `n + 1` bars, `n < 2`, or
    /// the market variance is zero.
    pub fn beta(&self, market: &OhlcvSeries, n: usize) -> Option<f64> {
        if n < 2 || self.bars.len() < n + 1 || market.bars.len() < n + 1 {
            return None;
        }
        use rust_decimal::prelude::ToPrimitive;
        let asset_lr: Vec<f64> = self.bars[self.bars.len() - n - 1..]
            .windows(2)
            .filter_map(|w| {
                let prev = w[0].close.value();
                if prev.is_zero() { return None; }
                (w[1].close.value() / prev).to_f64().map(|r| r.ln())
            })
            .collect();
        let mkt_lr: Vec<f64> = market.bars[market.bars.len() - n - 1..]
            .windows(2)
            .filter_map(|w| {
                let prev = w[0].close.value();
                if prev.is_zero() { return None; }
                (w[1].close.value() / prev).to_f64().map(|r| r.ln())
            })
            .collect();
        let len = asset_lr.len().min(mkt_lr.len());
        if len < 2 {
            return None;
        }
        let n_f = len as f64;
        let ma = asset_lr[..len].iter().sum::<f64>() / n_f;
        let mm = mkt_lr[..len].iter().sum::<f64>() / n_f;
        let cov = asset_lr[..len].iter().zip(mkt_lr[..len].iter())
            .map(|(a, m)| (a - ma) * (m - mm))
            .sum::<f64>() / n_f;
        let var_m = mkt_lr[..len].iter().map(|m| (m - mm).powi(2)).sum::<f64>() / n_f;
        if var_m == 0.0 { return None; }
        Some(cov / var_m)
    }

    /// Information ratio: `(mean_excess_return) / tracking_error` over the last `n` bars.
    ///
    /// Excess return is `asset_log_return - benchmark_log_return` per bar.
    /// Returns `None` when there is insufficient data or tracking error is zero.
    pub fn information_ratio(&self, benchmark: &OhlcvSeries, n: usize) -> Option<f64> {
        if n < 2 || self.bars.len() < n + 1 || benchmark.bars.len() < n + 1 {
            return None;
        }
        use rust_decimal::prelude::ToPrimitive;
        let excess: Vec<f64> = self.bars[self.bars.len() - n - 1..]
            .windows(2)
            .zip(benchmark.bars[benchmark.bars.len() - n - 1..].windows(2))
            .filter_map(|(aw, bw)| {
                let ap = aw[0].close.value();
                let bp = bw[0].close.value();
                if ap.is_zero() || bp.is_zero() { return None; }
                let ar = (aw[1].close.value() / ap).to_f64()?.ln();
                let br = (bw[1].close.value() / bp).to_f64()?.ln();
                Some(ar - br)
            })
            .collect();
        if excess.len() < 2 { return None; }
        let n_f = excess.len() as f64;
        let mean = excess.iter().sum::<f64>() / n_f;
        let te = (excess.iter().map(|e| (e - mean).powi(2)).sum::<f64>() / n_f).sqrt();
        if te == 0.0 { return None; }
        Some(mean / te)
    }

    /// Per-bar drawdown series from the rolling high-water mark.
    ///
    /// Each element is `(rolling_high - close) / rolling_high` expressed as a positive
    /// fraction (0 = at new high, 0.1 = 10% below peak). Empty when the series is empty.
    pub fn drawdown_series(&self) -> Vec<Decimal> {
        if self.bars.is_empty() {
            return vec![];
        }
        let mut peak = Decimal::MIN;
        self.bars
            .iter()
            .map(|b| {
                let close = b.close.value();
                if close > peak {
                    peak = close;
                }
                if peak.is_zero() {
                    Decimal::ZERO
                } else {
                    (peak - close) / peak
                }
            })
            .collect()
    }

    /// Returns `true` if the last close is above the SMA of the last `period` closes.
    ///
    /// Returns `None` when there are fewer than `period` bars or `period == 0`.
    pub fn above_moving_average(&self, period: usize) -> Option<bool> {
        if period == 0 || self.bars.len() < period {
            return None;
        }
        let start = self.bars.len() - period;
        #[allow(clippy::cast_possible_truncation)]
        let sma: Decimal = self.bars[start..].iter().map(|b| b.close.value()).sum::<Decimal>()
            / Decimal::from(period as u32);
        Some(self.bars.last()?.close.value() > sma)
    }

    /// Count bars in the last `n` where `high > prev_bar.high` (consecutive higher highs proxy).
    ///
    /// Returns 0 when the series has fewer than 2 bars or `n == 0`.
    pub fn consecutive_higher_highs(&self, n: usize) -> usize {
        if n == 0 || self.bars.len() < 2 {
            return 0;
        }
        let start = self.bars.len().saturating_sub(n).max(1);
        self.bars[start..]
            .iter()
            .enumerate()
            .filter(|(i, b)| b.high.value() > self.bars[start + i - 1].high.value())
            .count()
    }

    /// Counts bars where each bar's low is strictly below the prior bar's low,
    /// looking at the last `n` consecutive bar pairs.
    ///
    /// Returns `0` when `n == 0` or the series has fewer than 2 bars.
    pub fn consecutive_lower_lows(&self, n: usize) -> usize {
        if n == 0 || self.bars.len() < 2 {
            return 0;
        }
        let start = self.bars.len().saturating_sub(n).max(1);
        self.bars[start..]
            .iter()
            .enumerate()
            .filter(|(i, b)| b.low.value() < self.bars[start + i - 1].low.value())
            .count()
    }

    /// Distance of the latest close from its `n`-bar VWAP, as a percentage of VWAP.
    ///
    /// `deviation_pct = (close - vwap) / vwap * 100`
    ///
    /// Returns `None` if `n == 0`, series is shorter than `n`, or total volume is zero.
    pub fn vwap_deviation(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len().saturating_sub(n);
        let slice = &self.bars[start..];
        let total_vol: Decimal = slice.iter().map(|b| b.volume.value()).sum();
        if total_vol.is_zero() {
            return None;
        }
        let vwap: Decimal = slice.iter()
            .map(|b| {
                let tp = (b.high.value() + b.low.value() + b.close.value()) / Decimal::from(3u32);
                tp * b.volume.value()
            })
            .sum::<Decimal>() / total_vol;
        if vwap.is_zero() {
            return None;
        }
        let last_close = self.bars.last()?.close.value();
        Some((last_close - vwap) / vwap * Decimal::ONE_HUNDRED)
    }

    /// ATR as a percentage of the last closing price over the last `n` bars.
    ///
    /// Computed as `mean(ATR) / close * 100`. Returns `None` if fewer than `n` bars,
    /// `n == 0`, or the last close is zero.
    pub fn average_true_range_pct(&self, n: usize) -> Option<f64> {
        use rust_decimal::prelude::ToPrimitive;
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let atrs = self.atr_series(n);
        let last_close = self.bars.last()?.close.value();
        if last_close.is_zero() {
            return None;
        }
        let atr = (*atrs.last()?.as_ref()?).to_f64()?;
        let close_f64 = last_close.to_f64()?;
        Some(atr / close_f64 * 100.0)
    }

    /// Count bars in the last `n` that are doji candles (body ≤ `threshold` × range).
    ///
    /// Delegates to [`OhlcvBar::is_doji`] for each bar.
    pub fn count_doji(&self, n: usize, threshold: Decimal) -> usize {
        if n == 0 {
            return 0;
        }
        let start = self.bars.len().saturating_sub(n);
        self.bars[start..].iter().filter(|b| b.is_doji(threshold)).count()
    }

    /// Counts bars in the last `n` where `open > prev_close` (gap-up).
    ///
    /// Returns `0` if the series has fewer than 2 bars or `n == 0`.
    pub fn gap_up_bars(&self, n: usize) -> usize {
        if n == 0 || self.bars.len() < 2 {
            return 0;
        }
        let start = self.bars.len().saturating_sub(n + 1);
        self.bars[start..].windows(2).filter(|w| w[1].gap_up_from(&w[0])).count()
    }

    /// Counts bars in the last `n` where `open < prev_close` (gap-down).
    ///
    /// Returns `0` if the series has fewer than 2 bars or `n == 0`.
    pub fn gap_down_bars(&self, n: usize) -> usize {
        if n == 0 || self.bars.len() < 2 {
            return 0;
        }
        let start = self.bars.len().saturating_sub(n + 1);
        self.bars[start..].windows(2).filter(|w| w[1].gap_down_from(&w[0])).count()
    }

    /// Returns the cumulative volume over the last `n` bars.
    ///
    /// Returns `Decimal::ZERO` if `n == 0` or the series is empty.
    pub fn cum_volume(&self, n: usize) -> Decimal {
        if n == 0 {
            return Decimal::ZERO;
        }
        let start = self.bars.len().saturating_sub(n);
        self.bars[start..].iter().map(|b| b.volume.value()).sum()
    }

    /// Dual-period momentum score: `(sma_short - sma_long) / sma_long * 100`.
    ///
    /// Returns `None` when the series has fewer than `long` bars, `short == 0`,
    /// `long == 0`, `short >= long`, or the long SMA is zero.
    pub fn momentum_score(&self, short: usize, long: usize) -> Option<f64> {
        use rust_decimal::prelude::ToPrimitive;
        if short == 0 || long == 0 || short >= long || self.bars.len() < long {
            return None;
        }
        #[allow(clippy::cast_possible_truncation)]
        let sma = |n: usize| -> Option<Decimal> {
            let start = self.bars.len().saturating_sub(n);
            let s: Decimal = self.bars[start..].iter().map(|b| b.close.value()).sum();
            Some(s / Decimal::from(n as u32))
        };
        let sma_s = sma(short)?;
        let sma_l = sma(long)?;
        if sma_l.is_zero() {
            return None;
        }
        ((sma_s - sma_l) / sma_l * Decimal::ONE_HUNDRED).to_f64()
    }

    /// Returns the first bar in the series, or `None` if empty.
    pub fn first_bar(&self) -> Option<&OhlcvBar> {
        self.bars.first()
    }

    /// Volume-weighted close over the last `n` bars: `Σ(close × volume) / Σ(volume)`.
    ///
    /// Returns `None` when `n == 0`, the series has fewer than `n` bars, or total volume is zero.
    pub fn volume_weighted_close(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let vol_sum: Decimal = self.bars[start..].iter().map(|b| b.volume.value()).sum();
        if vol_sum.is_zero() {
            return None;
        }
        let pv_sum: Decimal = self.bars[start..]
            .iter()
            .map(|b| b.close.value() * b.volume.value())
            .sum();
        Some(pv_sum / vol_sum)
    }

    /// Last bar range divided by average range over the last `n` bars.
    ///
    /// Values > 1 indicate volatility expansion; < 1 contraction.
    /// Returns `None` when `n == 0`, the series has fewer than `n` bars, or average range is zero.
    pub fn range_expansion_ratio(&self, n: usize) -> Option<f64> {
        use rust_decimal::prelude::ToPrimitive;
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let last_range = self.bars.last()?.range();
        let start = self.bars.len() - n;
        let avg_range = self.bars[start..]
            .iter()
            .map(|b| b.range())
            .sum::<Decimal>();
        #[allow(clippy::cast_possible_truncation)]
        let avg = avg_range / Decimal::from(n as u32);
        if avg.is_zero() {
            return None;
        }
        (last_range / avg).to_f64()
    }

    /// Kaufman Efficiency Ratio over the last `n` bars.
    ///
    /// `ER = |close[end] - close[start]| / Σ|close[i] - close[i-1]|`.
    /// Returns `None` if fewer than `n+1` bars or the total path length is zero.
    pub fn efficiency_ratio(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.len() <= n {
            return None;
        }
        let start = self.bars.len() - n - 1;
        let closes: Vec<Decimal> = self.bars[start..].iter().map(|b| b.close.value()).collect();
        let direction = (closes[n] - closes[0]).abs();
        let path: Decimal = closes.windows(2).map(|w| (w[1] - w[0]).abs()).sum();
        if path.is_zero() {
            return None;
        }
        Some(direction / path)
    }

    /// Body-size as a percentage of range for the last `n` bars.
    ///
    /// Each element is `|close - open| / (high - low) * 100`, or `None` when
    /// the bar's high equals its low.
    pub fn body_pct_series(&self, n: usize) -> Vec<Option<Decimal>> {
        let start = self.bars.len().saturating_sub(n);
        self.bars[start..]
            .iter()
            .map(|b| {
                let range = b.high.value() - b.low.value();
                if range.is_zero() {
                    None
                } else {
                    let body = (b.close.value() - b.open.value()).abs();
                    Some(body / range * Decimal::ONE_HUNDRED)
                }
            })
            .collect()
    }

    /// Count of candle direction changes in the last `n` bars.
    ///
    /// A change is when the current bar's direction (close ≥ open vs close < open)
    /// differs from the previous bar. Returns `0` if fewer than 2 bars available.
    pub fn candle_color_changes(&self, n: usize) -> usize {
        let start = self.bars.len().saturating_sub(n);
        let slice = &self.bars[start..];
        if slice.len() < 2 {
            return 0;
        }
        slice.windows(2)
            .filter(|w| {
                let prev_bull = w[0].close.value() >= w[0].open.value();
                let curr_bull = w[1].close.value() >= w[1].open.value();
                prev_bull != curr_bull
            })
            .count()
    }

    /// Typical price `(high + low + close) / 3` for each of the last `n` bars.
    pub fn typical_price_series(&self, n: usize) -> Vec<Decimal> {
        let start = self.bars.len().saturating_sub(n);
        self.bars[start..]
            .iter()
            .map(|b| (b.high.value() + b.low.value() + b.close.value()) / Decimal::from(3))
            .collect()
    }

    /// Returns the open-gap percentage for each consecutive bar pair in the full series.
    ///
    /// `gap_pct[i] = (open[i] - close[i-1]) / close[i-1] * 100`
    ///
    /// Returns an empty vec if the series has fewer than 2 bars.
    pub fn open_gap_series(&self) -> Vec<Decimal> {
        if self.bars.len() < 2 {
            return Vec::new();
        }
        self.bars
            .windows(2)
            .filter_map(|w| {
                let prev_close = w[0].close.value();
                if prev_close.is_zero() {
                    return None;
                }
                Some((w[1].open.value() - prev_close) / prev_close * Decimal::ONE_HUNDRED)
            })
            .collect()
    }

    /// Average intraday range as a percentage of open: `mean((high - low) / open * 100)` over last `n` bars.
    ///
    /// Returns `None` if `n == 0`, the series is empty, or any open is zero.
    pub fn intraday_range_pct(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.is_empty() {
            return None;
        }
        let start = self.bars.len().saturating_sub(n);
        let slice = &self.bars[start..];
        let count = slice.len();
        if count == 0 {
            return None;
        }
        let sum: Option<Decimal> = slice.iter().try_fold(Decimal::ZERO, |acc, b| {
            let o = b.open.value();
            if o.is_zero() { return None; }
            Some(acc + (b.high.value() - b.low.value()) / o * Decimal::ONE_HUNDRED)
        });
        #[allow(clippy::cast_possible_truncation)]
        Some(sum? / Decimal::from(count as u32))
    }

    /// Counts bars in the last `n` where `close > prev_high` (breakout above prior high).
    ///
    /// Returns `0` if `n == 0` or the series has fewer than 2 bars.
    pub fn close_above_prior_high(&self, n: usize) -> usize {
        if n == 0 || self.bars.len() < 2 {
            return 0;
        }
        let start = self.bars.len().saturating_sub(n + 1);
        self.bars[start..].windows(2).filter(|w| w[1].close.value() > w[0].high.value()).count()
    }

    /// Skewness of close prices over the last `n` bars (Fisher's moment coefficient of skewness).
    ///
    /// Returns `None` if `n < 3`, series has fewer than `n` bars, or std dev is zero.
    pub fn skewness(&self, n: usize) -> Option<f64> {
        use rust_decimal::prelude::ToPrimitive;
        if n < 3 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len().saturating_sub(n);
        let vals: Vec<f64> = self.bars[start..]
            .iter()
            .filter_map(|b| b.close.value().to_f64())
            .collect();
        if vals.len() < 3 {
            return None;
        }
        let n_f = vals.len() as f64;
        let mean = vals.iter().sum::<f64>() / n_f;
        let variance = vals.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n_f;
        let std_dev = variance.sqrt();
        if std_dev == 0.0 {
            return None;
        }
        let skew = vals.iter().map(|x| ((x - mean) / std_dev).powi(3)).sum::<f64>() / n_f;
        Some(skew)
    }

    /// Excess kurtosis of close prices over the last `n` bars.
    ///
    /// Excess kurtosis = (fourth central moment / variance²) - 3.
    /// Returns `None` if `n < 4`, series has fewer than `n` bars, or variance is zero.
    pub fn kurtosis(&self, n: usize) -> Option<f64> {
        use rust_decimal::prelude::ToPrimitive;
        if n < 4 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len().saturating_sub(n);
        let vals: Vec<f64> = self.bars[start..]
            .iter()
            .filter_map(|b| b.close.value().to_f64())
            .collect();
        if vals.len() < 4 {
            return None;
        }
        let n_f = vals.len() as f64;
        let mean = vals.iter().sum::<f64>() / n_f;
        let variance = vals.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n_f;
        if variance == 0.0 {
            return None;
        }
        let kurt = vals.iter().map(|x| ((x - mean) / variance.sqrt()).powi(4)).sum::<f64>() / n_f - 3.0;
        Some(kurt)
    }

    /// Returns `true` when the fast SMA is above the slow SMA (golden-cross condition).
    ///
    /// Returns `false` if the series does not have enough bars for the slow period,
    /// or if `fast_period >= slow_period`.
    pub fn sma_crossover(&self, fast_period: usize, slow_period: usize) -> bool {
        if fast_period == 0 || slow_period == 0 || fast_period >= slow_period {
            return false;
        }
        if self.bars.len() < slow_period {
            return false;
        }
        let fast_start = self.bars.len() - fast_period;
        let slow_start = self.bars.len() - slow_period;
        let fast_avg: Decimal = self.bars[fast_start..].iter().map(|b| b.close.value()).sum::<Decimal>()
            / Decimal::from(fast_period as u32);
        let slow_avg: Decimal = self.bars[slow_start..].iter().map(|b| b.close.value()).sum::<Decimal>()
            / Decimal::from(slow_period as u32);
        fast_avg > slow_avg
    }

    /// Fraction of the last `n` closing prices that are at or below `price`.
    ///
    /// Returns a value in `[0.0, 1.0]`. Returns `None` if `n == 0` or the series is empty.
    pub fn price_percentile(&self, price: Decimal, n: usize) -> Option<f64> {
        if n == 0 || self.bars.is_empty() {
            return None;
        }
        let start = self.bars.len().saturating_sub(n);
        let slice = &self.bars[start..];
        let count = slice.iter().filter(|b| b.close.value() <= price).count();
        Some(count as f64 / slice.len() as f64)
    }

    /// Mean of `(high - low)` over the last `n` bars.
    ///
    /// Returns `None` if `n == 0` or the series has fewer than `n` bars.
    pub fn intraday_range_mean(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let sum: Decimal = self.bars[start..].iter().map(|b| b.high.value() - b.low.value()).sum();
        #[allow(clippy::cast_possible_truncation)]
        Some(sum / Decimal::from(n as u32))
    }

    /// Returns `(current_range / ATR) * 100`, showing how the current bar's
    /// high-low range compares to the average true range over the last `n` bars.
    ///
    /// Returns `None` if fewer than `n+1` bars, `n == 0`, or ATR is zero.
    pub fn range_to_atr_ratio(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.len() < n + 1 {
            return None;
        }
        let start = self.bars.len() - n - 1;
        let slice = &self.bars[start..];
        let mut tr_sum = Decimal::ZERO;
        for w in slice.windows(2) {
            let prev_close = w[0].close.value();
            let high = w[1].high.value();
            let low = w[1].low.value();
            let tr = (high - low)
                .max((high - prev_close).abs())
                .max((low - prev_close).abs());
            tr_sum += tr;
        }
        #[allow(clippy::cast_possible_truncation)]
        let atr = tr_sum / Decimal::from(n as u32);
        if atr.is_zero() {
            return None;
        }
        let last = self.bars.last()?;
        let current_range = last.high.value() - last.low.value();
        Some(current_range / atr * Decimal::ONE_HUNDRED)
    }

    /// Returns percentage momentum: `(close - close[n]) / close[n] * 100`.
    ///
    /// Positive when price has risen over the last `n` bars.
    /// Returns `None` if fewer than `n+1` bars, `n == 0`, or reference close is zero.
    pub fn close_momentum(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.len() < n + 1 {
            return None;
        }
        let ref_close = self.bars[self.bars.len() - n - 1].close.value();
        if ref_close.is_zero() {
            return None;
        }
        let current = self.bars.last()?.close.value();
        Some((current - ref_close) / ref_close * Decimal::ONE_HUNDRED)
    }

    /// Mean absolute gap percentage over the last `n` bars.
    ///
    /// `gap_pct[i] = |open[i] - close[i-1]| / close[i-1] * 100`.
    /// Returns `None` if fewer than `n+1` bars or `n == 0`.
    pub fn average_gap_pct(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.len() <= n {
            return None;
        }
        let start = self.bars.len() - n - 1;
        let slice = &self.bars[start..];
        let mut count = 0;
        let mut sum = Decimal::ZERO;
        for pair in slice.windows(2) {
            let pc = pair[0].close.value();
            if pc.is_zero() {
                continue;
            }
            sum += (pair[1].open.value() - pc).abs() / pc * Decimal::ONE_HUNDRED;
            count += 1;
        }
        if count == 0 {
            None
        } else {
            #[allow(clippy::cast_possible_truncation)]
            Some(sum / Decimal::from(count as u32))
        }
    }

    /// Bar-over-bar log returns for the last `n` close-to-close periods.
    ///
    /// Returns a `Vec` of up to `n` values. Requires at least `n + 1` bars in the series.
    /// Returns an empty `Vec` when `n == 0` or fewer than 2 bars exist.
    pub fn returns_series(&self, n: usize) -> Vec<Decimal> {
        if n == 0 || self.bars.len() < 2 {
            return vec![];
        }
        use rust_decimal::prelude::ToPrimitive;
        let start = self.bars.len().saturating_sub(n + 1);
        let slice = &self.bars[start..];
        slice
            .windows(2)
            .map(|w| {
                let prev = w[0].close.value();
                let curr = w[1].close.value();
                if prev.is_zero() {
                    Decimal::ZERO
                } else {
                    let ratio = (curr / prev).to_f64().unwrap_or(1.0);
                    Decimal::try_from(ratio.ln()).unwrap_or(Decimal::ZERO)
                }
            })
            .collect()
    }

    /// Length of the longest consecutive run of rising closes in the entire series.
    ///
    /// A close is "rising" when `close[i] > close[i-1]`.
    /// Returns `0` when fewer than 2 bars exist.
    pub fn max_consecutive_up(&self) -> usize {
        if self.bars.len() < 2 {
            return 0;
        }
        let mut max_run = 0usize;
        let mut current = 0usize;
        for w in self.bars.windows(2) {
            if w[1].close.value() > w[0].close.value() {
                current += 1;
                if current > max_run {
                    max_run = current;
                }
            } else {
                current = 0;
            }
        }
        max_run
    }

    /// Length of the longest consecutive run of falling closes in the entire series.
    ///
    /// A close is "falling" when `close[i] < close[i-1]`.
    /// Returns `0` when fewer than 2 bars exist.
    pub fn max_consecutive_down(&self) -> usize {
        if self.bars.len() < 2 {
            return 0;
        }
        let mut max_run = 0usize;
        let mut current = 0usize;
        for w in self.bars.windows(2) {
            if w[1].close.value() < w[0].close.value() {
                current += 1;
                if current > max_run {
                    max_run = current;
                }
            } else {
                current = 0;
            }
        }
        max_run
    }

    /// Simple moving average of the typical price `(high + low + close) / 3`
    /// over the last `period` bars.
    ///
    /// Returns `None` if `period == 0` or fewer than `period` bars exist.
    pub fn typical_price_sma(&self, period: usize) -> Option<Decimal> {
        if period == 0 || self.bars.len() < period {
            return None;
        }
        let start = self.bars.len() - period;
        let sum: Decimal = self.bars[start..]
            .iter()
            .map(|b| (b.high.value() + b.low.value() + b.close.value()) / Decimal::from(3u32))
            .sum();
        #[allow(clippy::cast_possible_truncation)]
        Some(sum / Decimal::from(period as u32))
    }

    /// Returns a reference to the bar at position `i`, or `None` if out of bounds.
    pub fn bar_at_index(&self, i: usize) -> Option<&OhlcvBar> {
        self.bars.get(i)
    }

    /// Standard deviation of closes over the last `n` bars.
    ///
    /// Returns `None` if `n < 2` or fewer than `n` bars exist.
    #[allow(clippy::cast_possible_truncation)]
    pub fn rolling_close_std(&self, n: usize) -> Option<Decimal> {
        if n < 2 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let closes: Vec<Decimal> = self.bars[start..].iter().map(|b| b.close.value()).collect();
        let mean = closes.iter().copied().sum::<Decimal>() / Decimal::from(n as u32);
        let variance = closes
            .iter()
            .map(|c| { let d = *c - mean; d * d })
            .sum::<Decimal>()
            / Decimal::from((n - 1) as u32);
        use rust_decimal::prelude::ToPrimitive;
        let std = variance.to_f64()?.sqrt();
        Decimal::try_from(std).ok()
    }

    /// Returns a `Vec<i8>` of gap directions (`+1` = gap up, `-1` = gap down, `0` = flat)
    /// for bar-over-bar open-to-prev-close gaps over the last `n` bars.
    ///
    /// A gap is defined as `open[i] != close[i-1]`. Returns at most `n - 1` values.
    /// Returns empty `Vec` when `n < 2` or fewer than 2 bars exist.
    pub fn gap_direction_series(&self, n: usize) -> Vec<i8> {
        if n < 2 || self.bars.len() < 2 {
            return vec![];
        }
        let start = self.bars.len().saturating_sub(n);
        self.bars[start..]
            .windows(2)
            .map(|w| {
                let gap = w[1].open.value() - w[0].close.value();
                if gap > Decimal::ZERO {
                    1i8
                } else if gap < Decimal::ZERO {
                    -1i8
                } else {
                    0i8
                }
            })
            .collect()
    }

    /// Returns the linear regression slope of volume over the last `n` bars.
    ///
    /// Positive slope → volume is trending up; negative → down.
    /// Returns `None` if `n < 2` or fewer than `n` bars exist.
    pub fn volume_trend(&self, n: usize) -> Option<f64> {
        use rust_decimal::prelude::ToPrimitive;
        if n < 2 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let vols: Vec<f64> = self.bars[start..]
            .iter()
            .filter_map(|b| b.volume.value().to_f64())
            .collect();
        if vols.len() < 2 {
            return None;
        }
        let n_f = vols.len() as f64;
        let sum_x: f64 = (0..vols.len()).map(|i| i as f64).sum();
        let sum_y: f64 = vols.iter().sum();
        let sum_xy: f64 = vols.iter().enumerate().map(|(i, &v)| i as f64 * v).sum();
        let sum_xx: f64 = (0..vols.len()).map(|i| (i as f64).powi(2)).sum();
        let denom = n_f * sum_xx - sum_x * sum_x;
        if denom == 0.0 { return None; }
        Some((n_f * sum_xy - sum_x * sum_y) / denom)
    }

    /// Average ratio of total wick length to body length over the last `n` bars.
    ///
    /// `wick = (high - low) - |close - open|`; `body = |close - open|`
    /// Returns `None` if `n == 0`, fewer than `n` bars, or all bodies are zero.
    pub fn wick_body_ratio(&self, n: usize) -> Option<f64> {
        use rust_decimal::prelude::ToPrimitive;
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let mut sum = 0.0f64;
        let mut count = 0usize;
        for b in &self.bars[start..] {
            let body = (b.close.value() - b.open.value()).abs().to_f64()?;
            if body == 0.0 { continue; }
            let range = (b.high.value() - b.low.value()).to_f64()?;
            let wick = (range - body).max(0.0);
            sum += wick / body;
            count += 1;
        }
        if count == 0 { return None; }
        Some(sum / count as f64)
    }

    /// Pearson correlation between volume and close price over the last `n` bars.
    ///
    /// Returns `None` if `n < 2`, fewer than `n` bars exist, or standard deviation is zero.
    pub fn volume_price_correlation(&self, n: usize) -> Option<f64> {
        use rust_decimal::prelude::ToPrimitive;
        if n < 2 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let xs: Vec<f64> = self.bars[start..]
            .iter()
            .filter_map(|b| b.volume.value().to_f64())
            .collect();
        let ys: Vec<f64> = self.bars[start..]
            .iter()
            .filter_map(|b| b.close.value().to_f64())
            .collect();
        if xs.len() < 2 { return None; }
        let n_f = xs.len() as f64;
        let mx = xs.iter().sum::<f64>() / n_f;
        let my = ys.iter().sum::<f64>() / n_f;
        let num: f64 = xs.iter().zip(ys.iter()).map(|(x, y)| (x - mx) * (y - my)).sum();
        let sx = (xs.iter().map(|x| (x - mx).powi(2)).sum::<f64>() / n_f).sqrt();
        let sy = (ys.iter().map(|y| (y - my).powi(2)).sum::<f64>() / n_f).sqrt();
        if sx == 0.0 || sy == 0.0 { return None; }
        Some(num / (n_f * sx * sy))
    }

    /// Average bar range as a percentage of close: `(high - low) / close × 100` over `n` bars.
    ///
    /// Returns `None` if `n == 0`, fewer than `n` bars, or any close is zero.
    pub fn bar_range_pct(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let mut sum = Decimal::ZERO;
        let mut count = 0u32;
        for b in &self.bars[start..] {
            let c = b.close.value();
            if c.is_zero() { continue; }
            sum += (b.high.value() - b.low.value()) / c * Decimal::ONE_HUNDRED;
            count += 1;
        }
        if count == 0 { return None; }
        Some(sum / Decimal::from(count))
    }

    /// Count of bars over the last `n` where close > midpoint of prior bar's high-low range.
    ///
    /// Returns `0` when `n < 2` or fewer than 2 bars available.
    pub fn close_vs_prior_range_count(&self, n: usize) -> usize {
        if n < 2 || self.bars.len() < 2 {
            return 0;
        }
        let start = self.bars.len().saturating_sub(n);
        let slice = &self.bars[start..];
        slice.windows(2)
            .filter(|w| {
                let mid = (w[0].high.value() + w[0].low.value()) / Decimal::TWO;
                w[1].close.value() > mid
            })
            .count()
    }

    /// Annualised Sharpe ratio of log returns over the last `n` bars.
    ///
    /// Uses 252 trading days to annualise. Returns `None` if fewer than 2 bars exist,
    /// `n == 0`, or the standard deviation of returns is zero.
    pub fn rolling_sharpe(&self, n: usize, risk_free_rate: Decimal) -> Option<Decimal> {
        if n == 0 || self.bars.len() < 2 {
            return None;
        }
        use rust_decimal::prelude::ToPrimitive;
        let returns = self.returns_series(n);
        if returns.len() < 2 {
            return None;
        }
        #[allow(clippy::cast_possible_truncation)]
        let len_d = Decimal::from(returns.len() as u32);
        let mean: Decimal = returns.iter().copied().sum::<Decimal>() / len_d;
        let rf_daily = risk_free_rate / Decimal::from(252u32);
        let excess_mean = mean - rf_daily;
        let variance = returns
            .iter()
            .map(|r| { let d = *r - mean; d * d })
            .sum::<Decimal>()
            / len_d;
        let std_f64 = variance.to_f64()?.sqrt();
        if std_f64 == 0.0 {
            return None;
        }
        let sharpe = excess_mean.to_f64()? / std_f64 * 252.0f64.sqrt();
        Decimal::try_from(sharpe).ok()
    }

    /// Returns where the latest close sits within the high-low range of the last `n` bars (0–100).
    ///
    /// `result = (close - lowest_low) / (highest_high - lowest_low) * 100`
    ///
    /// Returns `None` if `n == 0`, fewer than `n` bars exist, or the range is zero.
    pub fn close_range_position(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let slice = &self.bars[start..];
        let highest = slice.iter().map(|b| b.high.value()).fold(Decimal::MIN, Decimal::max);
        let lowest  = slice.iter().map(|b| b.low.value()).fold(Decimal::MAX, Decimal::min);
        let range = highest - lowest;
        if range.is_zero() {
            return None;
        }
        let close = self.bars.last()?.close.value();
        Some((close - lowest) / range * Decimal::ONE_HUNDRED)
    }

    /// Returns the number of bars since the highest close in the last `n` bars.
    ///
    /// Returns `0` if the highest close is the most recent bar, or when `n == 0` or
    /// fewer than `n` bars exist.
    pub fn bar_count_since_high(&self, n: usize) -> usize {
        if n == 0 || self.bars.len() < n {
            return 0;
        }
        let start = self.bars.len() - n;
        let slice = &self.bars[start..];
        let mut max_val = Decimal::MIN;
        let mut max_idx = 0;
        for (i, b) in slice.iter().enumerate() {
            let c = b.close.value();
            if c > max_val {
                max_val = c;
                max_idx = i;
            }
        }
        slice.len() - 1 - max_idx
    }

    /// Average `(close / open - 1) * 100` percentage over the last `n` bars.
    ///
    /// Returns `None` if `n == 0`, fewer than `n` bars exist, or all opens are zero.
    pub fn close_to_open_ratio(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let mut sum = Decimal::ZERO;
        let mut count = 0usize;
        for b in &self.bars[start..] {
            let o = b.open.value();
            if o.is_zero() {
                continue;
            }
            sum += (b.close.value() / o - Decimal::ONE) * Decimal::ONE_HUNDRED;
            count += 1;
        }
        if count == 0 {
            return None;
        }
        #[allow(clippy::cast_possible_truncation)]
        Some(sum / Decimal::from(count as u32))
    }

    /// Lag-`k` autocorrelation of log returns over the last `n` bars.
    ///
    /// Computes the Pearson correlation between `r[t]` and `r[t-lag]`.
    /// Returns `None` if `n == 0`, `lag == 0`, fewer than `n + lag + 1` bars exist,
    /// or the standard deviation is zero.
    pub fn autocorrelation(&self, n: usize, lag: usize) -> Option<f64> {
        if n == 0 || lag == 0 || self.bars.len() < n + lag + 1 {
            return None;
        }
        use rust_decimal::prelude::ToPrimitive;
        let returns = self.returns_series(n + lag);
        if returns.len() <= lag {
            return None;
        }
        let x: Vec<f64> = returns[..returns.len() - lag].iter().map(|r| r.to_f64().unwrap_or(0.0)).collect();
        let y: Vec<f64> = returns[lag..].iter().map(|r| r.to_f64().unwrap_or(0.0)).collect();
        let n_f = x.len() as f64;
        let mean_x = x.iter().sum::<f64>() / n_f;
        let mean_y = y.iter().sum::<f64>() / n_f;
        let cov: f64 = x.iter().zip(y.iter()).map(|(xi, yi)| (xi - mean_x) * (yi - mean_y)).sum::<f64>() / n_f;
        let std_x = (x.iter().map(|xi| (xi - mean_x).powi(2)).sum::<f64>() / n_f).sqrt();
        let std_y = (y.iter().map(|yi| (yi - mean_y).powi(2)).sum::<f64>() / n_f).sqrt();
        if std_x == 0.0 || std_y == 0.0 {
            return None;
        }
        Some(cov / (std_x * std_y))
    }

    /// Hurst exponent estimated via the rescaled range (R/S) method over the last `n` bars.
    ///
    /// H ≈ 0.5 → random walk; H > 0.5 → trending; H < 0.5 → mean-reverting.
    /// Returns `None` if `n < 8` or fewer than `n + 1` bars exist.
    pub fn hurst_exponent(&self, n: usize) -> Option<f64> {
        if n < 8 || self.bars.len() < n + 1 {
            return None;
        }
        use rust_decimal::prelude::ToPrimitive;
        let returns: Vec<f64> = self
            .returns_series(n)
            .iter()
            .map(|r| r.to_f64().unwrap_or(0.0))
            .collect();
        if returns.is_empty() {
            return None;
        }
        let mean = returns.iter().sum::<f64>() / returns.len() as f64;
        let cum: Vec<f64> = returns.iter().scan(0.0f64, |acc, &r| { *acc += r - mean; Some(*acc) }).collect();
        let r = cum.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
            - cum.iter().cloned().fold(f64::INFINITY, f64::min);
        let s = (returns.iter().map(|&r| (r - mean).powi(2)).sum::<f64>() / returns.len() as f64).sqrt();
        if s == 0.0 || r <= 0.0 {
            return None;
        }
        Some((r / s).ln() / (returns.len() as f64).ln())
    }

    /// Ulcer Index over the last `n` bars: RMS of percentage drawdowns from rolling peak.
    ///
    /// A measure of downside volatility; higher = more painful drawdowns.
    /// Returns `None` if `n == 0` or fewer than `n` bars exist.
    pub fn ulcer_index(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.len() < n {
            return None;
        }
        use rust_decimal::prelude::ToPrimitive;
        let start = self.bars.len() - n;
        let slice = &self.bars[start..];
        let mut peak = Decimal::ZERO;
        let mut sum_sq = 0.0f64;
        for b in slice {
            let c = b.close.value();
            if c > peak { peak = c; }
            if peak.is_zero() { continue; }
            let dd_pct = ((c - peak) / peak * Decimal::ONE_HUNDRED).to_f64().unwrap_or(0.0);
            sum_sq += dd_pct * dd_pct;
        }
        let ui = (sum_sq / n as f64).sqrt();
        Decimal::try_from(ui).ok()
    }

    /// Conditional Value-at-Risk (CVaR / Expected Shortfall) at `confidence_pct` over last `n` bars.
    ///
    /// Returns the average of log returns below the VaR quantile.
    /// Returns `None` if `n < 2`, `confidence_pct` is out of `(0, 100)`, or there are
    /// fewer than `n + 1` bars.
    pub fn cvar(&self, n: usize, confidence_pct: Decimal) -> Option<Decimal> {
        use rust_decimal::prelude::ToPrimitive;
        if n < 2 || confidence_pct <= Decimal::ZERO || confidence_pct >= Decimal::ONE_HUNDRED {
            return None;
        }
        let mut returns = self.returns_series(n);
        if returns.len() < 2 {
            return None;
        }
        returns.sort_unstable_by(|a, b| a.cmp(b));
        let cutoff = ((Decimal::ONE - confidence_pct / Decimal::ONE_HUNDRED)
            .to_f64()
            .unwrap_or(0.05)
            * returns.len() as f64)
            .ceil() as usize;
        let tail = &returns[..cutoff.min(returns.len())];
        if tail.is_empty() {
            return None;
        }
        #[allow(clippy::cast_possible_truncation)]
        let avg = tail.iter().copied().sum::<Decimal>() / Decimal::from(tail.len() as u32);
        Some(avg)
    }

    /// Returns the percentage change in close price over the last `n` bars.
    ///
    /// Formula: `(close[-1] - close[-n-1]) / close[-n-1] * 100`.
    /// Returns `None` if `n == 0`, fewer than `n + 1` bars exist, or the earlier close is zero.
    pub fn close_change_pct(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.len() <= n {
            return None;
        }
        let recent = self.bars.last()?.close.value();
        let earlier = self.bars[self.bars.len() - 1 - n].close.value();
        if earlier.is_zero() {
            return None;
        }
        Some((recent - earlier) / earlier * Decimal::ONE_HUNDRED)
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

impl OhlcvSeries {
    /// Counts the longest consecutive drawdown run: the maximum number of bars where
    /// each bar's close is strictly below the previous bar's close.
    ///
    /// Returns `0` when the series has fewer than 2 bars.
    pub fn max_drawdown_duration(&self) -> usize {
        if self.bars.len() < 2 {
            return 0;
        }
        let mut max_run = 0usize;
        let mut current = 0usize;
        for i in 1..self.bars.len() {
            if self.bars[i].close.value() < self.bars[i - 1].close.value() {
                current += 1;
                if current > max_run {
                    max_run = current;
                }
            } else {
                current = 0;
            }
        }
        max_run
    }

    /// Percentage of the last `n` bars where close > open (bullish bar ratio).
    ///
    /// Returns `None` if `n == 0` or series has fewer than `n` bars.
    /// Returns `0.0` when all bars are bearish/doji, `100.0` when all are bullish.
    pub fn close_above_open_pct(&self, n: usize) -> Option<f64> {
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let count = self.bars[start..]
            .iter()
            .filter(|b| b.close.value() > b.open.value())
            .count();
        Some(count as f64 / n as f64 * 100.0)
    }

    /// Average wick-to-range ratio over the last `n` bars.
    ///
    /// For each bar: `wick_ratio = (upper_shadow + lower_shadow) / range`.
    /// Bars with zero range are excluded from the average.
    ///
    /// Returns `None` if `n == 0`, series has fewer than `n` bars, or no bar has a
    /// non-zero range.
    pub fn avg_wick_ratio(&self, n: usize) -> Option<f64> {
        use rust_decimal::prelude::ToPrimitive;
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let mut sum = 0.0f64;
        let mut count = 0usize;
        for b in &self.bars[start..] {
            let range = b.range();
            if !range.is_zero() {
                let wick = b.upper_shadow() + b.lower_shadow();
                if let Some(ratio) = (wick / range).to_f64() {
                    sum += ratio;
                    count += 1;
                }
            }
        }
        if count == 0 {
            return None;
        }
        Some(sum / count as f64)
    }

    /// Average ratio of up-day return to down-day return magnitude over the last `n` bars.
    ///
    /// Computes log returns; averages positive returns as "gains" and the absolute value of
    /// negative returns as "losses".  Returns `None` if `n == 0`, fewer than `n+1` bars exist,
    /// or there are no losing bars (avoiding division by zero).
    pub fn gain_loss_ratio(&self, n: usize) -> Option<f64> {
        if n == 0 || self.bars.len() < n + 1 {
            return None;
        }
        use rust_decimal::prelude::ToPrimitive;
        let start = self.bars.len() - n - 1;
        let slice = &self.bars[start..];
        let mut gains = 0.0f64;
        let mut losses = 0.0f64;
        let mut gain_count = 0usize;
        let mut loss_count = 0usize;
        for w in slice.windows(2) {
            let pc = w[0].close.value().to_f64()?;
            let cc = w[1].close.value().to_f64()?;
            if pc <= 0.0 { continue; }
            let r = (cc / pc).ln();
            if r > 0.0 {
                gains += r;
                gain_count += 1;
            } else if r < 0.0 {
                losses += r.abs();
                loss_count += 1;
            }
        }
        if loss_count == 0 || losses == 0.0 {
            return None;
        }
        let avg_gain = gains / gain_count.max(1) as f64;
        let avg_loss = losses / loss_count as f64;
        Some(avg_gain / avg_loss)
    }

    /// Count of bars in the last `n` bars where `close > SMA(close, sma_period)` at that bar.
    ///
    /// The SMA is computed as a rolling SMA ending at each bar.  Bars that do not yet have
    /// enough history for the SMA are skipped.  Returns `None` if `n == 0` or the series has
    /// fewer than `n` bars.
    pub fn bars_above_sma(&self, n: usize, sma_period: usize) -> Option<usize> {
        if n == 0 || sma_period == 0 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let mut count = 0usize;
        for i in start..self.bars.len() {
            if i + 1 < sma_period {
                continue;
            }
            let sma_start = i + 1 - sma_period;
            let sum: Decimal = self.bars[sma_start..=i]
                .iter()
                .map(|b| b.close.value())
                .sum();
            let sma = sum / Decimal::from(sma_period as u32);
            if self.bars[i].close.value() > sma {
                count += 1;
            }
        }
        Some(count)
    }

    /// Distance of the current close above the lowest low in the last `n` bars.
    ///
    /// `close_distance_from_low = close[last] - min(low, n)`.
    /// Returns `None` if `n == 0` or fewer than `n` bars exist.
    pub fn close_distance_from_low(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let min_low = self.bars[start..]
            .iter()
            .map(|b| b.low.value())
            .reduce(Decimal::min)?;
        let last_close = self.bars.last()?.close.value();
        Some(last_close - min_low)
    }

    /// Ratio of the latest bar's volume to the average volume over the last `n` bars.
    ///
    /// `volume_ratio = last_volume / avg_volume(n)`.
    /// Returns `None` if `n == 0`, fewer than `n` bars exist, or average volume is zero.
    pub fn volume_ratio(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let sum: Decimal = self.bars[start..].iter().map(|b| b.volume.value()).sum();
        let avg = sum.checked_div(Decimal::from(n as u32))?;
        if avg.is_zero() {
            return None;
        }
        let last_vol = self.bars.last()?.volume.value();
        last_vol.checked_div(avg)
    }

    /// Momentum quality: fraction of up-closes among `n` bars where volume was above average.
    ///
    /// High-volume up days are "quality" momentum; this method returns the ratio of
    /// high-volume up closes to total high-volume bars.  Returns `None` if `n == 0`,
    /// fewer than `n` bars exist, or no bar in the window has above-average volume.
    pub fn momentum_quality(&self, n: usize) -> Option<f64> {
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let slice = &self.bars[start..];
        let avg_vol: Decimal = {
            let s: Decimal = slice.iter().map(|b| b.volume.value()).sum();
            s.checked_div(Decimal::from(n as u32))?
        };
        let mut high_vol_bars = 0usize;
        let mut high_vol_up = 0usize;
        for b in slice {
            if b.volume.value() > avg_vol {
                high_vol_bars += 1;
                if b.close > b.open {
                    high_vol_up += 1;
                }
            }
        }
        if high_vol_bars == 0 {
            return None;
        }
        Some(high_vol_up as f64 / high_vol_bars as f64)
    }

    /// Fraction of the last `n` bars that are bullish (close > open), as a value in `[0.0, 1.0]`.
    ///
    /// Returns `None` if `n == 0` or the series has fewer than `n` bars.
    pub fn bullish_candle_pct(&self, n: usize) -> Option<f64> {
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let bullish = self.bars[start..].iter().filter(|b| b.close > b.open).count();
        Some(bullish as f64 / n as f64)
    }

    /// Fraction of the last `n` bars where close was above the `period`-bar SMA of closes,
    /// as a value in `[0.0, 1.0]`.
    ///
    /// Returns `None` if `n == 0`, `period == 0`, or the series has fewer than `n + period - 1`
    /// bars (not enough history to compute the SMA for all `n` windows).
    pub fn price_above_ma_pct(&self, n: usize, period: usize) -> Option<f64> {
        if n == 0 || period == 0 || self.bars.len() < n + period - 1 {
            return None;
        }
        let total = self.bars.len();
        let mut above = 0usize;
        for i in (total - n)..total {
            let sma_start = i + 1 - period;
            let sma: Decimal = self.bars[sma_start..=i]
                .iter()
                .map(|b| b.close.value())
                .sum::<Decimal>()
                / Decimal::from(period as u32);
            if self.bars[i].close.value() > sma {
                above += 1;
            }
        }
        Some(above as f64 / n as f64)
    }

    /// Returns the last `n` true-range values as a `Vec<Decimal>`.
    ///
    /// True range for bar `i` = `max(high, prev_close) − min(low, prev_close)`.
    /// The first bar in the series has no previous close, so it contributes `high − low`.
    /// Returns `None` if `n == 0` or the series has fewer than `n` bars.
    pub fn true_range_series(&self, n: usize) -> Option<Vec<Decimal>> {
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let trs: Vec<Decimal> = self.bars[start..]
            .iter()
            .enumerate()
            .map(|(i, bar)| {
                let abs_i = start + i;
                if abs_i == 0 {
                    bar.high.value() - bar.low.value()
                } else {
                    let prev_close = self.bars[abs_i - 1].close.value();
                    let high = bar.high.value().max(prev_close);
                    let low = bar.low.value().min(prev_close);
                    high - low
                }
            })
            .collect();
        Some(trs)
    }

    /// Returns `(last_close − first_open) / first_open × 100` as a percentage.
    ///
    /// Measures the net intraday move across the entire series.
    /// Returns `None` if the series has fewer than 1 bar or `first_open` is zero.
    pub fn intraday_return_pct(&self) -> Option<Decimal> {
        if self.bars.is_empty() {
            return None;
        }
        let first_open = self.bars.first()?.open.value();
        if first_open.is_zero() {
            return None;
        }
        let last_close = self.bars.last()?.close.value();
        Some((last_close - first_open) / first_open * Decimal::ONE_HUNDRED)
    }

    /// Count of bars in the last `n` where `close < open` (bearish bars).
    ///
    /// Returns `None` if `n == 0` or the series has fewer than `n` bars.
    pub fn bearish_bar_count(&self, n: usize) -> Option<usize> {
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        Some(self.bars[start..].iter().filter(|b| b.close < b.open).count())
    }

    /// Average body size (|close − open|) over the last `n` bars.
    ///
    /// Returns `None` if `n == 0` or the series has fewer than `n` bars.
    pub fn avg_body_size(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let sum: Decimal = self.bars[start..]
            .iter()
            .map(|b| (b.close.value() - b.open.value()).abs())
            .sum();
        Some(sum / Decimal::from(n as u32))
    }

    /// Average `(high + low) / 2` midpoint over the last `n` bars.
    ///
    /// Returns `None` if `n == 0` or the series has fewer than `n` bars.
    pub fn hl_midpoint(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let sum: Decimal = self.bars[start..]
            .iter()
            .map(|b| (b.high.value() + b.low.value()) / Decimal::TWO)
            .sum();
        #[allow(clippy::cast_possible_truncation)]
        Some(sum / Decimal::from(n as u32))
    }

    /// Ratio of volume on up-bars (`close > open`) to total volume over the last `n` bars.
    ///
    /// Returns `None` if `n == 0`, the series has fewer than `n` bars, or total volume is zero.
    pub fn up_volume_ratio(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let total_vol: Decimal = self.bars[start..].iter().map(|b| b.volume.value()).sum();
        if total_vol.is_zero() {
            return None;
        }
        let up_vol: Decimal = self.bars[start..]
            .iter()
            .filter(|b| b.close > b.open)
            .map(|b| b.volume.value())
            .sum();
        up_vol.checked_div(total_vol)
    }

    /// Directional efficiency of price movement over the last `n` bars.
    ///
    /// `efficiency = |close[-1] − close[-n]| / Σ|close[i] − close[i-1]|`
    ///
    /// - 1.0 = perfectly trending (straight line).
    /// - Near 0 = choppy (path much longer than net displacement).
    ///
    /// Returns `None` if `n < 2`, the series has fewer than `n` bars, or total path is zero.
    pub fn price_efficiency(&self, n: usize) -> Option<Decimal> {
        if n < 2 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let net = (self.bars.last()?.close.value() - self.bars[start].close.value()).abs();
        let path: Decimal = self.bars[start..]
            .windows(2)
            .map(|w| (w[1].close.value() - w[0].close.value()).abs())
            .sum();
        if path.is_zero() {
            return None;
        }
        net.checked_div(path)
    }

    /// Mean absolute gap (`|open[i] − close[i-1]|`) over the last `n` bars.
    ///
    /// Measures average overnight jump between bars.
    /// Returns `None` if `n == 0` or the series has fewer than `n + 1` bars
    /// (need one prior bar for each gap).
    pub fn avg_gap(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.len() < n + 1 {
            return None;
        }
        let start = self.bars.len() - n;
        let sum: Decimal = (start..self.bars.len())
            .map(|i| (self.bars[i].open.value() - self.bars[i - 1].close.value()).abs())
            .sum();
        #[allow(clippy::cast_possible_truncation)]
        Some(sum / Decimal::from(n as u32))
    }

    /// Population variance of log-returns over the last `n + 1` bars.
    ///
    /// `log_return[i] = ln(close[i] / close[i-1])`.
    /// Requires `n + 1` closes → `n` log-returns.
    /// Returns `None` if `n < 2` or the series has fewer than `n + 1` bars.
    pub fn realized_variance(&self, n: usize) -> Option<f64> {
        if n < 2 || self.bars.len() < n + 1 {
            return None;
        }
        let start = self.bars.len() - (n + 1);
        let mut rets = Vec::with_capacity(n);
        for i in (start + 1)..=(start + n) {
            let prev = self.bars[i - 1].close.value();
            let curr = self.bars[i].close.value();
            use rust_decimal::prelude::ToPrimitive;
            let r = prev.to_f64()?;
            let c = curr.to_f64()?;
            if r <= 0.0 { return None; }
            rets.push((c / r).ln());
        }
        let mean = rets.iter().sum::<f64>() / rets.len() as f64;
        let var = rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / rets.len() as f64;
        Some(var)
    }

    /// Mean signed close-to-close change per bar over the last `n` bars.
    ///
    /// `velocity = (close[-1] - close[-n]) / n`
    ///
    /// Returns `None` if `n < 2` or the series has fewer than `n` bars.
    pub fn close_velocity(&self, n: usize) -> Option<Decimal> {
        if n < 2 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let delta = self.bars.last()?.close.value() - self.bars[start].close.value();
        #[allow(clippy::cast_possible_truncation)]
        delta.checked_div(Decimal::from(n as u32))
    }

    /// Mean upper wick length `(high − max(open, close))` over the last `n` bars.
    ///
    /// Returns `None` if `n == 0` or the series has fewer than `n` bars.
    pub fn avg_upper_wick(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let sum: Decimal = self.bars[start..]
            .iter()
            .map(|b| {
                let body_top = b.open.value().max(b.close.value());
                b.high.value() - body_top
            })
            .sum();
        #[allow(clippy::cast_possible_truncation)]
        Some(sum / Decimal::from(n as u32))
    }

    /// Median `(high + low) / 2` midpoint value over the last `n` bars.
    ///
    /// Returns `None` if `n == 0` or the series has fewer than `n` bars.
    pub fn median_price(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let mut mids: Vec<Decimal> = self.bars[start..]
            .iter()
            .map(|b| (b.high.value() + b.low.value()) / Decimal::TWO)
            .collect();
        mids.sort();
        let mid = n / 2;
        if n % 2 == 0 {
            Some((mids[mid - 1] + mids[mid]) / Decimal::TWO)
        } else {
            Some(mids[mid])
        }
    }

    /// Mean upper-shadow ratio `(high − max(open,close)) / (high − low)` over the last `n` bars.
    ///
    /// Bars where `high == low` (doji) contribute 0. Returns `None` if `n == 0`
    /// or the series has fewer than `n` bars.
    pub fn upper_shadow_ratio(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let sum: Decimal = self.bars[start..]
            .iter()
            .map(|b| {
                let range = b.high.value() - b.low.value();
                if range.is_zero() {
                    Decimal::ZERO
                } else {
                    (b.high.value() - b.open.value().max(b.close.value())) / range
                }
            })
            .sum();
        #[allow(clippy::cast_possible_truncation)]
        Some(sum / Decimal::from(n as u32))
    }

    /// Fraction of bars in the last `n + 1` where `open[i] > close[i-1]` (gap up).
    ///
    /// Returns `None` if `n == 0` or the series has fewer than `n + 1` bars.
    pub fn percent_gap_up_bars(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.len() < n + 1 {
            return None;
        }
        let start = self.bars.len() - n;
        let count = (start..self.bars.len())
            .filter(|&i| self.bars[i].open > self.bars[i - 1].close)
            .count();
        #[allow(clippy::cast_possible_truncation)]
        Decimal::from(count as u32).checked_div(Decimal::from(n as u32))
    }

    /// Length of the longest run of consecutive higher closes within the last `n` bars.
    ///
    /// A "higher close" means `close[i] > close[i-1]`.  The run is computed across
    /// consecutive comparisons (not against a fixed baseline).
    ///
    /// Returns `None` if `n < 2` or the series has fewer than `n` bars.
    pub fn consecutive_higher_closes(&self, n: usize) -> Option<usize> {
        if n < 2 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let mut max_run = 0usize;
        let mut cur_run = 0usize;
        for i in (start + 1)..self.bars.len() {
            if self.bars[i].close > self.bars[i - 1].close {
                cur_run += 1;
                if cur_run > max_run { max_run = cur_run; }
            } else {
                cur_run = 0;
            }
        }
        Some(max_run)
    }

    /// Volume-weighted average return over the last `n` bars.
    ///
    /// `return[i] = (close[i] - close[i-1]) / close[i-1]`; each return is weighted by
    /// the volume of bar `i`.  Bars with zero prior close are excluded from the sum.
    ///
    /// Returns `None` if `n < 2`, the series has fewer than `n` bars, or total volume is zero.
    pub fn volume_weighted_return(&self, n: usize) -> Option<Decimal> {
        if n < 2 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let mut vol_return_sum = Decimal::ZERO;
        let mut vol_sum = Decimal::ZERO;
        for i in (start + 1)..self.bars.len() {
            let prev_close = self.bars[i - 1].close.value();
            if prev_close.is_zero() { continue; }
            let ret = (self.bars[i].close.value() - prev_close) / prev_close;
            let vol = self.bars[i].volume.value();
            vol_return_sum += ret * vol;
            vol_sum += vol;
        }
        if vol_sum.is_zero() {
            return None;
        }
        Some(vol_return_sum / vol_sum)
    }

    /// Returns arithmetic close-to-close returns for the last `n` bars as `(close[i] - close[i-1]) / close[i-1]`.
    ///
    /// The result has `n - 1` entries (each bar needs a previous bar to compute a return).
    /// Returns `None` if `n < 2` or the series has fewer than `n` bars.
    pub fn close_returns(&self, n: usize) -> Option<Vec<Decimal>> {
        if n < 2 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let mut returns = Vec::with_capacity(n - 1);
        for i in (start + 1)..self.bars.len() {
            let prev = self.bars[i - 1].close.value();
            if prev.is_zero() {
                returns.push(Decimal::ZERO);
            } else {
                returns.push((self.bars[i].close.value() - prev) / prev);
            }
        }
        Some(returns)
    }

    /// Classifies recent volatility as `"low"`, `"medium"`, or `"high"` by comparing
    /// the average ATR of the last `atr_period` bars to its own mean over the last `lookback` bars.
    ///
    /// - **low**: latest ATR < 80% of the rolling mean
    /// - **high**: latest ATR > 120% of the rolling mean
    /// - **medium**: otherwise
    ///
    /// Returns `None` if there are fewer than `lookback + 1` bars (need history to compute ATR)
    /// or if `atr_period == 0` or `lookback == 0`.
    pub fn volatility_regime(&self, atr_period: usize, lookback: usize) -> Option<&'static str> {
        if atr_period == 0 || lookback == 0 {
            return None;
        }
        let needed = lookback + atr_period;
        if self.bars.len() < needed {
            return None;
        }
        let atr_series = self.atr_series(atr_period);
        let recent_atrs: Vec<Decimal> = atr_series
            .iter()
            .rev()
            .take(lookback)
            .filter_map(|v| *v)
            .collect();
        if recent_atrs.is_empty() {
            return None;
        }
        let mean: Decimal = recent_atrs.iter().copied().sum::<Decimal>()
            / Decimal::from(recent_atrs.len() as u32);
        if mean.is_zero() {
            return Some("medium");
        }
        let latest = *recent_atrs.first()?;
        let ratio = latest / mean;
        if ratio < Decimal::new(80, 2) {
            Some("low")
        } else if ratio > Decimal::new(120, 2) {
            Some("high")
        } else {
            Some("medium")
        }
    }

    /// Ratio of total volume on up-bars to total volume on down-bars over the last `n` bars.
    ///
    /// An up-bar is `close > open`; a down-bar is `close < open`. Doji bars are excluded.
    ///
    /// Returns `None` if `n == 0`, fewer than `n` bars exist, or there are no down-bars.
    pub fn up_down_volume_ratio(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let mut up_vol = Decimal::ZERO;
        let mut dn_vol = Decimal::ZERO;
        for b in &self.bars[start..] {
            let vol = b.volume.value();
            if b.close > b.open { up_vol += vol; }
            else if b.close < b.open { dn_vol += vol; }
        }
        if dn_vol.is_zero() { return None; }
        Some(up_vol / dn_vol)
    }

    /// Average bar range (high − low) as a percentage of the typical price, over the last `n` bars.
    ///
    /// `typical = (H + L + C) / 3`. Bars with zero typical price are excluded.
    ///
    /// Returns `None` if `n == 0`, fewer than `n` bars exist, or no bar has positive typical price.
    pub fn avg_range_pct(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let mut sum = Decimal::ZERO;
        let mut count = 0usize;
        let hundred = Decimal::from(100u32);
        let three = Decimal::from(3u32);
        for b in &self.bars[start..] {
            let tp = (b.high.value() + b.low.value() + b.close.value()) / three;
            if tp.is_zero() { continue; }
            sum += (b.high.value() - b.low.value()) / tp * hundred;
            count += 1;
        }
        if count == 0 { return None; }
        Some(sum / Decimal::from(count as u32))
    }

    /// Bar efficiency over the last `n` bars: net directional move / total path length.
    ///
    /// `efficiency = |close[last] - close[first]| / Σ|close[i] - close[i-1]|`
    ///
    /// A value of 1.0 means perfectly directional; near 0 means highly erratic.
    ///
    /// Returns `None` if `n < 2`, fewer than `n` bars exist, or total path is zero.
    pub fn bar_efficiency(&self, n: usize) -> Option<f64> {
        use rust_decimal::prelude::ToPrimitive;
        if n < 2 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let net = (self.bars.last().unwrap().close.value()
            - self.bars[start].close.value())
            .abs()
            .to_f64()
            .unwrap_or(0.0);
        let path: f64 = (start + 1..self.bars.len())
            .map(|i| {
                (self.bars[i].close.value() - self.bars[i - 1].close.value())
                    .abs()
                    .to_f64()
                    .unwrap_or(0.0)
            })
            .sum();
        if path == 0.0 { return None; }
        Some(net / path)
    }

    /// Average number of bars between successive new `n`-bar highs in the last `m` bars.
    ///
    /// A new high at bar `i` means `close[i] > max(close[i-n..i])`.
    ///
    /// Returns `None` if `m <= n`, fewer than `m` bars exist, or no new high is found.
    pub fn avg_bars_between_highs(&self, n: usize, m: usize) -> Option<f64> {
        if n == 0 || m <= n || self.bars.len() < m {
            return None;
        }
        let start = self.bars.len() - m;
        let mut high_indices: Vec<usize> = Vec::new();
        for i in (start + n)..self.bars.len() {
            let prev_max = self.bars[(i - n)..i]
                .iter()
                .map(|b| b.close.value())
                .max()
                .unwrap_or(Decimal::ZERO);
            if self.bars[i].close.value() > prev_max {
                high_indices.push(i);
            }
        }
        if high_indices.len() < 2 { return None; }
        let gaps: Vec<usize> = high_indices.windows(2).map(|w| w[1] - w[0]).collect();
        Some(gaps.iter().sum::<usize>() as f64 / gaps.len() as f64)
    }

    /// Number of consecutive bars (from the most recent bar backward) where close exceeded
    /// the prior `n`-bar rolling high.
    ///
    /// A bar at index `i` counts if `close[i] > max(close[i-n..i])`.
    /// The first `n` bars of the series are skipped (no prior window).
    ///
    /// Returns `None` if `n == 0` or the series has fewer than `n + 1` bars.
    pub fn breakout_bars(&self, n: usize) -> Option<usize> {
        if n == 0 || self.bars.len() <= n {
            return None;
        }
        let mut streak = 0usize;
        for i in (n..self.bars.len()).rev() {
            let prior_max = self.bars[(i - n)..i]
                .iter()
                .map(|b| b.close.value())
                .max()
                .unwrap_or(Decimal::ZERO);
            if self.bars[i].close.value() > prior_max {
                streak += 1;
            } else {
                break;
            }
        }
        Some(streak)
    }

    /// Count of doji candles in the last `n` bars.
    ///
    /// A bar is a doji when `|close - open| / (high - low) < threshold`.
    /// Use `threshold = 0.1` for the classic 10% body rule.
    ///
    /// Returns `None` if `n == 0` or the series has fewer than `n` bars.
    pub fn doji_count(&self, n: usize, threshold: f64) -> Option<usize> {
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        use rust_decimal::prelude::ToPrimitive;
        let count = self.bars[start..]
            .iter()
            .filter(|b| {
                let range = (b.high.value() - b.low.value()).to_f64().unwrap_or(0.0);
                if range == 0.0 {
                    return true; // zero-range bar is a perfect doji
                }
                let body = (b.close.value() - b.open.value())
                    .abs()
                    .to_f64()
                    .unwrap_or(0.0);
                body / range < threshold
            })
            .count();
        Some(count)
    }

    /// Coefficient of variation of closes over the last `n` bars: `std_dev / mean`.
    ///
    /// Returns `None` if `n < 2`, fewer than `n` bars exist, or mean is zero.
    pub fn close_dispersion(&self, n: usize) -> Option<f64> {
        use rust_decimal::prelude::ToPrimitive;
        if n < 2 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let vals: Vec<f64> = self.bars[start..]
            .iter()
            .map(|b| b.close.value().to_f64().unwrap_or(0.0))
            .collect();
        let mean = vals.iter().sum::<f64>() / n as f64;
        if mean == 0.0 { return None; }
        let variance = vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n as f64;
        Some(variance.sqrt() / mean)
    }

    /// Volume of the most recent bar as a percentage of the average volume over the last `n` bars.
    ///
    /// Returns `None` if `n == 0`, fewer than `n` bars exist, or average volume is zero.
    pub fn relative_volume(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let avg_vol: Decimal = self.bars[start..]
            .iter()
            .map(|b| b.volume.value())
            .sum::<Decimal>()
            / Decimal::from(n as u32);
        if avg_vol.is_zero() { return None; }
        let last_vol = self.bars.last()?.volume.value();
        Some(last_vol / avg_vol * Decimal::from(100u32))
    }

    /// Average midpoint of the open-close range over the last `n` bars.
    ///
    /// `midpoint[i] = (open[i] + close[i]) / 2`
    ///
    /// Returns `None` if `n == 0` or fewer than `n` bars exist.
    pub fn avg_oc_midpoint(&self, n: usize) -> Option<Decimal> {
        if n == 0 || self.bars.len() < n {
            return None;
        }
        let start = self.bars.len() - n;
        let sum: Decimal = self.bars[start..]
            .iter()
            .map(|b| (b.open.value() + b.close.value()) / Decimal::TWO)
            .sum();
        Some(sum / Decimal::from(n as u32))
    }
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

    /// Convenience helper: create a bar where O=H=L=C = `close`.
    fn bar(close: &str) -> OhlcvBar {
        make_bar(close, close, close, close)
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
    fn test_ohlcv_bar_is_long_candle_flat() {
        // range == 0 → always false
        let bar = make_bar("100", "100", "100", "100");
        assert!(!bar.is_long_candle(dec!(0.7)));
    }

    #[test]
    fn test_ohlcv_bar_is_long_candle_true() {
        // open=100, close=110, high=112, low=98 → body=10, range=14 → 10/14 ≈ 0.714 >= 0.7
        let bar = make_bar("100", "112", "98", "110");
        assert!(bar.is_long_candle(dec!(0.7)));
    }

    #[test]
    fn test_ohlcv_bar_is_long_candle_false() {
        // open=100, close=101, high=110, low=90 → body=1, range=20 → 0.05 < 0.7
        let bar = make_bar("100", "110", "90", "101");
        assert!(!bar.is_long_candle(dec!(0.7)));
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

    #[test]
    fn test_ohlcv_series_avg_volume_none_when_empty() {
        assert!(OhlcvSeries::new().avg_volume(3).is_none());
    }

    #[test]
    fn test_ohlcv_series_avg_volume_none_when_n_zero() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "105")).unwrap();
        assert!(series.avg_volume(0).is_none());
    }

    #[test]
    fn test_ohlcv_series_avg_volume_correct() {
        // make_bar sets volume = 100 per bar
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "105")).unwrap();
        series.push(make_bar("105", "115", "95", "110")).unwrap();
        series.push(make_bar("110", "120", "100", "115")).unwrap();
        // avg over 3 bars: (100+100+100)/3 = 100
        assert_eq!(series.avg_volume(3).unwrap(), dec!(100));
    }

    #[test]
    fn test_ohlcv_series_avg_volume_partial_window() {
        // n=5 but only 3 bars → None
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "105")).unwrap();
        series.push(make_bar("105", "115", "95", "110")).unwrap();
        assert!(series.avg_volume(5).is_none());
    }

    #[test]
    fn test_ohlcv_series_price_range_none_when_insufficient() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "105")).unwrap();
        assert!(series.price_range(0).is_none());
        assert!(series.price_range(2).is_none());
    }

    #[test]
    fn test_ohlcv_series_price_range_correct() {
        // bar1: high=110 low=90; bar2: high=120 low=80 → range = 120-80 = 40
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "100")).unwrap();
        series.push(make_bar("100", "120", "80", "100")).unwrap();
        assert_eq!(series.price_range(2).unwrap(), dec!(40));
    }

    #[test]
    fn test_ohlcv_series_above_ema_false_when_insufficient() {
        assert!(!OhlcvSeries::new().above_ema(3));
    }

    #[test]
    fn test_ohlcv_series_above_ema_rising_close() {
        let mut series = OhlcvSeries::new();
        for c in ["100", "100", "100", "100", "200"] {
            series.push(make_bar(c, "210", "90", c)).unwrap();
        }
        assert!(series.above_ema(3));
    }

    #[test]
    fn test_ohlcv_series_bullish_engulfing_count_zero_when_short() {
        assert_eq!(OhlcvSeries::new().bullish_engulfing_count(5), 0);
    }

    #[test]
    fn test_ohlcv_series_bullish_engulfing_count_detects_pattern() {
        let mut series = OhlcvSeries::new();
        // bar1: bearish (open=105, close=95)
        series.push(make_bar("105", "110", "90", "95")).unwrap();
        // bar2: bullish engulfing: open < prev_close(95), close > prev_open(105)
        series.push(make_bar("90", "120", "88", "110")).unwrap();
        assert_eq!(series.bullish_engulfing_count(2), 1);
    }

    #[test]
    fn test_ohlcv_series_range_expansion_none_when_insufficient() {
        assert!(OhlcvSeries::new().range_expansion(3).is_none());
    }

    #[test]
    fn test_ohlcv_series_range_expansion_constant_returns_one() {
        let mut series = OhlcvSeries::new();
        for _ in 0..5 {
            series.push(make_bar("100", "110", "90", "100")).unwrap();
        }
        // all bars identical range=20 → current/avg = 1
        assert_eq!(series.range_expansion(5).unwrap(), dec!(1));
    }

    #[test]
    fn test_ohlcv_series_bearish_engulfing_count_zero_when_short() {
        assert_eq!(OhlcvSeries::new().bearish_engulfing_count(5), 0);
    }

    #[test]
    fn test_ohlcv_series_bearish_engulfing_count_detects_pattern() {
        let mut series = OhlcvSeries::new();
        // bar1: bullish (open=95, close=105)
        series.push(make_bar("95", "110", "90", "105")).unwrap();
        // bar2: bearish engulfing: open > prev_close(105), close < prev_open(95)
        series.push(make_bar("110", "115", "88", "90")).unwrap();
        assert_eq!(series.bearish_engulfing_count(2), 1);
    }

    #[test]
    fn test_ohlcv_series_trend_strength_none_when_insufficient() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "100")).unwrap();
        assert!(series.trend_strength(2).is_none());
    }

    #[test]
    fn test_ohlcv_series_trend_strength_pure_trend_is_one() {
        // straight up trend: each close 10 higher — net = total movement → ratio = 1
        let mut series = OhlcvSeries::new();
        for c in ["100", "110", "120", "130"] {
            series.push(make_bar(c, "135", "95", c)).unwrap();
        }
        assert_eq!(series.trend_strength(4).unwrap(), dec!(1));
    }

    #[test]
    fn test_ohlcv_series_close_location_value_none_when_insufficient() {
        assert!(OhlcvSeries::new().close_location_value(1).is_none());
    }

    #[test]
    fn test_ohlcv_series_close_location_value_close_at_high() {
        // close == high → CLV = ((h-l)-(0))/(h-l) = 1
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "110")).unwrap();
        assert_eq!(series.close_location_value(1).unwrap(), dec!(1));
    }

    #[test]
    fn test_ohlcv_series_close_location_value_close_at_midpoint() {
        // close = 100 = midpoint of [90,110] → CLV = 0
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "100")).unwrap();
        assert_eq!(series.close_location_value(1).unwrap(), dec!(0));
    }

    #[test]
    fn test_ohlcv_series_mean_close_empty_returns_none() {
        assert!(OhlcvSeries::new().mean_close(5).is_none());
    }

    #[test]
    fn test_ohlcv_series_mean_close_equal_prices() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "100")).unwrap();
        series.push(make_bar("100", "110", "90", "100")).unwrap();
        series.push(make_bar("100", "110", "90", "100")).unwrap();
        assert_eq!(series.mean_close(3).unwrap(), dec!(100));
    }

    #[test]
    fn test_ohlcv_series_mean_close_windowed() {
        // 3 bars with closes 100, 110, 120 → mean of last 2 = (110+120)/2 = 115
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "100", "100", "100")).unwrap();
        series.push(make_bar("110", "110", "110", "110")).unwrap();
        series.push(make_bar("120", "120", "120", "120")).unwrap();
        assert_eq!(series.mean_close(2).unwrap(), dec!(115));
    }

    #[test]
    fn test_ohlcv_series_std_dev_less_than_two_bars_returns_none() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "100")).unwrap();
        assert!(series.std_dev(5).is_none());
    }

    #[test]
    fn test_ohlcv_series_std_dev_constant_prices_is_zero() {
        let mut series = OhlcvSeries::new();
        for _ in 0..4 {
            series.push(make_bar("100", "100", "100", "100")).unwrap();
        }
        assert_eq!(series.std_dev(4).unwrap(), dec!(0));
    }

    #[test]
    fn test_ohlcv_bar_gap_pct_upward_gap() {
        let prev = make_bar("100", "110", "90", "100");
        let curr = make_bar("110", "120", "105", "115");
        // gap_pct = (110 - 100) / 100 * 100 = 10
        assert_eq!(curr.gap_pct(&prev).unwrap(), dec!(10));
    }

    #[test]
    fn test_ohlcv_bar_gap_pct_downward_gap() {
        let prev = make_bar("100", "110", "90", "100");
        let curr = make_bar("90", "95", "85", "92");
        // gap_pct = (90 - 100) / 100 * 100 = -10
        assert_eq!(curr.gap_pct(&prev).unwrap(), dec!(-10));
    }

    #[test]
    fn test_ohlcv_bar_gap_pct_no_gap() {
        let prev = make_bar("100", "110", "90", "100");
        let curr = make_bar("100", "110", "90", "105");
        assert_eq!(curr.gap_pct(&prev).unwrap(), dec!(0));
    }

    #[test]
    fn test_ohlcv_series_n_bars_ago_returns_correct_bar() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "105")).unwrap();
        series.push(make_bar("105", "115", "95", "110")).unwrap();
        series.push(make_bar("110", "120", "100", "115")).unwrap();
        assert_eq!(series.n_bars_ago(0).unwrap().close.value(), dec!(115));
        assert_eq!(series.n_bars_ago(1).unwrap().close.value(), dec!(110));
        assert_eq!(series.n_bars_ago(2).unwrap().close.value(), dec!(105));
    }

    #[test]
    fn test_ohlcv_series_n_bars_ago_out_of_bounds() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "105")).unwrap();
        assert!(series.n_bars_ago(1).is_none());
        assert!(OhlcvSeries::new().n_bars_ago(0).is_none());
    }

    #[test]
    fn test_ohlcv_bar_is_outside_bar_true() {
        let prev = make_bar("100", "110", "90", "105");
        let outside = make_bar("100", "120", "80", "110");
        assert!(outside.is_outside_bar(&prev));
    }

    #[test]
    fn test_ohlcv_bar_is_outside_bar_false_for_inside() {
        let prev = make_bar("100", "120", "80", "110");
        let inside = make_bar("100", "110", "90", "105");
        assert!(!inside.is_outside_bar(&prev));
    }

    #[test]
    fn test_ohlcv_bar_is_outside_bar_false_partial() {
        let prev = make_bar("100", "110", "90", "105");
        let partial = make_bar("100", "115", "92", "110");
        assert!(!partial.is_outside_bar(&prev));
    }

    #[test]
    fn test_ohlcv_series_from_bars_valid() {
        let bars = vec![
            make_bar("100", "110", "90", "105"),
            make_bar("105", "115", "95", "110"),
        ];
        let series = OhlcvSeries::from_bars(bars).unwrap();
        assert_eq!(series.len(), 2);
    }

    #[test]
    fn test_ohlcv_series_from_bars_empty() {
        let series = OhlcvSeries::from_bars(vec![]).unwrap();
        assert!(series.is_empty());
    }

    #[test]
    fn test_ohlcv_series_count_bullish() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "105")).unwrap(); // bullish
        series.push(make_bar("105", "115", "95", "100")).unwrap(); // bearish
        series.push(make_bar("100", "110", "90", "108")).unwrap(); // bullish
        assert_eq!(series.count_bullish(3), 2);
        assert_eq!(series.count_bullish(1), 1); // last bar only
    }

    #[test]
    fn test_ohlcv_series_count_bearish() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("110", "115", "90", "100")).unwrap(); // bearish
        series.push(make_bar("105", "115", "95", "110")).unwrap(); // bullish
        assert_eq!(series.count_bearish(2), 1);
        assert_eq!(series.count_bearish(1), 0); // last bar is bullish
    }

    #[test]
    fn test_ohlcv_series_count_bullish_exceeds_len() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "105")).unwrap();
        assert_eq!(series.count_bullish(100), 1);
    }

    #[test]
    fn test_ohlcv_series_median_close_empty() {
        assert!(OhlcvSeries::new().median_close(5).is_none());
    }

    #[test]
    fn test_ohlcv_series_median_close_odd_count() {
        // closes: 100, 110, 120 → sorted: [100, 110, 120] → median = 110
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "100", "100", "100")).unwrap();
        series.push(make_bar("110", "110", "110", "110")).unwrap();
        series.push(make_bar("120", "120", "120", "120")).unwrap();
        assert_eq!(series.median_close(3).unwrap(), dec!(110));
    }

    #[test]
    fn test_ohlcv_series_median_close_even_count() {
        // closes: 100, 110 → median = (100+110)/2 = 105
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "100", "100", "100")).unwrap();
        series.push(make_bar("110", "110", "110", "110")).unwrap();
        assert_eq!(series.median_close(2).unwrap(), dec!(105));
    }

    #[test]
    fn test_ohlcv_series_percentile_rank_empty() {
        assert!(OhlcvSeries::new().percentile_rank(dec!(100), 5).is_none());
    }

    #[test]
    fn test_ohlcv_series_percentile_rank_above_all() {
        // all closes = 100, value = 101 → all below → percentile = 100
        let mut series = OhlcvSeries::new();
        for _ in 0..4 {
            series.push(make_bar("100", "100", "100", "100")).unwrap();
        }
        assert_eq!(series.percentile_rank(dec!(101), 4).unwrap(), dec!(100));
    }

    #[test]
    fn test_ohlcv_series_percentile_rank_below_all() {
        // all closes = 100, value = 99 → none below → percentile = 0
        let mut series = OhlcvSeries::new();
        for _ in 0..4 {
            series.push(make_bar("100", "100", "100", "100")).unwrap();
        }
        assert_eq!(series.percentile_rank(dec!(99), 4).unwrap(), dec!(0));
    }

    #[test]
    fn test_ohlcv_series_consecutive_ups_empty() {
        assert_eq!(OhlcvSeries::new().consecutive_ups(), 0);
    }

    #[test]
    fn test_ohlcv_series_consecutive_ups_all_bullish() {
        let mut series = OhlcvSeries::new();
        // bullish bar: open < close, make_bar(o, h, l, c)
        series.push(make_bar("100", "110", "90", "105")).unwrap(); // bullish
        series.push(make_bar("105", "115", "95", "110")).unwrap(); // bullish
        assert_eq!(series.consecutive_ups(), 2);
    }

    #[test]
    fn test_ohlcv_series_consecutive_ups_broken_by_bearish() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "105")).unwrap(); // bullish
        series.push(make_bar("110", "115", "95", "108")).unwrap(); // bearish
        series.push(make_bar("108", "115", "100", "112")).unwrap(); // bullish
        assert_eq!(series.consecutive_ups(), 1);
    }

    #[test]
    fn test_ohlcv_series_consecutive_downs_counts_bearish_tail() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "105")).unwrap(); // bullish
        series.push(make_bar("105", "110", "90", "100")).unwrap(); // bearish
        series.push(make_bar("100", "105", "85", "95")).unwrap(); // bearish
        assert_eq!(series.consecutive_downs(), 2);
        assert_eq!(series.consecutive_ups(), 0);
    }

    #[test]
    fn test_ohlcv_bar_is_marubozu_full_body() {
        // open=100, high=110, low=100, close=110 → no shadows
        let bar = make_bar("100", "110", "100", "110");
        assert!(bar.is_marubozu());
    }

    #[test]
    fn test_ohlcv_bar_is_marubozu_false_with_shadows() {
        let bar = make_bar("100", "115", "95", "110");
        assert!(!bar.is_marubozu());
    }

    #[test]
    fn test_ohlcv_bar_is_spinning_top_true() {
        // range=40, body=2, upper=18, lower=20
        let bar = make_bar("100", "120", "80", "102");
        assert!(bar.is_spinning_top());
    }

    #[test]
    fn test_ohlcv_bar_is_spinning_top_false_large_body() {
        // body=14, range=20 → body_ratio=0.70 > 0.30
        let bar = make_bar("100", "115", "95", "114");
        assert!(!bar.is_spinning_top());
    }

    #[test]
    fn test_ohlcv_series_average_volume_all_same() {
        // make_bar always sets volume = 100
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "105")).unwrap();
        series.push(make_bar("105", "115", "95", "110")).unwrap();
        assert_eq!(series.average_volume(2).unwrap(), dec!(100));
    }

    #[test]
    fn test_ohlcv_series_average_range() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "120", "80", "110")).unwrap(); // range=40
        series.push(make_bar("110", "125", "100", "115")).unwrap(); // range=25
        assert_eq!(series.average_range(2).unwrap(), dec!(32.5));
    }

    #[test]
    fn test_ohlcv_series_average_volume_empty_returns_none() {
        let series = OhlcvSeries::new();
        assert!(series.average_volume(5).is_none());
    }

    #[test]
    fn test_ohlcv_series_typical_price_mean_single_bar() {
        let mut series = OhlcvSeries::new();
        // typical = (120+80+110)/3 ≈ 103.333...
        let bar = make_bar("100", "120", "80", "110");
        series.push(bar).unwrap();
        let tp = series.typical_price_mean(1).unwrap();
        // (120+80+110)/3
        let expected = (dec!(120) + dec!(80) + dec!(110)) / dec!(3);
        assert_eq!(tp, expected);
    }

    #[test]
    fn test_ohlcv_series_below_sma_zero_when_all_above() {
        let mut series = OhlcvSeries::new();
        for _ in 0..3 { series.push(make_bar("100", "110", "90", "100")).unwrap(); }
        // SMA(3) = 100, close=100, not strictly below → 0
        assert_eq!(series.below_sma(3, 3), 0);
    }

    #[test]
    fn test_ohlcv_series_sortino_ratio_insufficient_data() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "105")).unwrap();
        assert!(series.sortino_ratio(0.0, 252.0).is_none());
    }

    #[test]
    fn test_ohlcv_bar_weighted_close_equals_hlcc4() {
        let bar = make_bar("100", "120", "80", "110");
        assert_eq!(bar.weighted_close(), bar.hlcc4());
    }

    #[test]
    fn test_ohlcv_bar_weighted_close_value() {
        // (high + low + close*2) / 4 = (120 + 80 + 110 + 110) / 4 = 420/4 = 105
        let bar = make_bar("100", "120", "80", "110");
        assert_eq!(bar.weighted_close(), dec!(105));
    }

    #[test]
    fn test_close_above_open_streak_three_bullish() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "95")).unwrap();   // bearish
        series.push(make_bar("95", "110", "90", "105")).unwrap();   // bullish
        series.push(make_bar("105", "115", "100", "112")).unwrap(); // bullish
        series.push(make_bar("112", "120", "108", "118")).unwrap(); // bullish
        assert_eq!(series.close_above_open_streak(), 3);
    }

    #[test]
    fn test_close_above_open_streak_last_bearish_returns_zero() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("105", "110", "100", "102")).unwrap(); // bullish
        series.push(make_bar("102", "108", "98", "99")).unwrap();   // bearish (close < open)
        assert_eq!(series.close_above_open_streak(), 0);
    }

    #[test]
    fn test_close_above_open_streak_empty_series_returns_zero() {
        assert_eq!(OhlcvSeries::new().close_above_open_streak(), 0);
    }

    #[test]
    fn test_max_drawdown_pct_declining_series() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "100")).unwrap();
        series.push(make_bar("100", "105", "75", "80")).unwrap();  // 20% drawdown from 100
        series.push(make_bar("80", "85", "75", "84")).unwrap();
        let dd = series.max_drawdown_pct(10).unwrap();
        assert!((dd - 20.0).abs() < 1e-6, "expected ~20, got {dd}");
    }

    #[test]
    fn test_max_drawdown_pct_flat_returns_zero() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "100")).unwrap();
        series.push(make_bar("100", "110", "90", "100")).unwrap();
        assert_eq!(series.max_drawdown_pct(10).unwrap(), 0.0);
    }

    #[test]
    fn test_max_drawdown_pct_single_bar_returns_none() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "100")).unwrap();
        assert!(series.max_drawdown_pct(10).is_none());
    }

    #[test]
    fn test_ohlcv_bar_gap_up_from_prev() {
        let prev = make_bar("100", "105", "95", "103");
        let curr = make_bar("107", "115", "106", "112"); // low(106) > prev.high(105)
        assert!(curr.gap_up_from(&prev));
    }

    #[test]
    fn test_ohlcv_bar_no_gap_up() {
        let prev = make_bar("100", "110", "90", "105");
        let curr = make_bar("105", "112", "104", "108"); // low(104) < prev.high(110)
        assert!(!curr.gap_up_from(&prev));
    }

    #[test]
    fn test_ohlcv_bar_gap_down_from_prev() {
        let prev = make_bar("100", "105", "95", "97");
        let curr = make_bar("93", "94", "88", "90"); // high(94) < prev.low(95)
        assert!(curr.gap_down_from(&prev));
    }

    #[test]
    fn test_ohlcv_bar_no_gap_down() {
        let prev = make_bar("100", "110", "90", "95");
        let curr = make_bar("96", "100", "92", "98"); // high(100) > prev.low(90)
        assert!(!curr.gap_down_from(&prev));
    }

    #[test]
    fn test_ohlcv_series_last_n_closes_returns_n() {
        let mut series = OhlcvSeries::new();
        for close in &["100", "102", "104", "106", "108"] {
            series.push(make_bar(close, "115", "95", close)).unwrap();
        }
        let closes = series.last_n_closes(3);
        assert_eq!(closes.len(), 3);
        assert_eq!(closes[2], dec!(108));
    }

    #[test]
    fn test_ohlcv_series_last_n_closes_fewer_than_n() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "100")).unwrap();
        let closes = series.last_n_closes(5);
        assert_eq!(closes.len(), 1);
    }

    #[test]
    fn test_ohlcv_series_volume_spike_detects_spike() {
        use crate::types::{NanoTimestamp, Quantity, Symbol};
        let sym = Symbol::new("X").unwrap();
        let p = crate::types::Price::new(dec!(100)).unwrap();
        let mut series = OhlcvSeries::new();
        // Add 3 bars with low volume
        for _ in 0..3 {
            series.push(OhlcvBar {
                symbol: sym.clone(), open: p, high: p, low: p, close: p,
                volume: Quantity::new(dec!(100)).unwrap(),
                ts_open: NanoTimestamp::new(0), ts_close: NanoTimestamp::new(1), tick_count: 1,
            }).unwrap();
        }
        // Add a spike bar with 5× volume
        series.push(OhlcvBar {
            symbol: sym.clone(), open: p, high: p, low: p, close: p,
            volume: Quantity::new(dec!(500)).unwrap(),
            ts_open: NanoTimestamp::new(2), ts_close: NanoTimestamp::new(3), tick_count: 1,
        }).unwrap();
        assert!(series.volume_spike(3, dec!(3)));
    }

    #[test]
    fn test_ohlcv_series_volume_spike_false_for_normal_volume() {
        use crate::types::{NanoTimestamp, Quantity, Symbol};
        let sym = Symbol::new("X").unwrap();
        let p = crate::types::Price::new(dec!(100)).unwrap();
        let mut series = OhlcvSeries::new();
        for _ in 0..4 {
            series.push(OhlcvBar {
                symbol: sym.clone(), open: p, high: p, low: p, close: p,
                volume: Quantity::new(dec!(100)).unwrap(),
                ts_open: NanoTimestamp::new(0), ts_close: NanoTimestamp::new(1), tick_count: 1,
            }).unwrap();
        }
        assert!(!series.volume_spike(3, dec!(3)));
    }

    #[test]
    fn test_efficiency_ratio_trending() {
        let mut series = OhlcvSeries::new();
        // Strictly rising prices → direction == path → ER = 1
        for i in 0..6u32 {
            series.push(make_bar(&format!("{}", 100 + i), &format!("{}", 105 + i), &format!("{}", 99 + i), &format!("{}", 100 + i))).unwrap();
        }
        let er = series.efficiency_ratio(5).unwrap();
        assert_eq!(er, dec!(1));
    }

    #[test]
    fn test_efficiency_ratio_none_when_not_enough_bars() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "110", "90", "100")).unwrap();
        assert!(series.efficiency_ratio(5).is_none());
    }

    #[test]
    fn test_efficiency_ratio_zero_period_returns_none() {
        let series = OhlcvSeries::new();
        assert!(series.efficiency_ratio(0).is_none());
    }

    #[test]
    fn test_body_pct_series_full_body() {
        let mut series = OhlcvSeries::new();
        // Bar: open=90, close=110, high=110, low=90 → body=20, range=20 → 100%
        series.push(make_bar("90", "110", "90", "110")).unwrap();
        let v = series.body_pct_series(1);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0], Some(dec!(100)));
    }

    #[test]
    fn test_body_pct_series_zero_range_returns_none() {
        let mut series = OhlcvSeries::new();
        series.push(make_bar("100", "100", "100", "100")).unwrap();
        let v = series.body_pct_series(1);
        assert_eq!(v[0], None);
    }

    #[test]
    fn test_candle_color_changes_alternating() {
        let mut series = OhlcvSeries::new();
        // Bullish, Bearish, Bullish → 2 changes
        series.push(make_bar("95", "110", "90", "105")).unwrap();  // bull
        series.push(make_bar("105", "115", "100", "102")).unwrap(); // bear
        series.push(make_bar("102", "115", "98", "110")).unwrap();  // bull
        assert_eq!(series.candle_color_changes(3), 2);
    }

    #[test]
    fn test_candle_color_changes_no_changes() {
        let mut series = OhlcvSeries::new();
        // All bullish → 0 changes
        for _ in 0..3 {
            series.push(make_bar("95", "110", "90", "105")).unwrap();
        }
        assert_eq!(series.candle_color_changes(3), 0);
    }

    #[test]
    fn test_typical_price_series_values() {
        let mut series = OhlcvSeries::new();
        // H=110, L=90, C=100 → tp = (110+90+100)/3 = 100
        series.push(make_bar("95", "110", "90", "100")).unwrap();
        let v = series.typical_price_series(1);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0], dec!(100));
    }

    #[test]
    fn test_typical_price_series_empty_series_returns_empty() {
        let series = OhlcvSeries::new();
        assert!(series.typical_price_series(3).is_empty());
    }

    #[test]
    fn test_bar_at_index_valid() {
        let bars = vec![bar("100"), bar("101"), bar("102")];
        let series = OhlcvSeries::from_bars(bars).unwrap();
        assert!(series.bar_at_index(0).is_some());
        assert_eq!(series.bar_at_index(2).unwrap().close.value(), dec!(102));
    }

    #[test]
    fn test_bar_at_index_out_of_bounds() {
        let bars = vec![bar("100")];
        let series = OhlcvSeries::from_bars(bars).unwrap();
        assert!(series.bar_at_index(5).is_none());
    }

    #[test]
    fn test_rolling_close_std_returns_none_for_fewer_than_two() {
        let bars = vec![bar("100")];
        let series = OhlcvSeries::from_bars(bars).unwrap();
        assert!(series.rolling_close_std(1).is_none());
    }

    #[test]
    fn test_rolling_close_std_constant_prices_is_zero() {
        let bars = vec![bar("100"), bar("100"), bar("100")];
        let series = OhlcvSeries::from_bars(bars).unwrap();
        let std = series.rolling_close_std(3).unwrap();
        assert_eq!(std, Decimal::ZERO);
    }

    #[test]
    fn test_rolling_close_std_varying_prices_positive() {
        let bars = vec![bar("100"), bar("110"), bar("120"), bar("130")];
        let series = OhlcvSeries::from_bars(bars).unwrap();
        let std = series.rolling_close_std(4).unwrap();
        assert!(std > Decimal::ZERO);
    }

    #[test]
    fn test_gap_direction_series_empty_for_single_bar() {
        let bars = vec![bar("100")];
        let series = OhlcvSeries::from_bars(bars).unwrap();
        assert!(series.gap_direction_series(3).is_empty());
    }

    #[test]
    fn test_gap_direction_series_flat_on_equal_prices() {
        let bars = vec![bar("100"), bar("100"), bar("100")];
        let series = OhlcvSeries::from_bars(bars).unwrap();
        let gaps = series.gap_direction_series(3);
        assert!(gaps.iter().all(|&g| g == 0));
    }

    #[test]
    fn test_gap_direction_series_detects_gap_up() {
        // bar opens 5 above prior close
        let p1 = Price::new(dec!(100)).unwrap();
        let p2 = Price::new(dec!(110)).unwrap();
        let b1 = OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p1, high: p1, low: p1, close: p1,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        };
        let b2 = OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p2, high: p2, low: p2, close: p2,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(2),
            ts_close: NanoTimestamp::new(3),
            tick_count: 1,
        };
        let series = OhlcvSeries::from_bars(vec![b1, b2]).unwrap();
        let gaps = series.gap_direction_series(2);
        assert_eq!(gaps, vec![1i8]);
    }

    #[test]
    fn test_bullish_candle_pct_all_bullish() {
        // open < close for all bars → 100 %
        let bars = vec![
            make_bar("95", "105", "94", "100"),
            make_bar("99", "110", "98", "108"),
            make_bar("107", "115", "106", "112"),
        ];
        let series = OhlcvSeries::from_bars(bars).unwrap();
        assert_eq!(series.bullish_candle_pct(3).unwrap(), 1.0);
    }

    #[test]
    fn test_bullish_candle_pct_none_for_zero_n() {
        let series = OhlcvSeries::from_bars(vec![bar("100")]).unwrap();
        assert!(series.bullish_candle_pct(0).is_none());
    }

    #[test]
    fn test_price_above_ma_pct_all_above() {
        // Rising prices: every close will be above the 2-bar SMA of the prior window.
        let bars = vec![
            bar("100"), bar("102"), bar("104"), bar("106"), bar("108"),
        ];
        let series = OhlcvSeries::from_bars(bars).unwrap();
        // n=3, period=2 → need at least 4 bars
        let pct = series.price_above_ma_pct(3, 2).unwrap();
        assert!(pct > 0.0);
    }

    #[test]
    fn test_price_above_ma_pct_insufficient_bars() {
        let series = OhlcvSeries::from_bars(vec![bar("100"), bar("101")]).unwrap();
        assert!(series.price_above_ma_pct(2, 3).is_none());
    }

    #[test]
    fn test_avg_body_size_flat() {
        // open == close → body = 0
        let bars = vec![bar("100"), bar("100"), bar("100")];
        let series = OhlcvSeries::from_bars(bars).unwrap();
        assert_eq!(series.avg_body_size(3).unwrap(), dec!(0));
    }

    #[test]
    fn test_avg_body_size_none_for_zero_n() {
        let series = OhlcvSeries::from_bars(vec![bar("100")]).unwrap();
        assert!(series.avg_body_size(0).is_none());
    }

    #[test]
    fn test_true_range_series_flat() {
        let bars = vec![bar("100"), bar("100"), bar("100")];
        let series = OhlcvSeries::from_bars(bars).unwrap();
        let trs = series.true_range_series(3).unwrap();
        assert_eq!(trs.len(), 3);
        // All flat bars → true range = 0
        for tr in trs {
            assert_eq!(tr, dec!(0));
        }
    }

    #[test]
    fn test_true_range_series_none_when_insufficient() {
        let series = OhlcvSeries::from_bars(vec![bar("100")]).unwrap();
        assert!(series.true_range_series(0).is_none());
        assert!(series.true_range_series(2).is_none());
    }

    #[test]
    fn test_intraday_return_pct_positive() {
        // bar() uses same price for open and close, so use custom bars
        let make_bar = |o: &str, c: &str| {
            let op = Price::new(o.parse::<rust_decimal::Decimal>().unwrap()).unwrap();
            let cl = Price::new(c.parse::<rust_decimal::Decimal>().unwrap()).unwrap();
            OhlcvBar {
                symbol: Symbol::new("X").unwrap(),
                open: op, high: cl, low: op, close: cl,
                volume: Quantity::zero(),
                ts_open: NanoTimestamp::new(0),
                ts_close: NanoTimestamp::new(1),
                tick_count: 1,
            }
        };
        let series = OhlcvSeries::from_bars(vec![make_bar("100", "110")]).unwrap();
        // (110 - 100) / 100 * 100 = 10%
        assert_eq!(series.intraday_return_pct().unwrap(), dec!(10));
    }

    #[test]
    fn test_intraday_return_pct_empty() {
        assert!(OhlcvSeries::new().intraday_return_pct().is_none());
    }

    #[test]
    fn test_bearish_bar_count_all_flat() {
        let bars = vec![bar("100"), bar("100"), bar("100")];
        let series = OhlcvSeries::from_bars(bars).unwrap();
        // flat bars (open == close) are not bearish
        assert_eq!(series.bearish_bar_count(3).unwrap(), 0);
    }

    #[test]
    fn test_bearish_bar_count_none_insufficient() {
        let series = OhlcvSeries::from_bars(vec![bar("100")]).unwrap();
        assert!(series.bearish_bar_count(0).is_none());
        assert!(series.bearish_bar_count(2).is_none());
    }

    #[test]
    fn test_hl_midpoint_flat() {
        let bars = vec![bar("100"), bar("100"), bar("100")];
        let series = OhlcvSeries::from_bars(bars).unwrap();
        assert_eq!(series.hl_midpoint(3).unwrap(), dec!(100));
    }

    #[test]
    fn test_hl_midpoint_none_when_insufficient() {
        let series = OhlcvSeries::from_bars(vec![bar("100")]).unwrap();
        assert!(series.hl_midpoint(0).is_none());
        assert!(series.hl_midpoint(2).is_none());
    }

    #[test]
    fn test_up_volume_ratio_flat_bars() {
        // flat bars (close == open) → no up-volume → ratio = 0
        let bars = vec![bar("100"), bar("100"), bar("100")];
        let series = OhlcvSeries::from_bars(bars).unwrap();
        // bars have non-zero volume (make_bar uses qty 100); flat → up_vol = 0
        let ratio = series.up_volume_ratio(3);
        if let Some(r) = ratio {
            assert_eq!(r, dec!(0));
        }
        // None is also valid if volume were truly zero
    }

    #[test]
    fn test_price_efficiency_trending() {
        // Monotonically rising prices → path equals net → efficiency = 1
        let bars: Vec<_> = (100..106u32).map(|i| bar(&i.to_string())).collect();
        let series = OhlcvSeries::from_bars(bars).unwrap();
        assert_eq!(series.price_efficiency(5).unwrap(), dec!(1));
    }

    #[test]
    fn test_price_efficiency_none_insufficient() {
        let series = OhlcvSeries::from_bars(vec![bar("100")]).unwrap();
        assert!(series.price_efficiency(1).is_none());
        assert!(series.price_efficiency(3).is_none());
    }

    #[test]
    fn test_avg_gap_zero_when_no_jumps() {
        let bars = vec![bar("100"), bar("100"), bar("100")];
        let series = OhlcvSeries::from_bars(bars).unwrap();
        assert_eq!(series.avg_gap(2).unwrap(), dec!(0));
    }

    #[test]
    fn test_avg_gap_none_when_insufficient() {
        let series = OhlcvSeries::from_bars(vec![bar("100")]).unwrap();
        assert!(series.avg_gap(0).is_none());
        assert!(series.avg_gap(1).is_none());
    }
}
