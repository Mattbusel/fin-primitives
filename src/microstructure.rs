//! # Market Microstructure Anomaly Detection
//!
//! Detects manipulative order-book patterns in real time: **spoofing**,
//! **layering**, and **quote stuffing**.
//!
//! ## Background
//!
//! Spoofing, layering, and quote stuffing are illegal in most jurisdictions
//! and are commonly used to artificially move prices before reversing positions.
//!
//! | Pattern | Description |
//! |---------|-------------|
//! | **Spoofing** | Large orders placed and cancelled before execution to create a false impression of depth. |
//! | **Layering** | Multiple orders at different price levels placed and cancelled rapidly to create the illusion of a thick book. |
//! | **Quote stuffing** | Flooding the order book with tiny, rapidly cancelled orders to saturate exchange quote processing. |
//!
//! These heuristics are based on publicly documented regulatory guidance (CFTC,
//! SEC) and academic literature (Comerton-Forde & Putniņš, 2015; Lee et al., 2013).
//!
//! ## Usage
//!
//! ```rust
//! use fin_primitives::microstructure::{MicrostructureDetector, DetectorConfig, OrderEvent, OrderAction};
//! use fin_primitives::types::{Price, Quantity, Side};
//!
//! let mut detector = MicrostructureDetector::new(DetectorConfig::default());
//!
//! // Feed order events as they arrive from the exchange feed.
//! detector.on_event(OrderEvent {
//!     order_id: 1,
//!     action: OrderAction::Add,
//!     price: Price::new("100.00").unwrap(),
//!     quantity: Quantity::new("10000").unwrap(),
//!     side: Side::Bid,
//!     timestamp_ns: 0,
//! });
//!
//! detector.on_event(OrderEvent {
//!     order_id: 1,
//!     action: OrderAction::Cancel,
//!     price: Price::new("100.00").unwrap(),
//!     quantity: Quantity::new("10000").unwrap(),
//!     side: Side::Bid,
//!     timestamp_ns: 50_000_000, // 50 ms later
//! });
//!
//! let alerts = detector.drain_alerts();
//! for alert in &alerts {
//!     println!("ALERT: {:?} at order {}", alert.kind, alert.order_id);
//! }
//! ```

use std::collections::{HashMap, VecDeque};

use rust_decimal::Decimal;

use crate::types::{Price, Quantity, Side};

// ---------------------------------------------------------------------------
// OrderEvent
// ---------------------------------------------------------------------------

/// An action applied to an order in the exchange feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderAction {
    /// A new order was added to the book.
    Add,
    /// An existing order was modified (price or quantity changed).
    Modify,
    /// An existing order was cancelled before execution.
    Cancel,
    /// An order was (fully or partially) filled.
    Fill,
}

/// A single order-book event from the exchange feed.
#[derive(Debug, Clone)]
pub struct OrderEvent {
    /// Exchange-assigned order identifier.
    pub order_id: u64,
    /// What happened to this order.
    pub action: OrderAction,
    /// Order price.
    pub price: Price,
    /// Order quantity (for Cancel/Fill this is the cancelled/filled quantity).
    pub quantity: Quantity,
    /// Which side of the book.
    pub side: Side,
    /// Nanoseconds since Unix epoch.
    pub timestamp_ns: i64,
}

// ---------------------------------------------------------------------------
// Internal order state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct LiveOrder {
    order_id: u64,
    price: Price,
    quantity: Quantity,
    side: Side,
    added_at_ns: i64,
}

// ---------------------------------------------------------------------------
// AlertKind
// ---------------------------------------------------------------------------

/// The type of anomaly detected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlertKind {
    /// Large order cancelled within `spoof_cancel_window_ns`.
    Spoofing,
    /// Multiple orders at different levels cancelled in rapid succession.
    Layering,
    /// Burst of tiny cancels exceeding `stuff_rate_threshold` per second.
    QuoteStuffing,
}

impl std::fmt::Display for AlertKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AlertKind::Spoofing => write!(f, "SPOOFING"),
            AlertKind::Layering => write!(f, "LAYERING"),
            AlertKind::QuoteStuffing => write!(f, "QUOTE_STUFFING"),
        }
    }
}

// ---------------------------------------------------------------------------
// Alert
// ---------------------------------------------------------------------------

/// An anomaly alert produced by [`MicrostructureDetector`].
#[derive(Debug, Clone)]
pub struct Alert {
    /// Type of manipulative pattern detected.
    pub kind: AlertKind,
    /// Order ID associated with the alert (for layering: the triggering cancel).
    pub order_id: u64,
    /// Nanosecond timestamp when the alert was raised.
    pub timestamp_ns: i64,
    /// Human-readable description.
    pub detail: String,
}

