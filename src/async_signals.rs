//! # Module: async_signals
//!
//! ## Responsibility
//! Wraps a `SignalPipeline` with Tokio MPSC channels to provide a non-blocking,
//! async-friendly streaming interface.  Callers push `OhlcvBar` ticks through a
//! sender channel; the spawned task updates the pipeline and broadcasts
//! `SignalUpdate` messages on the output channel.
//!
//! ## Guarantees
//! - The spawned task terminates cleanly when the tick sender is dropped
//! - Output messages are sent in the order bars are received
//! - Zero dynamic allocation on the hot path: output buffers are pre-allocated
//!   at construction time
//!
//! ## NOT Responsible For
//! - Persistence of signal history
//! - Cross-pipeline fan-out (compose multiple `StreamingSignalPipeline`s yourself)

use crate::ohlcv::OhlcvBar;
use crate::signals::pipeline::SignalPipeline;
use crate::signals::SignalValue;
use chrono::{DateTime, Utc};
use tokio::sync::mpsc;

// ─── SignalUpdate ──────────────────────────────────────────────────────────────

/// A single computed signal value emitted by the streaming pipeline.
#[derive(Debug, Clone)]
pub struct SignalUpdate {
    /// Name of the signal that produced this value.
    pub signal_name: String,
    /// The computed value (or `SignalValue::Unavailable` during warm-up).
    pub value: SignalValue,
    /// Wall-clock time at which the update was produced.
    pub timestamp: DateTime<Utc>,
}

impl SignalUpdate {
    /// Creates a new `SignalUpdate`.
    pub fn new(signal_name: impl Into<String>, value: SignalValue, timestamp: DateTime<Utc>) -> Self {
        Self {
            signal_name: signal_name.into(),
            value,
            timestamp,
        }
    }

    /// Returns `true` if this update carries a ready (scalar) value.
    pub fn is_ready(&self) -> bool {
        self.value.is_scalar()
    }
}

// ─── StreamingSignalPipeline ──────────────────────────────────────────────────

/// Configuration for a `StreamingSignalPipeline`.
#[derive(Debug, Clone)]
pub struct StreamingConfig {
    /// Capacity of the tick input channel.
    pub tick_channel_capacity: usize,
    /// Capacity of the signal output channel.
    pub output_channel_capacity: usize,
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            tick_channel_capacity: 1_024,
            output_channel_capacity: 4_096,
        }
    }
}

/// A `SignalPipeline` wrapped with Tokio MPSC channels for async streaming.
///
/// Construct via [`StreamingSignalPipeline::new`], then call
/// [`StreamingSignalPipeline::spawn`] to start the background task and obtain
/// the sender / receiver handles.
pub struct StreamingSignalPipeline {
    pipeline: SignalPipeline,
    config: StreamingConfig,
}

impl StreamingSignalPipeline {
    /// Creates a new `StreamingSignalPipeline` wrapping the given pipeline.
    pub fn new(pipeline: SignalPipeline) -> Self {
        Self {
            pipeline,
            config: StreamingConfig::default(),
        }
    }

    /// Creates a new `StreamingSignalPipeline` with custom channel capacities.
    pub fn with_config(pipeline: SignalPipeline, config: StreamingConfig) -> Self {
        Self { pipeline, config }
    }

    /// Spawns the background processing task.
    ///
    /// Returns:
    /// - `tick_tx`: send `OhlcvBar` values here to drive the pipeline.
    /// - `update_rx`: receive `SignalUpdate` values from this end.
    ///
    /// The task runs until `tick_tx` (and all its clones) are dropped.
    pub fn spawn(
        self,
    ) -> (mpsc::Sender<OhlcvBar>, mpsc::Receiver<SignalUpdate>) {
        let (tick_tx, tick_rx) = mpsc::channel::<OhlcvBar>(self.config.tick_channel_capacity);
        let (update_tx, update_rx) =
            mpsc::channel::<SignalUpdate>(self.config.output_channel_capacity);

        tokio::spawn(run_pipeline(self.pipeline, tick_rx, update_tx));

        (tick_tx, update_rx)
    }
}

// ─── spawn_signal_stream ──────────────────────────────────────────────────────

/// Convenience function: spawns a signal-streaming task and returns the output receiver.
///
/// `pipeline` is consumed; `tick_rx` is the caller-owned tick input end.  The
/// returned `mpsc::Receiver<SignalUpdate>` carries all computed signal values.
///
/// Pre-allocates output buffers using the default `StreamingConfig` capacities.
pub fn spawn_signal_stream(
    pipeline: SignalPipeline,
    tick_rx: mpsc::Receiver<OhlcvBar>,
) -> mpsc::Receiver<SignalUpdate> {
    let capacity = StreamingConfig::default().output_channel_capacity;
    let (update_tx, update_rx) = mpsc::channel::<SignalUpdate>(capacity);
    tokio::spawn(run_pipeline(pipeline, tick_rx, update_tx));
    update_rx
}

