//! # Module: latency
//!
//! ## Responsibility
//! Tracks order latency across the three phases of an order lifecycle:
//! submit → acknowledge → fill → book-update.
//! Provides percentile statistics (p50 / p95 / p99) for each phase.
//!
//! ## Guarantees
//! - All latencies are stored as `i64` nanoseconds; no floating-point drift in storage
//! - Percentiles are computed from sorted snapshots; no panics on empty sets
//! - Phases are tracked independently; missing phases do not corrupt other phases
//!
//! ## NOT Responsible For
//! - Clock synchronization
//! - Order routing

use crate::error::FinError;
use crate::types::NanoTimestamp;

/// Phase of an order's lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LatencyPhase {
    /// Time from order submission to exchange acknowledgement (ns).
    SubmitToAck,
    /// Time from acknowledgement to first fill (ns).
    AckToFill,
    /// Time from fill to order book update reflecting the fill (ns).
    FillToBookUpdate,
}

/// A latency measurement for one phase of a single order.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LatencySample {
    /// Which phase this measurement covers.
    pub phase: LatencyPhase,
    /// Latency in nanoseconds.
    pub latency_ns: i64,
}

/// Per-phase statistics snapshot.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PhaseStats {
    /// Number of samples recorded.
    pub count: usize,
    /// Minimum observed latency (ns).
    pub min_ns: i64,
    /// Maximum observed latency (ns).
    pub max_ns: i64,
    /// Approximate mean latency (ns).
    pub mean_ns: f64,
    /// 50th percentile (median) latency (ns).
    pub p50_ns: i64,
    /// 95th percentile latency (ns).
    pub p95_ns: i64,
    /// 99th percentile latency (ns).
    pub p99_ns: i64,
}

/// Records open orders and accumulates latency samples for each lifecycle phase.
///
/// # Example
/// ```rust
/// use fin_primitives::latency::{OrderLatencyTracker, LatencyPhase};
/// use fin_primitives::types::NanoTimestamp;
///
/// let mut tracker = OrderLatencyTracker::new();
/// let t0 = NanoTimestamp::new(1_000_000_000);
/// let t1 = NanoTimestamp::new(1_000_001_000);
/// let t2 = NanoTimestamp::new(1_000_002_500);
/// let t3 = NanoTimestamp::new(1_000_003_000);
///
/// tracker.record_submit("ord1", t0);
/// tracker.record_ack("ord1", t1).unwrap();
/// tracker.record_fill("ord1", t2).unwrap();
/// tracker.record_book_update("ord1", t3).unwrap();
///
/// let stats = tracker.stats(LatencyPhase::SubmitToAck).unwrap();
/// assert_eq!(stats.count, 1);
/// assert_eq!(stats.p50_ns, 1000);
/// ```
#[derive(Debug, Default)]
pub struct OrderLatencyTracker {
    /// Pending orders: order_id → lifecycle timestamps (submit, ack, fill, book_update).
    pending: std::collections::HashMap<String, OrderTimestamps>,
    /// Accumulated submit-to-ack latencies (ns).
    submit_to_ack: Vec<i64>,
    /// Accumulated ack-to-fill latencies (ns).
    ack_to_fill: Vec<i64>,
    /// Accumulated fill-to-book-update latencies (ns).
    fill_to_book: Vec<i64>,
}

#[derive(Debug, Clone, Default)]
struct OrderTimestamps {
    submit: Option<i64>,
    ack: Option<i64>,
    fill: Option<i64>,
}

impl OrderLatencyTracker {
    /// Creates a new empty `OrderLatencyTracker`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the submission timestamp of `order_id`.
    ///
    /// If a record for `order_id` already exists it is reset.
    pub fn record_submit(&mut self, order_id: impl Into<String>, ts: NanoTimestamp) {
        let mut ots = OrderTimestamps::default();
        ots.submit = Some(ts.nanos());
        self.pending.insert(order_id.into(), ots);
    }