// ---------------------------------------------------------------------------
// DetectorConfig
// ---------------------------------------------------------------------------

/// Configuration for [`MicrostructureDetector`].
#[derive(Debug, Clone)]
pub struct DetectorConfig {
    /// An order is flagged as a spoof candidate if its size (in units) exceeds
    /// this threshold.  Defaults to `1_000`.
    pub spoof_min_quantity: Decimal,

    /// Maximum nanoseconds between Add and Cancel for a spoof detection.
    /// Defaults to 500 ms.
    pub spoof_cancel_window_ns: i64,

    /// Number of levels that must be cancelled within `layer_window_ns` to
    /// trigger a layering alert.  Defaults to `3`.
    pub layer_min_levels: usize,

    /// Nanosecond window for layering detection.  Defaults to 200 ms.
    pub layer_window_ns: i64,

    /// Number of cancels per second that triggers quote-stuffing detection.
    /// Defaults to `100`.
    pub stuff_rate_threshold: usize,

    /// Nanosecond window for quote-stuffing rate calculation.  Defaults to 1 s.
    pub stuff_window_ns: i64,
}

impl Default for DetectorConfig {
    fn default() -> Self {
        Self {
            spoof_min_quantity: Decimal::from(1_000),
            spoof_cancel_window_ns: 500_000_000,     // 500 ms
            layer_min_levels: 3,
            layer_window_ns: 200_000_000,            // 200 ms
            stuff_rate_threshold: 100,
            stuff_window_ns: 1_000_000_000,          // 1 s
        }
    }
}

// ---------------------------------------------------------------------------
// MicrostructureDetector
// ---------------------------------------------------------------------------

/// Real-time market microstructure anomaly detector.
///
/// Feed [`OrderEvent`]s via [`on_event`][MicrostructureDetector::on_event];
/// consume alerts via [`drain_alerts`][MicrostructureDetector::drain_alerts].
pub struct MicrostructureDetector {
    config: DetectorConfig,
    /// Currently live orders keyed by order_id.
    live: HashMap<u64, LiveOrder>,
    /// Ring buffer of recent cancel timestamps (ns) for quote-stuffing.
    recent_cancels: VecDeque<i64>,
    /// Ring buffer of (timestamp_ns, price_level) for layering detection.
    recent_cancel_levels: VecDeque<(i64, Price)>,
    /// Accumulated alerts not yet drained.
    alerts: Vec<Alert>,
    /// Aggregate statistics.
    stats: DetectorStats,
}

impl MicrostructureDetector {
    /// Create a new detector with the given configuration.
    pub fn new(config: DetectorConfig) -> Self {
        Self {
            config,
            live: HashMap::new(),
            recent_cancels: VecDeque::new(),
            recent_cancel_levels: VecDeque::new(),
            alerts: Vec::new(),
            stats: DetectorStats::default(),
        }
    }

    /// Process one order-book event.
    pub fn on_event(&mut self, event: OrderEvent) {
        self.stats.events_total += 1;

        match event.action {
            OrderAction::Add => {
                self.live.insert(
                    event.order_id,
                    LiveOrder {
                        order_id: event.order_id,
                        price: event.price.clone(),
                        quantity: event.quantity.clone(),
                        side: event.side,
                        added_at_ns: event.timestamp_ns,
                    },
                );
            }
            OrderAction::Cancel => {
                self.stats.cancels_total += 1;
                self.check_spoof(&event);
                self.record_cancel_for_layering(&event);
                self.record_cancel_for_stuffing(&event);
                self.live.remove(&event.order_id);
            }
            OrderAction::Modify => {
                if let Some(ord) = self.live.get_mut(&event.order_id) {
                    ord.price = event.price;
                    ord.quantity = event.quantity;
                }
            }
            OrderAction::Fill => {
                self.live.remove(&event.order_id);
            }
        }
    }

    // ------------------------------------------------------------------
    // Spoof detection
    // ------------------------------------------------------------------

