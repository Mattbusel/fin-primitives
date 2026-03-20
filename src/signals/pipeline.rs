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
}

impl Default for SignalPipeline {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
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
}
