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

    /// Returns the typical price: `(high + low + close) / 3`.
    pub fn typical_price(&self) -> Decimal {
        (self.high.value() + self.low.value() + self.close.value()) / Decimal::from(3u32)
    }

    /// Returns the price range: `high - low`.
    pub fn range(&self) -> Decimal {
        self.high.value() - self.low.value()
    }

    /// Returns `true` if `close >= open`.
    pub fn is_bullish(&self) -> bool {
        self.close.value() >= self.open.value()
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
        }
        bar
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

    /// Returns the bar at `index`, or `None` if out of bounds.
    pub fn get(&self, index: usize) -> Option<&OhlcvBar> {
        self.bars.get(index)
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

    /// Returns a `Vec` of close prices in series order.
    pub fn closes(&self) -> Vec<Decimal> {
        self.bars.iter().map(|b| b.close.value()).collect()
    }

    /// Returns a `Vec` of volumes in series order.
    pub fn volumes(&self) -> Vec<Decimal> {
        self.bars.iter().map(|b| b.volume.value()).collect()
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
}