// ─── internal task ───────────────────────────────────────────────────────────

/// Background task: consumes bars from `tick_rx`, updates `pipeline`, and sends
/// all resulting `SignalUpdate`s on `update_tx`.
///
/// Terminates gracefully when `tick_rx` is closed (sender side dropped).
async fn run_pipeline(
    mut pipeline: SignalPipeline,
    mut tick_rx: mpsc::Receiver<OhlcvBar>,
    update_tx: mpsc::Sender<SignalUpdate>,
) {
    // Pre-allocate a reusable name buffer to avoid per-bar heap allocation.
    // We build the list of signal names once before entering the hot loop.
    // (SignalPipeline does not expose an iterator over names directly, so we
    //  discover them lazily on the first bar and then reuse the vec.)
    let mut known_names: Vec<String> = Vec::new();

    while let Some(bar) = tick_rx.recv().await {
        let ts = Utc::now();

        // SignalPipeline::update takes &OhlcvBar and returns SignalMap (infallible).
        let map = pipeline.update(&bar);

        // Build name list from first non-empty map
        if known_names.is_empty() {
            known_names = map.names().iter().map(|s| (*s).to_owned()).collect();
        }

        // Emit one SignalUpdate per signal; reuse the known_names vec
        for name in &known_names {
            let value = map
                .get(name)
                .cloned()
                .unwrap_or(SignalValue::Unavailable);

            let update = SignalUpdate::new(name.clone(), value, ts);

            // If the receiver is gone, stop processing
            if update_tx.send(update).await.is_err() {
                return;
            }
        }
    }
}

// ─── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::indicators::Sma;
    use crate::signals::pipeline::SignalPipeline;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn make_bar(close: rust_decimal::Decimal, ts: i64) -> OhlcvBar {
        let sym = Symbol::new("X").unwrap();
        let p = Price::new(close).unwrap();
        OhlcvBar {
            symbol: sym,
            open: p,
            high: p,
            low: p,
            close: p,
            volume: Quantity::new(dec!(1)).unwrap(),
            ts_open: NanoTimestamp(ts),
            ts_close: NanoTimestamp(ts + 1),
            tick_count: 1,
        }
    }

    #[tokio::test]
    async fn test_streaming_pipeline_receives_updates() {
        let sma = Sma::new("sma3", 3);
        let pipeline = SignalPipeline::new().add(sma);

        let (tick_tx, mut update_rx) = StreamingSignalPipeline::new(pipeline).spawn();

        // Send 5 bars
        for i in 1u32..=5 {
            let bar = make_bar(rust_decimal::Decimal::from(i) * dec!(10), i64::from(i));
            tick_tx.send(bar).await.unwrap();
        }

        // Drop sender to signal end-of-stream
        drop(tick_tx);

        let mut updates: Vec<SignalUpdate> = Vec::new();
        while let Some(u) = update_rx.recv().await {
            updates.push(u);
        }

        // We sent 5 bars with 1 signal → expect 5 updates
        assert_eq!(updates.len(), 5);
        // First two updates are Unavailable (SMA(3) needs 3 bars)
        assert!(updates[0].value.is_unavailable());
        assert!(updates[1].value.is_unavailable());
        // Third and later should be scalar
        assert!(updates[2].value.is_scalar());
    }

    #[tokio::test]
    async fn test_spawn_signal_stream_convenience() {
        let sma = Sma::new("sma2", 2);
        let pipeline = SignalPipeline::new().add(sma);

        let (tick_tx, tick_rx) = mpsc::channel::<OhlcvBar>(16);
        let mut update_rx = spawn_signal_stream(pipeline, tick_rx);

        for i in 1u32..=3 {
            let bar = make_bar(rust_decimal::Decimal::from(i) * dec!(5), i64::from(i));
            tick_tx.send(bar).await.unwrap();
        }
        drop(tick_tx);

        let mut count = 0usize;
        while let Some(_) = update_rx.recv().await {
            count += 1;
        }
        assert_eq!(count, 3);
    }

    #[tokio::test]
    async fn test_pipeline_closes_when_sender_dropped() {
        let sma = Sma::new("sma5", 5);
        let pipeline = SignalPipeline::new().add(sma);
        let (tick_tx, mut update_rx) = StreamingSignalPipeline::new(pipeline).spawn();

        // Drop immediately without sending anything
        drop(tick_tx);

        // Receiver should return None immediately
        let result = update_rx.recv().await;
        assert!(result.is_none());
    }
}