    /// Records the acknowledgement timestamp of `order_id`.
    ///
    /// Stores the submit→ack latency sample.
    ///
    /// # Errors
    /// - [`FinError::InvalidInput`] if `order_id` is unknown or submit was not recorded.
    /// - [`FinError::InvalidInput`] if ack timestamp is before submit timestamp.
    pub fn record_ack(&mut self, order_id: &str, ts: NanoTimestamp) -> Result<(), FinError> {
        let rec = self
            .pending
            .get_mut(order_id)
            .ok_or_else(|| FinError::InvalidInput(format!("unknown order '{order_id}'")))?;
        let submit = rec.submit.ok_or_else(|| {
            FinError::InvalidInput(format!("submit not recorded for '{order_id}'"))
        })?;
        let ack_ns = ts.nanos();
        if ack_ns < submit {
            return Err(FinError::InvalidInput(format!(
                "ack timestamp before submit for '{order_id}'"
            )));
        }
        rec.ack = Some(ack_ns);
        self.submit_to_ack.push(ack_ns - submit);
        Ok(())
    }

    /// Records the fill timestamp of `order_id`.
    ///
    /// Stores the ack→fill latency sample.
    ///
    /// # Errors
    /// - [`FinError::InvalidInput`] if `order_id` is unknown or ack was not recorded.
    /// - [`FinError::InvalidInput`] if fill timestamp is before ack timestamp.
    pub fn record_fill(&mut self, order_id: &str, ts: NanoTimestamp) -> Result<(), FinError> {
        let rec = self
            .pending
            .get_mut(order_id)
            .ok_or_else(|| FinError::InvalidInput(format!("unknown order '{order_id}'")))?;
        let ack = rec.ack.ok_or_else(|| {
            FinError::InvalidInput(format!("ack not recorded for '{order_id}'"))
        })?;
        let fill_ns = ts.nanos();
        if fill_ns < ack {
            return Err(FinError::InvalidInput(format!(
                "fill timestamp before ack for '{order_id}'"
            )));
        }
        rec.fill = Some(fill_ns);
        self.ack_to_fill.push(fill_ns - ack);
        Ok(())
    }

    /// Records the book-update timestamp of `order_id` and removes it from pending.
    ///
    /// Stores the fill→book-update latency sample.
    ///
    /// # Errors
    /// - [`FinError::InvalidInput`] if `order_id` is unknown or fill was not recorded.
    /// - [`FinError::InvalidInput`] if book_update timestamp is before fill timestamp.
    pub fn record_book_update(
        &mut self,
        order_id: &str,
        ts: NanoTimestamp,
    ) -> Result<(), FinError> {
        let rec = self
            .pending
            .remove(order_id)
            .ok_or_else(|| FinError::InvalidInput(format!("unknown order '{order_id}'")))?;
        let fill = rec.fill.ok_or_else(|| {
            FinError::InvalidInput(format!("fill not recorded for '{order_id}'"))
        })?;
        let book_ns = ts.nanos();
        if book_ns < fill {
            return Err(FinError::InvalidInput(format!(
                "book_update timestamp before fill for '{order_id}'"
            )));
        }
        self.fill_to_book.push(book_ns - fill);
        Ok(())
    }

    /// Returns percentile statistics for `phase`, or `None` if no samples exist.
    pub fn stats(&self, phase: LatencyPhase) -> Option<PhaseStats> {
        let samples = match phase {
            LatencyPhase::SubmitToAck => &self.submit_to_ack,
            LatencyPhase::AckToFill => &self.ack_to_fill,
            LatencyPhase::FillToBookUpdate => &self.fill_to_book,
        };
        if samples.is_empty() {
            return None;
        }
        let mut sorted = samples.clone();
        sorted.sort_unstable();
        let n = sorted.len();
        let min_ns = *sorted.first().unwrap_or(&0);
        let max_ns = *sorted.last().unwrap_or(&0);
        let mean_ns = sorted.iter().map(|&v| v as f64).sum::<f64>() / n as f64;
        let p50_ns = percentile_ns(&sorted, 50);
        let p95_ns = percentile_ns(&sorted, 95);
        let p99_ns = percentile_ns(&sorted, 99);
        Some(PhaseStats {
            count: n,
            min_ns,
            max_ns,
            mean_ns,
            p50_ns,
            p95_ns,
            p99_ns,
        })
    }

