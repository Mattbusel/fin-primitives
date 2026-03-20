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

    /// Returns an iterator over `(name, Decimal)` for every signal that produced a `Scalar` value.
    pub fn scalars(&self) -> impl Iterator<Item = (&str, Decimal)> {
        self.values.iter().filter_map(|(k, v)| match v {
            SignalValue::Scalar(d) => Some((k.as_str(), *d)),
            SignalValue::Unavailable => None,
        })
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
        let mut values = HashMap::with_capacity(self.signals.len());
        let mut errors = HashMap::new();
        for signal in &mut self.signals {
            let name = signal.name().to_owned();
            match signal.update_bar(bar) {
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

    /// Resets all registered signals to their initial (warm-up) state.
    ///
    /// Equivalent to calling `signal.reset()` on each registered signal.
    /// Useful for walk-forward backtesting without rebuilding the pipeline.
    pub fn reset(&mut self) {
        for signal in &mut self.signals {
            signal.reset();
        }
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
}
