//! # Module: ohlcv
//!
//! ## Responsibility
//! Provides OHLCV bar data structures, timeframe definitions, tick-to-bar aggregation,
//! and an ordered bar series with window queries.
//!
//! ## Guarantees
//! - `OhlcvBar::validate()` enforces: `high >= open`, `high >= close`, `low <= open`,
//!   `low <= close`, `high >= low`
//! - `OhlcvAggregator` produces a completed bar when a tick crosses a timeframe boundary
//! - `OhlcvSeries::push` maintains insertion order
//!
//! ## NOT Responsible For
//! - Persistence
//! - Cross-symbol aggregation

use crate::error::FinError;
use crate::tick::Tick;
use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
use rust_decimal::Decimal;

/// A completed OHLCV bar for a single symbol and timeframe bucket.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
        (self.high.value() + self.low.value() + self.close.value())
            / Decimal::from(3u32)
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
}

impl Timeframe {
    /// Returns the timeframe duration in nanoseconds.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidTimeframe`] if the duration is zero.
    pub fn to_nanos(&self) -> Result<i64, FinError> {
        let secs: u64 = match self {
            Timeframe::Seconds(n) => *n as u64,
            Timeframe::Minutes(n) => *n as u64 * 60,
            Timeframe::Hours(n) => *n as u64 * 3600,
            Timeframe::Days(n) => *n as u64 * 86400,
        };
        if secs == 0 {
            return Err(FinError::InvalidTimeframe);
        }
        Ok((secs * 1_000_000_000) as i64)
    }

    /// Returns the bucket start timestamp for a given tick timestamp.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidTimeframe`] if the timeframe duration is zero.
    pub fn bucket_start(&self, ts: NanoTimestamp) -> Result<NanoTimestamp, FinError> {
        let nanos = self.to_nanos()?;
        let bucket = (ts.0 / nanos) * nanos;
        Ok(NanoTimestamp(bucket))
    }
}

/// Aggregates ticks into OHLCV bars according to a fixed timeframe.
pub struct OhlcvAggregator {
    symbol: Symbol,
    timeframe: Timeframe,
    current_bar: Option<OhlcvBar>,
    current_bucket_start: Option<NanoTimestamp>,
}

impl OhlcvAggregator {
    /// Constructs a new `OhlcvAggregator`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidTimeframe`] if the timeframe is zero-duration.
    pub fn new(symbol: Symbol, timeframe: Timeframe) -> Result<Self, FinError> {
        // Validate timeframe eagerly.
        timeframe.to_nanos()?;
        Ok(Self { symbol, timeframe, current_bar: None, current_bucket_start: None })
    }

    /// Processes a single tick, returning a completed bar when the timeframe boundary is crossed.
    ///
    /// # Returns
    /// - `Ok(Some(bar))` — a bar was completed (the tick belongs to the next bucket)
    /// - `Ok(None)` — the tick was incorporated into the current bar
    ///
    /// # Errors
    /// Returns [`FinError::InvalidTimeframe`] if `timeframe.bucket_start` fails.
    pub fn push_tick(&mut self, tick: &Tick) -> Result<Option<OhlcvBar>, FinError> {
        if tick.symbol != self.symbol {
            return Ok(None);
        }
        let bucket = self.timeframe.bucket_start(tick.timestamp)?;
        match self.current_bucket_start {
            None => {
                // First tick ever.
                self.current_bucket_start = Some(bucket);
                self.current_bar = Some(self.new_bar(tick));
                Ok(None)
            }
            Some(current_bucket) if bucket == current_bucket => {
                // Same bucket — update existing bar.
                self.update_bar(tick);
                Ok(None)
            }
            Some(_) => {
                // New bucket — complete the current bar and start a new one.
                let completed = self.current_bar.take();
                self.current_bucket_start = Some(bucket);
                self.current_bar = Some(self.new_bar(tick));
                Ok(completed)
            }
        }
    }

    /// Flushes the current partial bar, returning it if one exists.
    pub fn flush(&mut self) -> Option<OhlcvBar> {
        self.current_bucket_start = None;
        self.current_bar.take()
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
            bar.volume = Quantity::new(bar.volume.value() + tick.quantity.value())
                .unwrap_or(bar.volume);
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
            ts_open: NanoTimestamp(0),
            ts_close: NanoTimestamp(1),
            tick_count: 1,
        }
    }

    fn make_tick(sym: &str, price: &str, qty: &str, ts: i64) -> Tick {
        Tick::new(
            Symbol::new(sym).unwrap(),
            make_price(price),
            make_qty(qty),
            Side::Ask,
            NanoTimestamp(ts),
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
        // (120 + 80 + 110) / 3 = 310 / 3
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
    fn test_timeframe_bucket_start() {
        let tf = Timeframe::Seconds(60);
        let nanos_per_min = 60_000_000_000_i64;
        let ts = NanoTimestamp(nanos_per_min + 500_000_000);
        let bucket = tf.bucket_start(ts).unwrap();
        assert_eq!(bucket.0, nanos_per_min);
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

        // First minute
        let t1 = make_tick("X", "100", "1", 0);
        let t2 = make_tick("X", "105", "2", nanos_per_min / 2);
        // Second minute — triggers completion of first bar
        let t3 = make_tick("X", "110", "1", nanos_per_min + 1);

        let r1 = agg.push_tick(&t1).unwrap();
        assert!(r1.is_none());
        let r2 = agg.push_tick(&t2).unwrap();
        assert!(r2.is_none());
        let r3 = agg.push_tick(&t3).unwrap();
        let bar = r3.unwrap();
        assert_eq!(bar.open.value(), dec!(100));
        assert_eq!(bar.high.value(), dec!(105));
        assert_eq!(bar.close.value(), dec!(105));
        assert_eq!(bar.tick_count, 2);
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
        assert!(result.is_none());
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
}