    /// Returns the number of orders still awaiting lifecycle completion.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Returns all accumulated samples for `phase`.
    pub fn samples(&self, phase: LatencyPhase) -> &[i64] {
        match phase {
            LatencyPhase::SubmitToAck => &self.submit_to_ack,
            LatencyPhase::AckToFill => &self.ack_to_fill,
            LatencyPhase::FillToBookUpdate => &self.fill_to_book,
        }
    }
}

/// Returns the `p`th percentile (nearest-rank method) of a **sorted** slice.
fn percentile_ns(sorted: &[i64], p: usize) -> i64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((p * sorted.len()) / 100).min(sorted.len() - 1);
    sorted[idx]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(n: i64) -> NanoTimestamp {
        NanoTimestamp::new(n)
    }

    fn full_lifecycle(tracker: &mut OrderLatencyTracker, id: &str, t0: i64, t1: i64, t2: i64, t3: i64) {
        tracker.record_submit(id, ts(t0));
        tracker.record_ack(id, ts(t1)).unwrap();
        tracker.record_fill(id, ts(t2)).unwrap();
        tracker.record_book_update(id, ts(t3)).unwrap();
    }

    #[test]
    fn test_single_order_lifecycle() {
        let mut tracker = OrderLatencyTracker::new();
        full_lifecycle(&mut tracker, "o1", 1000, 2000, 4000, 5000);

        let s2a = tracker.stats(LatencyPhase::SubmitToAck).unwrap();
        assert_eq!(s2a.count, 1);
        assert_eq!(s2a.p50_ns, 1000);

        let a2f = tracker.stats(LatencyPhase::AckToFill).unwrap();
        assert_eq!(a2f.p50_ns, 2000);

        let f2b = tracker.stats(LatencyPhase::FillToBookUpdate).unwrap();
        assert_eq!(f2b.p50_ns, 1000);

        assert_eq!(tracker.pending_count(), 0);
    }

    #[test]
    fn test_percentiles_multiple_orders() {
        let mut tracker = OrderLatencyTracker::new();
        // 10 orders with submit→ack latencies 100ns..1000ns (step 100)
        for i in 1..=10_i64 {
            let id = format!("o{i}");
            let base = i * 10_000;
            tracker.record_submit(&id, ts(base));
            tracker.record_ack(&id, ts(base + i * 100)).unwrap();
            tracker.record_fill(&id, ts(base + i * 100 + 500)).unwrap();
            tracker.record_book_update(&id, ts(base + i * 100 + 500 + 200)).unwrap();
        }
        let stats = tracker.stats(LatencyPhase::SubmitToAck).unwrap();
        assert_eq!(stats.count, 10);
        assert_eq!(stats.min_ns, 100);
        assert_eq!(stats.max_ns, 1000);
        // p50 at index 5 (0-based) of sorted [100..1000]
        assert_eq!(stats.p50_ns, 500);
        assert_eq!(stats.p99_ns, 1000);
    }

    #[test]
    fn test_no_stats_before_samples() {
        let tracker = OrderLatencyTracker::new();
        assert!(tracker.stats(LatencyPhase::SubmitToAck).is_none());
    }

    #[test]
    fn test_unknown_order_errors() {
        let mut tracker = OrderLatencyTracker::new();
        assert!(matches!(
            tracker.record_ack("ghost", ts(1000)).unwrap_err(),
            FinError::InvalidInput(_)
        ));
    }

    #[test]
    fn test_ack_before_submit_errors() {
        let mut tracker = OrderLatencyTracker::new();
        tracker.record_submit("o1", ts(5000));
        assert!(matches!(
            tracker.record_ack("o1", ts(4000)).unwrap_err(),
            FinError::InvalidInput(_)
        ));
    }

    #[test]
    fn test_fill_before_ack_errors() {
        let mut tracker = OrderLatencyTracker::new();
        tracker.record_submit("o1", ts(1000));
        tracker.record_ack("o1", ts(2000)).unwrap();
        assert!(matches!(
            tracker.record_fill("o1", ts(1500)).unwrap_err(),
            FinError::InvalidInput(_)
        ));
    }
}
