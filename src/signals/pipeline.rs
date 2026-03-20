//! Signal pipeline: applies multiple signals to each OHLCV bar and collects results.

use crate::error::FinError;
use crate::ohlcv::OhlcvBar;
use crate::signals::{Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::HashMap;

/// A named map of signal output values produced by a single [`SignalPipeline::update`] call.
///
/// Keys are the signal names as returned by [`crate::signals::Signal::name`].
/// Per-signal errors are stored alongside successful values so that one failing
/// indicator does not abort the entire pipeline.
pub struct SignalMap {
    values: HashMap<String, SignalValue>,
    errors: HashMap<String, FinError>,
}

impl SignalMap {
    /// Returns the signal value for `name`, or `None` if the name is not in this map.
    pub fn get(&self, name: &str) -> Option<&SignalValue> {
        self.values.get(name)
    }

    /// Returns the per-signal error for `name`, if that signal errored this cycle.
    pub fn error(&self, name: &str) -> Option<&FinError> {
        self.errors.get(name)
    }

    /// Returns `true` if any signal produced an error this update cycle.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Returns an iterator over the names of signals that errored this cycle.
    pub fn error_names(&self) -> impl Iterator<Item = &str> {
        self.errors.keys().map(String::as_str)
    }

    /// Returns an iterator over all `(name, value)` pairs in this map.
    pub fn values(&self) -> impl Iterator<Item = (&str, &SignalValue)> {
        self.values.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Returns all signal names in this map (order unspecified).
    pub fn names(&self) -> Vec<&str> {
        self.values.keys().map(String::as_str).collect()
    }

    /// Returns the number of signal entries in this map.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Returns `true` if this map contains no signal entries.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Returns the scalar value for `name`, or `default` if absent or unavailable.
    ///
    /// Combines `.get(name)` and `.as_decimal()` in one call. Useful in hot paths
    /// where downstream logic needs a numeric fallback rather than an `Option`.
    pub fn scalar_or(&self, name: &str, default: Decimal) -> Decimal {
        self.values
            .get(name)
            .and_then(SignalValue::as_decimal)
            .unwrap_or(default)
    }

    /// Returns an iterator over `(name, Decimal)` for every signal that produced a `Scalar` value.
    pub fn scalars(&self) -> impl Iterator<Item = (&str, Decimal)> {
        self.values.iter().filter_map(|(k, v)| match v {
            SignalValue::Scalar(d) => Some((k.as_str(), *d)),
            SignalValue::Unavailable => None,
        })
    }

    /// Returns the scalar value for `name` if it exists and is ready, or `None` otherwise.
    pub fn get_scalar(&self, name: &str) -> Option<Decimal> {
        self.values.get(name)?.as_decimal()
    }

    /// Returns the `(name, value)` pair with the smallest scalar value, or `None` if no scalars.
    pub fn min_scalar(&self) -> Option<(&str, Decimal)> {
        self.scalars()
            .reduce(|acc, item| if item.1 < acc.1 { item } else { acc })
    }

    /// Returns the `(name, value)` pair with the largest scalar value, or `None` if no scalars.
    pub fn max_scalar(&self) -> Option<(&str, Decimal)> {
        self.scalars()
            .reduce(|acc, item| if item.1 > acc.1 { item } else { acc })
    }

    /// Returns the sum of all ready scalar values in this map.
    pub fn sum_scalars(&self) -> Decimal {
        self.scalars().map(|(_, v)| v).sum()
    }

    /// Collects all ready scalar values into an owned `HashMap<String, Decimal>`.
    ///
    /// Useful when the caller needs an owned snapshot of all current signal values,
    /// e.g. to send across a channel or serialize.
    pub fn get_all_scalars(&self) -> std::collections::HashMap<String, Decimal> {
        self.scalars()
            .map(|(name, val)| (name.to_owned(), val))
            .collect()
    }

    /// Returns a `Vec` of names of all signals that errored in this update cycle.
    pub fn names_with_errors(&self) -> Vec<&str> {
        self.errors.keys().map(String::as_str).collect()
    }

    /// Returns the number of signals that produced a `Scalar` value this update cycle.
    pub fn count_ready(&self) -> usize {
        self.scalars().count()
    }

    /// Returns the arithmetic mean of all ready scalar values, or `None` if there are none.
    pub fn average_scalar(&self) -> Option<Decimal> {
        let mut count = 0usize;
        let mut sum = Decimal::ZERO;
        for (_, v) in self.scalars() {
            sum += v;
            count += 1;
        }
        if count == 0 {
            None
        } else {
            #[allow(clippy::cast_possible_truncation)]
            Some(sum / Decimal::from(count as u64))
        }
    }

    /// Returns a `HashMap` of signal names to scalar values for all signals whose scalar
    /// value is strictly greater than `threshold`.
    ///
    /// Signals that are `Unavailable` or whose value does not exceed the threshold are excluded.
    pub fn filter_scalars_above(&self, threshold: Decimal) -> std::collections::HashMap<&str, Decimal> {
        self.scalars()
            .filter(|(_, v)| *v > threshold)
            .collect()
    }

    /// Returns a `HashMap` of signal names to scalar values for all signals whose scalar
    /// value is strictly less than `threshold`.
    pub fn filter_scalars_below(&self, threshold: Decimal) -> std::collections::HashMap<&str, Decimal> {
        self.scalars()
            .filter(|(_, v)| *v < threshold)
            .collect()
    }

    /// Returns a `HashMap` of signal names to scalar values for signals whose value
    /// falls within `[lo, hi]` (inclusive on both ends).
    pub fn scalars_in_range(&self, lo: Decimal, hi: Decimal) -> std::collections::HashMap<&str, Decimal> {
        self.scalars()
            .filter(|(_, v)| *v >= lo && *v <= hi)
            .collect()
    }

    /// Returns the number of scalars strictly above `threshold`.
    pub fn above_count(&self, threshold: Decimal) -> usize {
        self.scalars().filter(|(_, v)| *v > threshold).count()
    }

    /// Returns the number of scalars strictly below `threshold`.
    pub fn below_count(&self, threshold: Decimal) -> usize {
        self.scalars().filter(|(_, v)| *v < threshold).count()
    }

    /// Returns the median scalar value across all available scalars.
    ///
    /// Returns `None` if there are no scalar values.
    pub fn median_scalar(&self) -> Option<Decimal> {
        let mut vals: Vec<Decimal> = self.scalars().map(|(_, v)| v).collect();
        if vals.is_empty() {
            return None;
        }
        vals.sort();
        let mid = vals.len() / 2;
        if vals.len() % 2 == 0 {
            Some((vals[mid - 1] + vals[mid]) / Decimal::TWO)
        } else {
            Some(vals[mid])
        }
    }
}

/// A pipeline that applies a sequence of signals to each incoming OHLCV bar.
pub struct SignalPipeline {
    signals: Vec<Box<dyn Signal>>,
}

impl SignalPipeline {
    /// Creates an empty `SignalPipeline`.
    pub fn new() -> Self {
        Self {
            signals: Vec::new(),
        }
    }

    /// Adds a signal to the pipeline (builder pattern).
    #[must_use]
    #[allow(clippy::should_implement_trait)]
    pub fn add(mut self, signal: impl Signal + 'static) -> Self {
        self.signals.push(Box::new(signal));
        self
    }

    /// Updates all signals with `bar` and returns a [`SignalMap`] of all results.
    ///
    /// Each signal is evaluated independently. Per-signal arithmetic errors are
    /// stored in the returned `SignalMap` (accessible via `map.error(name)`) rather
    /// than aborting the pipeline. Erroring signals appear as `Unavailable` in the map.
    pub fn update(&mut self, bar: &OhlcvBar) -> SignalMap {
        self.update_bar_input(&crate::signals::BarInput::from(bar))
    }

    /// Update all signals from a [`BarInput`] directly, without requiring an [`OhlcvBar`].
    ///
    /// Use this variant when working with synthetic or non-OHLCV data sources that
    /// already produce [`BarInput`] (e.g. custom tick aggregators, external feeds).
    pub fn update_bar_input(&mut self, bar: &crate::signals::BarInput) -> SignalMap {
        let mut values = HashMap::with_capacity(self.signals.len());
        let mut errors = HashMap::new();
        for signal in &mut self.signals {
            let name = signal.name().to_owned();
            match signal.update(bar) {
                Ok(value) => {
                    values.insert(name, value);
                }
                Err(e) => {
                    values.insert(name.clone(), SignalValue::Unavailable);
                    errors.insert(name, e);
                }
            }
        }
        SignalMap { values, errors }
    }

    /// Returns an iterator over the names of all registered signals in insertion order.
    pub fn signal_names(&self) -> impl Iterator<Item = &str> {
        self.signals.iter().map(|s| s.name())
    }

    /// Returns the total number of registered signals.
    pub fn signal_count(&self) -> usize {
        self.signals.len()
    }

    /// Returns the number of registered signals (alias for `signal_count`).
    pub fn len(&self) -> usize {
        self.signals.len()
    }

    /// Returns `true` if no signals are registered.
    pub fn is_empty(&self) -> bool {
        self.signals.is_empty()
    }

    /// Returns the number of signals that are currently ready.
    pub fn ready_count(&self) -> usize {
        self.signals.iter().filter(|s| s.is_ready()).count()
    }

    /// Returns the number of signals that are still warming up (not yet ready).
    pub fn not_ready_count(&self) -> usize {
        self.signals.iter().filter(|s| !s.is_ready()).count()
    }

    /// Returns `true` if every registered signal is ready to produce values.
    ///
    /// Useful as a gate before using pipeline output in production logic:
    /// only act on signals once `all_ready()` returns `true`.
    pub fn all_ready(&self) -> bool {
        !self.signals.is_empty() && self.signals.iter().all(|s| s.is_ready())
    }

    /// Returns an iterator over the names of signals that are currently ready.
    ///
    /// Useful for selectively reading only warmed-up signals from the output map.
    pub fn names_ready(&self) -> impl Iterator<Item = &str> {
        self.signals
            .iter()
            .filter(|s| s.is_ready())
            .map(|s| s.name())
    }

    /// Returns a reference to the signal with the given `name`, or `None` if not registered.
    pub fn get_signal(&self, name: &str) -> Option<&dyn Signal> {
        self.signals
            .iter()
            .find(|s| s.name() == name)
            .map(|s| s.as_ref())
    }

    /// Resets all registered signals to their initial (warm-up) state.
    ///
    /// Equivalent to calling `signal.reset()` on each registered signal.
    /// Useful for walk-forward backtesting without rebuilding the pipeline.
    pub fn reset(&mut self) {
        for signal in &mut self.signals {
            signal.reset();
        }
    }

    /// Updates all signals for every bar in `series`, returning one [`SignalMap`] per bar.
    ///
    /// The output vector has the same length as `series`. Useful for batch-processing
    /// a historical series in one call before inspecting the final state.
    pub fn update_series(&mut self, series: &crate::ohlcv::OhlcvSeries) -> Vec<SignalMap> {
        series.bars().iter().map(|bar| self.update(bar)).collect()
    }

    /// Runs all bars in `series` through the pipeline discarding outputs.
    ///
    /// Use this to warm up signal state before starting live updates,
    /// without allocating a `Vec<SignalMap>`.
    pub fn warm_up_bars(&mut self, series: &crate::ohlcv::OhlcvSeries) {
        for bar in series.bars() {
            self.update(bar);
        }
    }

    /// Resets all signals in the pipeline to their initial (un-warmed) state.
    pub fn reset_all(&mut self) {
        for signal in &mut self.signals {
            signal.reset();
        }
    }

    /// Returns the names of signals that are currently ready (producing values).
    ///
    /// A signal is "ready" when it has accumulated enough bars.
    pub fn ready_signal_names(&self) -> Vec<&str> {
        self.signals
            .iter()
            .filter(|s| s.is_ready())
            .map(|s| s.name())
            .collect()
    }

    /// Retains only the signals for which `predicate` returns `true`.
    ///
    /// Signals for which `predicate` returns `false` are dropped in place.
    /// Useful for culling a pipeline by period, name pattern, or readiness.
    pub fn retain<F>(&mut self, mut predicate: F)
    where
        F: FnMut(&dyn Signal) -> bool,
    {
        self.signals.retain(|s| predicate(s.as_ref()));
    }

    /// Returns the `(name, period)` pairs for every registered signal, in insertion order.
    pub fn signal_periods(&self) -> Vec<(&str, usize)> {
        self.signals.iter().map(|s| (s.name(), s.period())).collect()
    }

    /// Returns the names of all registered signals in insertion order.
    pub fn names(&self) -> Vec<&str> {
        self.signals.iter().map(|s| s.name()).collect()
    }

    /// Returns `(name, bars_remaining)` for each signal not yet ready.
    ///
    /// `bars_remaining` is an estimate: `signal.period().saturating_sub(ready_count)`.
    /// Returns an empty `Vec` once all signals are ready.
    pub fn warmup_periods_remaining(&self) -> Vec<(&str, usize)> {
        self.signals
            .iter()
            .filter(|s| !s.is_ready())
            .map(|s| (s.name(), s.period()))
            .collect()
    }

    /// Returns a sorted `Vec<&str>` of all registered signal names.
    pub fn names_sorted(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.signals.iter().map(|s| s.name()).collect();
        names.sort_unstable();
        names
    }

    /// Returns the maximum `period()` across all registered signals, or `0` if the pipeline is empty.
    pub fn longest_period(&self) -> usize {
        self.signals.iter().map(|s| s.period()).max().unwrap_or(0)
    }

    /// Returns the minimum `period()` across all registered signals, or `0` if the pipeline is empty.
    pub fn shortest_period(&self) -> usize {
        self.signals.iter().map(|s| s.period()).min().unwrap_or(0)
    }

    /// Removes the signal with the given `name` from the pipeline, returning `true` if found.
    ///
    /// If multiple signals share the same name (not recommended), only the first is removed.
    pub fn remove(&mut self, name: &str) -> bool {
        if let Some(pos) = self.signals.iter().position(|s| s.name() == name) {
            self.signals.remove(pos);
            true
        } else {
            false
        }
    }

    /// Returns the fraction of signals that are currently ready, in `[0.0, 1.0]`.
    ///
    /// Returns `0.0` if the pipeline is empty.
    pub fn pct_ready(&self) -> f64 {
        if self.signals.is_empty() {
            return 0.0;
        }
        self.ready_count() as f64 / self.signals.len() as f64
    }

}

impl Default for SignalPipeline {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::{OhlcvBar, OhlcvSeries};
    use crate::signals::indicators::{Ema, Rsi, Sma};
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(close: &str) -> OhlcvBar {
        let p = Price::new(close.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p,
            high: p,
            low: p,
            close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_signal_pipeline_update_all() {
        let mut pipeline = SignalPipeline::new()
            .add(Sma::new("sma3", 3).unwrap())
            .add(Ema::new("ema3", 3).unwrap())
            .add(Rsi::new("rsi3", 3).unwrap());

        let prices = ["100", "102", "104", "106"];
        let mut last_map = None;
        for p in &prices {
            last_map = Some(pipeline.update(&bar(p)));
        }
        let map = last_map.unwrap();
        assert!(map.get("sma3").is_some());
        assert!(map.get("ema3").is_some());
        assert!(map.get("rsi3").is_some());
        assert!(!map.has_errors());
        assert_eq!(pipeline.ready_count(), 3);
    }

    #[test]
    fn test_signal_pipeline_ready_count_zero_initially() {
        let pipeline = SignalPipeline::new()
            .add(Sma::new("sma5", 5).unwrap())
            .add(Ema::new("ema5", 5).unwrap());
        assert_eq!(pipeline.ready_count(), 0);
    }

    #[test]
    fn test_signal_pipeline_empty_map_for_empty_pipeline() {
        let mut pipeline = SignalPipeline::new();
        let map = pipeline.update(&bar("100"));
        assert!(map.get("any").is_none());
        assert!(!map.has_errors());
    }

    #[test]
    fn test_signal_pipeline_signal_names() {
        let pipeline = SignalPipeline::new()
            .add(Sma::new("sma3", 3).unwrap())
            .add(Ema::new("ema5", 5).unwrap());
        let names: Vec<&str> = pipeline.signal_names().collect();
        assert_eq!(names, vec!["sma3", "ema5"]);
    }

    #[test]
    fn test_signal_pipeline_signal_count() {
        let pipeline = SignalPipeline::new()
            .add(Sma::new("a", 2).unwrap())
            .add(Rsi::new("b", 3).unwrap());
        assert_eq!(pipeline.signal_count(), 2);
    }

    #[test]
    fn test_signal_pipeline_no_errors_on_normal_input() {
        let mut pipeline = SignalPipeline::new()
            .add(Sma::new("sma3", 3).unwrap())
            .add(Rsi::new("rsi3", 3).unwrap());
        for p in &["100", "101", "102", "103"] {
            let map = pipeline.update(&bar(p));
            assert!(!map.has_errors());
        }
    }

    #[test]
    fn test_signal_map_scalars_yields_ready_values() {
        let mut pipeline = SignalPipeline::new()
            .add(Sma::new("sma3", 3).unwrap())
            .add(Ema::new("ema3", 3).unwrap());
        pipeline.update(&bar("100"));
        pipeline.update(&bar("102"));
        let map = pipeline.update(&bar("104"));
        let scalars: Vec<_> = map.scalars().collect();
        assert_eq!(scalars.len(), 2);
        let names: Vec<_> = scalars.iter().map(|(k, _)| *k).collect();
        assert!(names.contains(&"sma3"));
        assert!(names.contains(&"ema3"));
    }

    #[test]
    fn test_signal_map_scalars_empty_before_warmup() {
        let mut pipeline = SignalPipeline::new().add(Sma::new("sma5", 5).unwrap());
        let map = pipeline.update(&bar("100")); // only 1 bar
        let scalars: Vec<_> = map.scalars().collect();
        assert!(scalars.is_empty());
    }

    #[test]
    fn test_pipeline_get_signal_found() {
        let pipeline = SignalPipeline::new()
            .add(Sma::new("sma3", 3).unwrap())
            .add(Ema::new("ema5", 5).unwrap());
        assert!(pipeline.get_signal("sma3").is_some());
        assert_eq!(pipeline.get_signal("sma3").unwrap().name(), "sma3");
    }

    #[test]
    fn test_pipeline_get_signal_not_found() {
        let pipeline = SignalPipeline::new().add(Sma::new("sma3", 3).unwrap());
        assert!(pipeline.get_signal("nonexistent").is_none());
    }

    #[test]
    fn test_pipeline_get_signal_returns_correct_period() {
        let pipeline = SignalPipeline::new()
            .add(Sma::new("sma10", 10).unwrap())
            .add(Ema::new("ema20", 20).unwrap());
        assert_eq!(pipeline.get_signal("ema20").unwrap().period(), 20);
    }

    #[test]
    fn test_signal_map_get_scalar_returns_value_when_ready() {
        let mut pipeline = SignalPipeline::new().add(Sma::new("sma3", 3).unwrap());
        pipeline.update(&bar("100"));
        pipeline.update(&bar("102"));
        let map = pipeline.update(&bar("104"));
        let v = map.get_scalar("sma3").unwrap();
        assert_eq!(v, dec!(102)); // (100 + 102 + 104) / 3
    }

    #[test]
    fn test_signal_map_get_scalar_returns_none_before_warmup() {
        let mut pipeline = SignalPipeline::new().add(Sma::new("sma5", 5).unwrap());
        let map = pipeline.update(&bar("100"));
        assert!(map.get_scalar("sma5").is_none());
    }

    #[test]
    fn test_signal_map_get_scalar_missing_name() {
        let mut pipeline = SignalPipeline::new().add(Sma::new("sma3", 3).unwrap());
        let map = pipeline.update(&bar("100"));
        assert!(map.get_scalar("nonexistent").is_none());
    }

    #[test]
    fn test_signal_map_min_max_scalar() {
        let mut pipeline = SignalPipeline::new()
            .add(Sma::new("sma2", 2).unwrap())
            .add(Sma::new("sma3", 3).unwrap());
        pipeline.update(&bar("100"));
        pipeline.update(&bar("102"));
        let map = pipeline.update(&bar("106"));
        // sma2 = (102+106)/2 = 104; sma3 = (100+102+106)/3 = 102.666...
        let (min_name, min_val) = map.min_scalar().unwrap();
        let (max_name, max_val) = map.max_scalar().unwrap();
        assert!(min_val < max_val);
        assert_ne!(min_name, max_name);
    }

    #[test]
    fn test_signal_map_min_max_scalar_empty() {
        let mut pipeline = SignalPipeline::new().add(Sma::new("sma5", 5).unwrap());
        let map = pipeline.update(&bar("100"));
        assert!(map.min_scalar().is_none());
        assert!(map.max_scalar().is_none());
    }

    #[test]
    fn test_signal_map_sum_scalars() {
        let mut pipeline = SignalPipeline::new()
            .add(Sma::new("sma2", 2).unwrap())
            .add(Sma::new("sma3", 3).unwrap());
        pipeline.update(&bar("100"));
        pipeline.update(&bar("100"));
        let map = pipeline.update(&bar("100"));
        // Both SMAs = 100
        assert_eq!(map.sum_scalars(), dec!(200));
    }

    #[test]
    fn test_signal_map_sum_scalars_before_warmup() {
        let mut pipeline = SignalPipeline::new().add(Sma::new("sma5", 5).unwrap());
        let map = pipeline.update(&bar("100"));
        assert_eq!(map.sum_scalars(), dec!(0));
    }

    #[test]
    fn test_signal_pipeline_update_series_length_matches() {
        use crate::ohlcv::{OhlcvBar, OhlcvSeries};
        let bars: Vec<OhlcvBar> = ["100", "102", "104", "106", "108"]
            .iter()
            .map(|p| bar(p))
            .collect();
        let series = OhlcvSeries::from_bars(bars).unwrap();
        let mut pipeline = SignalPipeline::new().add(Sma::new("sma3", 3).unwrap());
        let maps = pipeline.update_series(&series);
        assert_eq!(maps.len(), 5);
    }

    #[test]
    fn test_signal_map_get_all_scalars_returns_owned_map() {
        let mut pipeline = SignalPipeline::new()
            .add(Sma::new("sma3", 3).unwrap())
            .add(Ema::new("ema3", 3).unwrap());
        pipeline.update(&bar("100"));
        pipeline.update(&bar("102"));
        let map = pipeline.update(&bar("104"));
        let scalars = map.get_all_scalars();
        assert_eq!(scalars.len(), 2);
        assert!(scalars.contains_key("sma3"));
        assert!(scalars.contains_key("ema3"));
    }

    #[test]
    fn test_signal_map_get_all_scalars_empty_before_warmup() {
        let mut pipeline = SignalPipeline::new().add(Sma::new("sma5", 5).unwrap());
        let map = pipeline.update(&bar("100"));
        assert!(map.get_all_scalars().is_empty());
    }

    #[test]
    fn test_signal_pipeline_update_series_last_map_has_value() {
        use crate::ohlcv::{OhlcvBar, OhlcvSeries};
        let bars: Vec<OhlcvBar> = ["100", "100", "100", "100"]
            .iter()
            .map(|p| bar(p))
            .collect();
        let series = OhlcvSeries::from_bars(bars).unwrap();
        let mut pipeline = SignalPipeline::new().add(Sma::new("sma3", 3).unwrap());
        let maps = pipeline.update_series(&series);
        assert_eq!(maps.last().unwrap().get_scalar("sma3"), Some(dec!(100)));
    }

    #[test]
    fn test_signal_map_names_with_errors_empty_when_no_errors() {
        let mut pipeline = SignalPipeline::new().add(Sma::new("sma3", 3).unwrap());
        let map = pipeline.update(&bar("100"));
        assert!(map.names_with_errors().is_empty());
    }

    #[test]
    fn test_signal_pipeline_warm_up_bars_advances_state() {
        let bars: Vec<OhlcvBar> = ["100", "101", "102", "103", "104"]
            .iter()
            .map(|p| bar(p))
            .collect();
        let series = OhlcvSeries::from_bars(bars).unwrap();
        let mut pipeline = SignalPipeline::new().add(Sma::new("sma3", 3).unwrap());
        pipeline.warm_up_bars(&series);
        // After 5 bars the SMA(3) should be ready; next update should yield a scalar
        let map = pipeline.update(&bar("100"));
        assert!(map.get_scalar("sma3").is_some());
    }

    #[test]
    fn test_signal_pipeline_warm_up_bars_fewer_bars_than_period() {
        let bars: Vec<OhlcvBar> = vec![bar("100")];
        let series = OhlcvSeries::from_bars(bars).unwrap();
        let mut pipeline = SignalPipeline::new().add(Sma::new("sma3", 3).unwrap());
        // Should not panic even when series has fewer bars than period
        pipeline.warm_up_bars(&series);
        let map = pipeline.update(&bar("100"));
        // Only 2 bars total — still below period of 3
        assert!(map.get_scalar("sma3").is_none());
    }

    #[test]
    fn test_signal_pipeline_reset_all_clears_state() {
        let mut pipeline = SignalPipeline::new().add(Sma::new("sma3", 3).unwrap());
        pipeline.update(&bar("100"));
        pipeline.update(&bar("101"));
        pipeline.update(&bar("102")); // now ready
        pipeline.reset_all();
        // After reset, signal should not be ready
        let map = pipeline.update(&bar("103"));
        assert!(map.get_scalar("sma3").is_none());
    }

    #[test]
    fn test_signal_pipeline_ready_signal_names_empty_before_warmup() {
        let mut pipeline = SignalPipeline::new()
            .add(Sma::new("sma3", 3).unwrap())
            .add(Sma::new("sma5", 5).unwrap());
        pipeline.update(&bar("100"));
        assert!(pipeline.ready_signal_names().is_empty());
    }

    #[test]
    fn test_signal_pipeline_ready_signal_names_after_warmup() {
        let mut pipeline = SignalPipeline::new()
            .add(Sma::new("sma3", 3).unwrap())
            .add(Sma::new("sma5", 5).unwrap());
        for i in 0..3 {
            let p = format!("{}", 100 + i);
            pipeline.update(&bar(&p));
        }
        let names = pipeline.ready_signal_names();
        assert_eq!(names, vec!["sma3"]);
    }

    #[test]
    fn test_signal_pipeline_remove_existing_signal() {
        let mut pipeline = SignalPipeline::new()
            .add(Sma::new("sma3", 3).unwrap())
            .add(Ema::new("ema5", 5).unwrap());
        assert!(pipeline.remove("sma3"));
        assert_eq!(pipeline.signal_count(), 1);
        assert!(pipeline.get_signal("sma3").is_none());
        assert!(pipeline.get_signal("ema5").is_some());
    }

    #[test]
    fn test_signal_pipeline_remove_nonexistent_returns_false() {
        let mut pipeline = SignalPipeline::new().add(Sma::new("sma3", 3).unwrap());
        assert!(!pipeline.remove("nonexistent"));
        assert_eq!(pipeline.signal_count(), 1);
    }

    #[test]
    fn test_signal_pipeline_remove_then_update_only_remaining() {
        let mut pipeline = SignalPipeline::new()
            .add(Sma::new("sma3", 3).unwrap())
            .add(Ema::new("ema3", 3).unwrap());
        pipeline.remove("sma3");
        pipeline.update(&bar("100"));
        pipeline.update(&bar("102"));
        let map = pipeline.update(&bar("104"));
        assert!(map.get("sma3").is_none());
        assert!(map.get("ema3").is_some());
    }

    #[test]
    fn test_signal_map_filter_scalars_above_returns_matching() {
        let mut pipeline = SignalPipeline::new()
            .add(Sma::new("sma2", 2).unwrap())
            .add(Sma::new("sma3", 3).unwrap());
        pipeline.update(&bar("100"));
        pipeline.update(&bar("110"));
        let map = pipeline.update(&bar("120"));
        // sma2 = (110+120)/2 = 115; sma3 = (100+110+120)/3 = 110
        let above = map.filter_scalars_above(dec!(112));
        assert_eq!(above.len(), 1);
        assert!(above.contains_key("sma2"));
    }

    #[test]
    fn test_signal_map_filter_scalars_above_empty_when_none_qualify() {
        let mut pipeline = SignalPipeline::new().add(Sma::new("sma3", 3).unwrap());
        pipeline.update(&bar("100"));
        pipeline.update(&bar("100"));
        let map = pipeline.update(&bar("100"));
        let above = map.filter_scalars_above(dec!(200));
        assert!(above.is_empty());
    }

    #[test]
    fn test_signal_map_filter_scalars_above_excludes_unavailable() {
        let mut pipeline = SignalPipeline::new().add(Sma::new("sma5", 5).unwrap());
        let map = pipeline.update(&bar("100")); // not yet ready
        let above = map.filter_scalars_above(dec!(0));
        assert!(above.is_empty());
    }

    #[test]
    fn test_signal_map_count_ready_zero_before_warmup() {
        let mut pipeline = SignalPipeline::new().add(Sma::new("sma5", 5).unwrap());
        let map = pipeline.update(&bar("100"));
        assert_eq!(map.count_ready(), 0);
    }

    #[test]
    fn test_signal_map_count_ready_after_warmup() {
        let mut pipeline = SignalPipeline::new()
            .add(Sma::new("sma3", 3).unwrap())
            .add(Ema::new("ema3", 3).unwrap());
        pipeline.update(&bar("100"));
        pipeline.update(&bar("101"));
        let map = pipeline.update(&bar("102"));
        assert_eq!(map.count_ready(), 2);
    }

    #[test]
    fn test_signal_map_average_scalar_returns_none_when_empty() {
        let mut pipeline = SignalPipeline::new().add(Sma::new("sma5", 5).unwrap());
        let map = pipeline.update(&bar("100"));
        assert!(map.average_scalar().is_none());
    }

    #[test]
    fn test_signal_map_average_scalar_single_value() {
        let mut pipeline = SignalPipeline::new().add(Sma::new("sma3", 3).unwrap());
        pipeline.update(&bar("100"));
        pipeline.update(&bar("100"));
        let map = pipeline.update(&bar("100"));
        assert_eq!(map.average_scalar(), Some(dec!(100)));
    }

    #[test]
    fn test_signal_map_average_scalar_multiple_values() {
        let mut pipeline = SignalPipeline::new()
            .add(Sma::new("sma2", 2).unwrap())
            .add(Sma::new("sma3", 3).unwrap());
        pipeline.update(&bar("100"));
        pipeline.update(&bar("100"));
        let map = pipeline.update(&bar("100")); // both SMAs = 100
        assert_eq!(map.average_scalar(), Some(dec!(100)));
    }

    #[test]
    fn test_signal_pipeline_retain_by_period() {
        let mut pipeline = SignalPipeline::new()
            .add(Sma::new("sma3", 3).unwrap())
            .add(Sma::new("sma5", 5).unwrap())
            .add(Ema::new("ema10", 10).unwrap());
        pipeline.retain(|s| s.period() <= 5);
        assert_eq!(pipeline.signal_count(), 2);
        assert!(pipeline.get_signal("ema10").is_none());
    }

    #[test]
    fn test_signal_pipeline_retain_all_pass() {
        let mut pipeline = SignalPipeline::new()
            .add(Sma::new("sma3", 3).unwrap())
            .add(Ema::new("ema5", 5).unwrap());
        pipeline.retain(|_| true);
        assert_eq!(pipeline.signal_count(), 2);
    }

    #[test]
    fn test_signal_pipeline_retain_none_pass() {
        let mut pipeline = SignalPipeline::new()
            .add(Sma::new("sma3", 3).unwrap())
            .add(Ema::new("ema5", 5).unwrap());
        pipeline.retain(|_| false);
        assert_eq!(pipeline.signal_count(), 0);
    }

    #[test]
    fn test_signal_pipeline_signal_periods() {
        let pipeline = SignalPipeline::new()
            .add(Sma::new("sma3", 3).unwrap())
            .add(Ema::new("ema5", 5).unwrap())
            .add(Rsi::new("rsi14", 14).unwrap());
        let periods = pipeline.signal_periods();
        assert_eq!(periods.len(), 3);
        assert_eq!(periods[0], ("sma3", 3));
        assert_eq!(periods[1], ("ema5", 5));
        assert_eq!(periods[2], ("rsi14", 14));
    }

    #[test]
    fn test_signal_pipeline_signal_periods_empty() {
        let pipeline = SignalPipeline::new();
        assert!(pipeline.signal_periods().is_empty());
    }

    #[test]
    fn test_signal_pipeline_names_sorted() {
        let pipeline = SignalPipeline::new()
            .add(Sma::new("zzz", 3).unwrap())
            .add(Ema::new("aaa", 5).unwrap())
            .add(Rsi::new("mmm", 7).unwrap());
        let names = pipeline.names_sorted();
        assert_eq!(names, vec!["aaa", "mmm", "zzz"]);
    }

    #[test]
    fn test_signal_pipeline_longest_shortest_period() {
        let pipeline = SignalPipeline::new()
            .add(Sma::new("s3", 3).unwrap())
            .add(Ema::new("e10", 10).unwrap())
            .add(Rsi::new("r7", 7).unwrap());
        assert_eq!(pipeline.longest_period(), 10);
        assert_eq!(pipeline.shortest_period(), 3);
    }

    #[test]
    fn test_signal_pipeline_longest_shortest_empty() {
        let pipeline = SignalPipeline::new();
        assert_eq!(pipeline.longest_period(), 0);
        assert_eq!(pipeline.shortest_period(), 0);
    }
}