    fn check_spoof(&mut self, cancel: &OrderEvent) {
        let order = match self.live.get(&cancel.order_id) {
            Some(o) => o.clone(),
            None => return,
        };

        let hold_ns = cancel.timestamp_ns - order.added_at_ns;
        let size = *cancel.quantity.as_ref();
        if size >= self.config.spoof_min_quantity
            && hold_ns >= 0
            && hold_ns <= self.config.spoof_cancel_window_ns
        {
            self.alerts.push(Alert {
                kind: AlertKind::Spoofing,
                order_id: cancel.order_id,
                timestamp_ns: cancel.timestamp_ns,
                detail: format!(
                    "Large order {} ({} qty) cancelled after {} ms",
                    cancel.order_id,
                    size,
                    hold_ns / 1_000_000
                ),
            });
            self.stats.spoof_alerts += 1;
        }
    }

    // ------------------------------------------------------------------
    // Layering detection
    // ------------------------------------------------------------------

    fn record_cancel_for_layering(&mut self, cancel: &OrderEvent) {
        let now = cancel.timestamp_ns;
        let window = self.config.layer_window_ns;

        // Evict old entries.
        while self
            .recent_cancel_levels
            .front()
            .map(|(ts, _)| now - ts > window)
            .unwrap_or(false)
        {
            self.recent_cancel_levels.pop_front();
        }

        self.recent_cancel_levels
            .push_back((now, cancel.price.clone()));

        // Count distinct price levels in the window.
        let mut levels: Vec<&Price> = self
            .recent_cancel_levels
            .iter()
            .map(|(_, p)| p)
            .collect();
        levels.dedup_by(|a, b| a.as_ref() == b.as_ref());
        let distinct = levels.len();

        if distinct >= self.config.layer_min_levels {
            // Only fire one alert per window (clear to avoid alert spam).
            self.recent_cancel_levels.clear();
            self.alerts.push(Alert {
                kind: AlertKind::Layering,
                order_id: cancel.order_id,
                timestamp_ns: now,
                detail: format!(
                    "Cancels at {} distinct price levels within {} ms window",
                    distinct,
                    window / 1_000_000
                ),
            });
            self.stats.layer_alerts += 1;
        }
    }

    // ------------------------------------------------------------------
    // Quote stuffing detection
    // ------------------------------------------------------------------

    fn record_cancel_for_stuffing(&mut self, cancel: &OrderEvent) {
        let now = cancel.timestamp_ns;
        let window = self.config.stuff_window_ns;

        // Evict old cancel timestamps.
        while self
            .recent_cancels
            .front()
            .map(|ts| now - ts > window)
            .unwrap_or(false)
        {
            self.recent_cancels.pop_front();
        }

        self.recent_cancels.push_back(now);

        // Convert count in window to a per-second rate.
        let rate = self.recent_cancels.len();

        if rate >= self.config.stuff_rate_threshold {
            self.recent_cancels.clear(); // reset to avoid alert flood
            self.alerts.push(Alert {
                kind: AlertKind::QuoteStuffing,
                order_id: cancel.order_id,
                timestamp_ns: now,
                detail: format!(
                    "Quote stuffing: {} cancels in {} ms window",
                    rate,
                    window / 1_000_000
                ),
            });
            self.stats.stuff_alerts += 1;
        }
    }

    // ------------------------------------------------------------------
    // Public output
    // ------------------------------------------------------------------

    /// Drain and return all accumulated alerts since the last drain.
    pub fn drain_alerts(&mut self) -> Vec<Alert> {
        std::mem::take(&mut self.alerts)
    }

    /// Number of live orders currently tracked.
    pub fn live_order_count(&self) -> usize {
        self.live.len()
    }

    /// Aggregate detection statistics.
    pub fn stats(&self) -> &DetectorStats {
        &self.stats
    }
}

// ---------------------------------------------------------------------------
// Stats
// ---------------------------------------------------------------------------

/// Aggregate statistics for [`MicrostructureDetector`].
#[derive(Debug, Clone, Default)]
pub struct DetectorStats {
    /// Total events processed.
    pub events_total: u64,
    /// Total cancels seen.
    pub cancels_total: u64,
    /// Total spoofing alerts raised.
    pub spoof_alerts: u64,
    /// Total layering alerts raised.
    pub layer_alerts: u64,
    /// Total quote-stuffing alerts raised.
    pub stuff_alerts: u64,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use rust_decimal::Decimal;

    fn price(s: &str) -> Price {
        Price::new(s).unwrap()
    }

    fn qty(n: u64) -> Quantity {
        Quantity::new(&n.to_string()).unwrap()
    }

    fn add_event(id: u64, p: &str, q: u64, ts: i64) -> OrderEvent {
        OrderEvent {
            order_id: id,
            action: OrderAction::Add,
            price: price(p),
            quantity: qty(q),
            side: Side::Bid,
            timestamp_ns: ts,
        }
    }

