//! Signal pipeline: applies multiple signals to each OHLCV bar and returns a named map of results.

use crate::error::FinError;
use crate::ohlcv::OhlcvBar;
use crate::signals::{Signal, SignalValue};
use std::collections::HashMap;

/// A named map of signal output values for a single bar update.
pub struct SignalMap(HashMap<String, SignalValue>);

impl SignalMap {
    /// Returns the signal value for `name`, or `None` if the name is not in this map.
    pub fn get(&self, name: &str) -> Option<&SignalValue> {
        self.0.get(name)
    }
}

/// A pipeline that applies a sequence of signals to each incoming OHLCV bar.
pub struct SignalPipeline {
    signals: Vec<Box<dyn Signal>>,
}

impl SignalPipeline {
    /// Creates an empty `SignalPipeline`.
    pub fn new() -> Self {
        Self { signals: Vec::new() }
    }

    /// Adds a signal to the pipeline (builder pattern).
    #[allow(clippy::should_implement_trait)]
    pub fn add(mut self, signal: impl Signal + 'static) -> Self {
        self.signals.push(Box::new(signal));
        self
    }

    /// Updates all signals with `bar` and returns a [`SignalMap`] of all results.
    ///
    /// # Errors
    /// Returns [`FinError`] if any signal's arithmetic fails.
    pub fn update(&mut self, bar: &OhlcvBar) -> Result<SignalMap, FinError> {
        let mut map = HashMap::with_capacity(self.signals.len());
        for signal in &mut self.signals {
            let name = signal.name().to_owned();
            let value = signal.update(bar)?;
            map.insert(name, value);
        }
        Ok(SignalMap(map))
    }

    /// Returns the number of signals that are currently ready.
    pub fn ready_count(&self) -> usize {
        self.signals.iter().filter(|s| s.is_ready()).count()
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
            ts_open: NanoTimestamp(0),
            ts_close: NanoTimestamp(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_signal_pipeline_update_all() {
        let mut pipeline = SignalPipeline::new()
            .add(Sma::new("sma3", 3))
            .add(Ema::new("ema3", 3))
            .add(Rsi::new("rsi3", 3));

        let prices = ["100", "102", "104", "106"];
        let mut last_map = None;
        for p in &prices {
            last_map = Some(pipeline.update(&bar(p)).unwrap());
        }
        let map = last_map.unwrap();
        // After 4 bars all should be ready (period=3 + 1 extra for RSI)
        assert!(map.get("sma3").is_some());
        assert!(map.get("ema3").is_some());
        assert!(map.get("rsi3").is_some());
        assert_eq!(pipeline.ready_count(), 3);
    }

    #[test]
    fn test_signal_pipeline_ready_count_zero_initially() {
        let pipeline = SignalPipeline::new()
            .add(Sma::new("sma5", 5))
            .add(Ema::new("ema5", 5));
        assert_eq!(pipeline.ready_count(), 0);
    }

    #[test]
    fn test_signal_pipeline_empty_map_for_empty_pipeline() {
        let mut pipeline = SignalPipeline::new();
        let map = pipeline.update(&bar("100")).unwrap();
        assert!(map.get("any").is_none());
    }
}