    fn cancel_event(id: u64, p: &str, q: u64, ts: i64) -> OrderEvent {
        OrderEvent {
            order_id: id,
            action: OrderAction::Cancel,
            price: price(p),
            quantity: qty(q),
            side: Side::Bid,
            timestamp_ns: ts,
        }
    }

    #[test]
    fn detects_spoof_large_order_fast_cancel() {
        let cfg = DetectorConfig {
            spoof_min_quantity: Decimal::from(500),
            spoof_cancel_window_ns: 500_000_000, // 500 ms
            ..Default::default()
        };
        let mut det = MicrostructureDetector::new(cfg);

        det.on_event(add_event(1, "100.00", 5000, 0));
        det.on_event(cancel_event(1, "100.00", 5000, 100_000_000)); // 100 ms

        let alerts = det.drain_alerts();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].kind, AlertKind::Spoofing);
    }

    #[test]
    fn no_spoof_if_below_size_threshold() {
        let cfg = DetectorConfig {
            spoof_min_quantity: Decimal::from(10_000),
            ..Default::default()
        };
        let mut det = MicrostructureDetector::new(cfg);

        det.on_event(add_event(2, "100.00", 100, 0));
        det.on_event(cancel_event(2, "100.00", 100, 100_000_000));

        assert!(det.drain_alerts().is_empty());
    }

    #[test]
    fn no_spoof_if_cancel_too_slow() {
        let cfg = DetectorConfig {
            spoof_min_quantity: Decimal::from(100),
            spoof_cancel_window_ns: 100_000_000, // 100 ms
            ..Default::default()
        };
        let mut det = MicrostructureDetector::new(cfg);

        det.on_event(add_event(3, "100.00", 5000, 0));
        det.on_event(cancel_event(3, "100.00", 5000, 200_000_000)); // 200 ms — too slow

        assert!(det.drain_alerts().is_empty());
    }

    #[test]
    fn detects_layering_three_levels() {
        let cfg = DetectorConfig {
            layer_min_levels: 3,
            layer_window_ns: 500_000_000,
            spoof_min_quantity: Decimal::from(u64::MAX), // disable spoof
            ..Default::default()
        };
        let mut det = MicrostructureDetector::new(cfg);

        // Add and cancel at 3 distinct levels within 500 ms.
        for (id, price_str) in [(10, "99.00"), (11, "100.00"), (12, "101.00")] {
            det.on_event(add_event(id, price_str, 10, 0));
            det.on_event(cancel_event(id, price_str, 10, 100_000_000 * (id as i64 - 9)));
        }

        let alerts = det.drain_alerts();
        assert!(
            alerts.iter().any(|a| a.kind == AlertKind::Layering),
            "expected layering alert"
        );
    }

    #[test]
    fn detects_quote_stuffing_high_cancel_rate() {
        let cfg = DetectorConfig {
            stuff_rate_threshold: 5,
            stuff_window_ns: 1_000_000_000,
            spoof_min_quantity: Decimal::from(u64::MAX), // disable spoof
            layer_min_levels: 100,                       // disable layering
            ..Default::default()
        };
        let mut det = MicrostructureDetector::new(cfg);

        // Add and immediately cancel 6 orders within 1 second.
        for i in 0..6 {
            det.on_event(add_event(100 + i, "100.00", 1, i as i64 * 10_000_000));
            det.on_event(cancel_event(
                100 + i,
                "100.00",
                1,
                i as i64 * 10_000_000 + 1,
            ));
        }

        let alerts = det.drain_alerts();
        assert!(
            alerts.iter().any(|a| a.kind == AlertKind::QuoteStuffing),
            "expected quote stuffing alert"
        );
    }

    #[test]
    fn fill_removes_order_from_live() {
        let mut det = MicrostructureDetector::new(DetectorConfig::default());
        det.on_event(add_event(999, "100.00", 100, 0));
        assert_eq!(det.live_order_count(), 1);
        det.on_event(OrderEvent {
            order_id: 999,
            action: OrderAction::Fill,
            price: price("100.00"),
            quantity: qty(100),
            side: Side::Bid,
            timestamp_ns: 1,
        });
        assert_eq!(det.live_order_count(), 0);
    }

    #[test]
    fn stats_track_events_and_cancels() {
        let mut det = MicrostructureDetector::new(DetectorConfig::default());
        det.on_event(add_event(1, "100.00", 100, 0));
        det.on_event(cancel_event(1, "100.00", 100, 1));
        let s = det.stats();
        assert_eq!(s.events_total, 2);
        assert_eq!(s.cancels_total, 1);
    }
}
