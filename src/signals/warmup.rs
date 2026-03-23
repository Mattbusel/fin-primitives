//! # Signal Warmup Contracts
//!
//! Formalises the warmup period that every indicator implicitly has, making it
//! queryable, enforceable at the type level, and reportable across a full pipeline.
//!
//! ## Overview
//!
//! Every signal in `fin-primitives` returns [`SignalValue::Unavailable`] until it has
//! accumulated enough bars to produce a meaningful value. Before this module, that
//! behaviour was implicit — callers had to inspect each value and handle `Unavailable`
//! ad-hoc. The warmup contract system makes it explicit and composable:
//!
//! | Type | Purpose |
//! |------|---------|
//! | [`WarmupContract`] | Trait: query warmup period, readiness, and remaining bars |
//! | [`WarmupGuard`] | Wrapper: converts `Unavailable` into a typed `Err(NotReady)` |
//! | [`WarmupReport`] | Report: snapshot of all signal warmup states in a pipeline |
//! | [`SignalWarmupStatus`] | Per-signal status entry within a `WarmupReport` |
//!
//! ## Example
//!
//! ```rust
//! use fin_primitives::signals::indicators::Sma;
//! use fin_primitives::signals::{BarInput, Signal};
//! use fin_primitives::signals::warmup::{WarmupContract, WarmupGuard};
//! use rust_decimal_macros::dec;
//!
//! let sma = Sma::new("sma5", 5).unwrap();
//! let mut guard = WarmupGuard::new(sma);
//!
//! // Feed 4 bars — guard returns Err(NotReady)
//! for _ in 0..4 {
//!     let bar = BarInput::from_close(dec!(100));
//!     let result = guard.update_checked(&bar);
//!     assert!(result.is_err());
//! }
//!
//! // 5th bar — guard returns Ok(SignalValue::Scalar(...))
//! let bar = BarInput::from_close(dec!(100));
//! let result = guard.update_checked(&bar);
//! assert!(result.is_ok());
//! ```

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

// ── WarmupContract ────────────────────────────────────────────────────────────

/// A formal contract for inspecting a signal's warmup state.
///
/// Every signal that implements [`Signal`] has an implicit warmup period; this
/// trait exposes that information as a structured, queryable contract. Implement
/// this trait alongside `Signal` to participate in [`WarmupReport`] generation.
pub trait WarmupContract {
    /// Returns the total number of bars required before the signal is ready.
    ///
    /// This is a static property of the indicator: it does not change during
    /// the signal's lifetime. Corresponds to the indicator's `period()`.
    fn warmup_period(&self) -> usize;

    /// Returns `true` if the signal has accumulated enough bars to produce a value.
    ///
    /// After this returns `true`, [`Signal::update`] will no longer return
    /// `SignalValue::Unavailable` due to insufficient data.
    fn is_ready(&self) -> bool;

    /// Returns the number of bars still needed before the signal becomes ready.
    ///
    /// Returns `0` if the signal is already ready.
    fn bars_remaining(&self) -> usize;
}

/// Blanket implementation of [`WarmupContract`] for any type that implements [`Signal`].
///
/// This allows all built-in indicators (`Sma`, `Ema`, `Rsi`, …) to be used with
/// [`WarmupGuard`] and [`WarmupReport`] without additional boilerplate.
impl<S: Signal> WarmupContract for S {
    fn warmup_period(&self) -> usize {
        self.period()
    }

    fn is_ready(&self) -> bool {
        self.is_ready()
    }

    fn bars_remaining(&self) -> usize {
        if self.is_ready() {
            0
        } else {
            // `period()` is the number of bars needed; subtract what we've seen.
            // Because `is_ready()` is false, the signal itself tracks this implicitly
            // via its period field. We derive the remaining count from the period.
            // Individual indicators store their bar count internally; the public API
            // only exposes `is_ready()` and `period()`, so we use those.
            //
            // The accurate remaining count would require access to internal state.
            // We conservatively return `warmup_period()` as the upper bound when
            // the signal is not ready but no internal count is exposed. Concrete
            // structs may override this blanket impl for precision.
            self.period()
        }
    }
}

// ── NotReady error ────────────────────────────────────────────────────────────

/// Error returned by [`WarmupGuard`] when a signal update is requested before warmup completes.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("Signal '{name}' not ready: {bars_remaining} bars still needed (period = {warmup_period})")]
pub struct NotReady {
    /// Name of the signal that is not yet ready.
    pub name: String,
    /// Total bars required before the signal becomes ready.
    pub warmup_period: usize,
    /// Bars still needed.
    pub bars_remaining: usize,
}

// ── WarmupGuard ───────────────────────────────────────────────────────────────

/// Wraps any [`Signal`] and converts `SignalValue::Unavailable` into a typed error.
///
/// Without a guard, callers must match on `SignalValue::Unavailable` manually at
/// every call site. `WarmupGuard` makes the warmup contract explicit and enforceable:
/// any call to [`WarmupGuard::update_checked`] before warmup completes returns
/// `Err(NotReady { … })` rather than silently emitting an incomplete value.
///
/// The inner signal is still updated on every call, so the guard does not delay
/// warmup — it just fails loudly until the signal is ready.
///
/// # Example
///
/// ```rust
/// use fin_primitives::signals::indicators::Ema;
/// use fin_primitives::signals::{BarInput, Signal};
/// use fin_primitives::signals::warmup::WarmupGuard;
/// use rust_decimal_macros::dec;
///
/// let ema = Ema::new("ema3", 3).unwrap();
/// let mut guard = WarmupGuard::new(ema);
///
/// let bar = BarInput::from_close(dec!(50));
/// assert!(guard.update_checked(&bar).is_err()); // not ready yet
/// ```
pub struct WarmupGuard<S: Signal> {
    inner: S,
    /// Number of bars fed into the inner signal since construction or last reset.
    bars_seen: usize,
}

impl<S: Signal> WarmupGuard<S> {
    /// Wraps `signal` in a `WarmupGuard`.
    pub fn new(signal: S) -> Self {
        Self { inner: signal, bars_seen: 0 }
    }

    /// Updates the inner signal and returns its value, or `Err(NotReady)` if not yet ready.
    ///
    /// The inner signal is updated regardless of readiness, so warmup progresses
    /// normally. Once ready, subsequent calls return `Ok(SignalValue::Scalar(_))`.
    ///
    /// # Errors
    ///
    /// - Returns `Err(NotReady)` if the signal has not yet accumulated enough bars.
    /// - Returns `Err(FinError)` (wrapped) if the inner signal returns an arithmetic error.
    pub fn update_checked(&mut self, bar: &BarInput) -> Result<SignalValue, WarmupError> {
        self.bars_seen += 1;
        let value = self.inner.update(bar).map_err(WarmupError::Signal)?;
        match &value {
            SignalValue::Unavailable => {
                let period = self.inner.period();
                let remaining = period.saturating_sub(self.bars_seen);
                Err(WarmupError::NotReady(NotReady {
                    name: self.inner.name().to_owned(),
                    warmup_period: period,
                    bars_remaining: remaining,
                }))
            }
            SignalValue::Scalar(_) => Ok(value),
        }
    }

    /// Returns `true` if the inner signal is ready.
    pub fn is_ready(&self) -> bool {
        self.inner.is_ready()
    }

    /// Returns the number of bars remaining until the signal is ready.
    ///
    /// Returns `0` if already ready.
    pub fn bars_remaining(&self) -> usize {
        self.inner.period().saturating_sub(self.bars_seen)
    }

    /// Returns the total warmup period of the inner signal.
    pub fn warmup_period(&self) -> usize {
        self.inner.period()
    }

    /// Returns the number of bars fed into this guard since construction or last reset.
    pub fn bars_seen(&self) -> usize {
        self.bars_seen
    }

    /// Resets the inner signal and the bar counter.
    pub fn reset(&mut self) {
        self.inner.reset();
        self.bars_seen = 0;
    }

    /// Returns a reference to the inner signal.
    pub fn inner(&self) -> &S {
        &self.inner
    }

    /// Consumes the guard and returns the inner signal.
    pub fn into_inner(self) -> S {
        self.inner
    }
}

// ── WarmupError ───────────────────────────────────────────────────────────────

/// Errors that can be returned by [`WarmupGuard::update_checked`].
#[derive(Debug, thiserror::Error)]
pub enum WarmupError {
    /// The signal has not yet accumulated enough bars to produce a value.
    #[error("{0}")]
    NotReady(NotReady),

    /// The inner signal returned a `FinError` (e.g., arithmetic overflow).
    #[error("signal error: {0}")]
    Signal(FinError),
}

// ── WarmupReport ─────────────────────────────────────────────────────────────

/// Warmup status for a single signal within a [`WarmupReport`].
#[derive(Debug, Clone, PartialEq)]
pub struct SignalWarmupStatus {
    /// The name of the signal.
    pub name: String,
    /// The number of bars required for this signal to become ready.
    pub warmup_period: usize,
    /// Whether the signal is currently ready.
    pub is_ready: bool,
    /// Bars still needed (0 if ready).
    pub bars_remaining: usize,
}

impl SignalWarmupStatus {
    /// Returns a human-readable summary line for this status entry.
    pub fn summary(&self) -> String {
        if self.is_ready {
            format!("[READY]   {} (period={})", self.name, self.warmup_period)
        } else {
            format!(
                "[WARMING] {} (period={}, remaining={})",
                self.name, self.warmup_period, self.bars_remaining
            )
        }
    }
}

/// A snapshot of warmup states for all signals tracked by a [`WarmupReporter`].
///
/// Use [`WarmupReporter::report`] to generate a `WarmupReport` after each bar.
/// The report lets callers determine which signals are ready and which are still
/// warming up, without inspecting each signal individually.
///
/// # Example
///
/// ```rust
/// use fin_primitives::signals::indicators::{Sma, Rsi};
/// use fin_primitives::signals::warmup::WarmupReporter;
///
/// let sma = Sma::new("sma10", 10).unwrap();
/// let rsi = Rsi::new("rsi14", 14).unwrap();
/// let reporter = WarmupReporter::new(vec![
///     sma.warmup_period(),
///     rsi.warmup_period(),
/// ], vec!["sma10".into(), "rsi14".into()]);
/// let report = reporter.report(0);
/// assert!(!report.all_ready());
/// ```
#[derive(Debug, Clone)]
pub struct WarmupReport {
    /// Per-signal status entries, in the order they were registered.
    pub statuses: Vec<SignalWarmupStatus>,
    /// Total bars consumed by the reporting pipeline so far.
    pub bars_consumed: usize,
}

impl WarmupReport {
    /// Returns `true` if every tracked signal is ready.
    pub fn all_ready(&self) -> bool {
        self.statuses.iter().all(|s| s.is_ready)
    }

    /// Returns `true` if at least one tracked signal is not yet ready.
    pub fn any_warming(&self) -> bool {
        self.statuses.iter().any(|s| !s.is_ready)
    }

    /// Returns the number of signals that are currently ready.
    pub fn ready_count(&self) -> usize {
        self.statuses.iter().filter(|s| s.is_ready).count()
    }

    /// Returns the number of signals still warming up.
    pub fn warming_count(&self) -> usize {
        self.statuses.iter().filter(|s| !s.is_ready).count()
    }

    /// Returns the maximum number of bars remaining across all warming signals.
    ///
    /// This is the number of additional bars needed before the *entire* pipeline
    /// is ready. Returns `0` if all signals are already ready.
    pub fn pipeline_bars_remaining(&self) -> usize {
        self.statuses.iter().map(|s| s.bars_remaining).max().unwrap_or(0)
    }

    /// Returns an iterator over signals that are ready.
    pub fn ready_signals(&self) -> impl Iterator<Item = &SignalWarmupStatus> {
        self.statuses.iter().filter(|s| s.is_ready)
    }

    /// Returns an iterator over signals that are still warming up.
    pub fn warming_signals(&self) -> impl Iterator<Item = &SignalWarmupStatus> {
        self.statuses.iter().filter(|s| !s.is_ready)
    }

    /// Returns a multi-line human-readable summary of the pipeline warmup state.
    pub fn display(&self) -> String {
        let mut lines = vec![format!(
            "WarmupReport [bars_consumed={}, ready={}/{}, pipeline_remaining={}]",
            self.bars_consumed,
            self.ready_count(),
            self.statuses.len(),
            self.pipeline_bars_remaining(),
        )];
        for status in &self.statuses {
            lines.push(format!("  {}", status.summary()));
        }
        lines.join("\n")
    }
}

// ── WarmupReporter ────────────────────────────────────────────────────────────

/// Tracks warmup progress for a set of named signals and produces [`WarmupReport`]s.
///
/// `WarmupReporter` stores the static warmup period for each signal by name, along
/// with the current bar count, allowing it to generate accurate [`WarmupReport`]s
/// at any point without holding mutable references to the signals themselves.
///
/// Construct via [`WarmupReporter::new`], then call [`WarmupReporter::tick`] after
/// each bar update and [`WarmupReporter::report`] whenever you need a snapshot.
///
/// # Example
///
/// ```rust
/// use fin_primitives::signals::warmup::WarmupReporter;
///
/// let mut reporter = WarmupReporter::new(
///     vec![5, 14],
///     vec!["sma5".into(), "rsi14".into()],
/// );
///
/// for _ in 0..5 {
///     reporter.tick();
/// }
///
/// let report = reporter.report(reporter.bars_consumed());
/// assert!(report.statuses[0].is_ready);   // sma5 ready
/// assert!(!report.statuses[1].is_ready);  // rsi14 still warming
/// ```
pub struct WarmupReporter {
    /// Registered signal names, in registration order.
    names: Vec<String>,
    /// Warmup periods for each signal, parallel to `names`.
    periods: Vec<usize>,
    /// Total bars consumed.
    bars_consumed: usize,
}

impl WarmupReporter {
    /// Creates a new `WarmupReporter` for the given named signals.
    ///
    /// `periods` and `names` must have the same length. The i-th entry in `periods`
    /// corresponds to the warmup period for the i-th name in `names`.
    ///
    /// # Panics
    /// Panics in debug builds if `periods.len() != names.len()`.
    pub fn new(periods: Vec<usize>, names: Vec<String>) -> Self {
        debug_assert_eq!(periods.len(), names.len(), "periods and names must have equal length");
        Self { names, periods, bars_consumed: 0 }
    }

    /// Advances the bar counter by one.
    ///
    /// Call this after each bar is processed to keep the reporter in sync with
    /// the indicator pipeline.
    pub fn tick(&mut self) {
        self.bars_consumed += 1;
    }

    /// Advances the bar counter by `n` bars at once.
    pub fn tick_n(&mut self, n: usize) {
        self.bars_consumed += n;
    }

    /// Returns the total number of bars consumed so far.
    pub fn bars_consumed(&self) -> usize {
        self.bars_consumed
    }

    /// Resets the bar counter to zero.
    pub fn reset(&mut self) {
        self.bars_consumed = 0;
    }

    /// Generates a [`WarmupReport`] given the current `bars_consumed`.
    ///
    /// Pass `self.bars_consumed()` for live reporting, or any integer for
    /// hypothetical "how ready would we be after N bars?" queries.
    pub fn report(&self, bars_consumed: usize) -> WarmupReport {
        let statuses = self
            .names
            .iter()
            .zip(self.periods.iter())
            .map(|(name, &period)| {
                let is_ready = bars_consumed >= period;
                let bars_remaining = period.saturating_sub(bars_consumed);
                SignalWarmupStatus {
                    name: name.clone(),
                    warmup_period: period,
                    is_ready,
                    bars_remaining,
                }
            })
            .collect();
        WarmupReport { statuses, bars_consumed }
    }

    /// Returns the number of signals registered with this reporter.
    pub fn signal_count(&self) -> usize {
        self.names.len()
    }

    /// Returns the maximum warmup period across all registered signals.
    ///
    /// This is the minimum number of bars before the full pipeline can be considered
    /// ready. Returns `0` if no signals are registered.
    pub fn max_warmup_period(&self) -> usize {
        self.periods.iter().copied().max().unwrap_or(0)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals::indicators::{Ema, Rsi, Sma};
    use rust_decimal_macros::dec;

    fn bar(close: &str) -> BarInput {
        BarInput::from_close(close.parse().unwrap())
    }

    // ── WarmupContract blanket impl ──────────────────────────────────────────

    #[test]
    fn test_warmup_contract_sma_period() {
        let sma = Sma::new("sma10", 10).unwrap();
        assert_eq!(sma.warmup_period(), 10);
    }

    #[test]
    fn test_warmup_contract_rsi_period() {
        let rsi = Rsi::new("rsi14", 14).unwrap();
        assert_eq!(rsi.warmup_period(), 14);
    }

    #[test]
    fn test_warmup_contract_ema_not_ready_initially() {
        let ema = Ema::new("ema5", 5).unwrap();
        assert!(!ema.is_ready());
    }

    #[test]
    fn test_warmup_contract_sma_ready_after_period() {
        let mut sma = Sma::new("sma3", 3).unwrap();
        sma.update(&bar("10")).unwrap();
        sma.update(&bar("20")).unwrap();
        assert!(!sma.is_ready());
        sma.update(&bar("30")).unwrap();
        assert!(sma.is_ready());
        assert_eq!(sma.bars_remaining(), 0);
    }

    // ── WarmupGuard ──────────────────────────────────────────────────────────

    #[test]
    fn test_warmup_guard_returns_err_before_ready() {
        let sma = Sma::new("sma5", 5).unwrap();
        let mut guard = WarmupGuard::new(sma);
        for _ in 0..4 {
            let result = guard.update_checked(&bar("100"));
            assert!(
                matches!(result, Err(WarmupError::NotReady(_))),
                "expected NotReady error"
            );
        }
    }

    #[test]
    fn test_warmup_guard_returns_ok_after_warmup() {
        let sma = Sma::new("sma3", 3).unwrap();
        let mut guard = WarmupGuard::new(sma);
        guard.update_checked(&bar("10")).ok();
        guard.update_checked(&bar("20")).ok();
        let result = guard.update_checked(&bar("30"));
        assert!(result.is_ok(), "expected Ok after warmup");
        assert!(matches!(result.unwrap(), SignalValue::Scalar(_)));
    }

    #[test]
    fn test_warmup_guard_is_ready_tracks_correctly() {
        let sma = Sma::new("sma2", 2).unwrap();
        let mut guard = WarmupGuard::new(sma);
        assert!(!guard.is_ready());
        guard.update_checked(&bar("10")).ok();
        assert!(!guard.is_ready());
        guard.update_checked(&bar("20")).ok();
        assert!(guard.is_ready());
    }

    #[test]
    fn test_warmup_guard_bars_remaining_decrements() {
        let sma = Sma::new("sma4", 4).unwrap();
        let mut guard = WarmupGuard::new(sma);
        assert_eq!(guard.bars_remaining(), 4);
        guard.update_checked(&bar("1")).ok();
        assert_eq!(guard.bars_remaining(), 3);
        guard.update_checked(&bar("1")).ok();
        assert_eq!(guard.bars_remaining(), 2);
    }

    #[test]
    fn test_warmup_guard_bars_remaining_zero_when_ready() {
        let sma = Sma::new("sma2", 2).unwrap();
        let mut guard = WarmupGuard::new(sma);
        guard.update_checked(&bar("10")).ok();
        guard.update_checked(&bar("20")).ok();
        assert_eq!(guard.bars_remaining(), 0);
    }

    #[test]
    fn test_warmup_guard_reset_restarts_warmup() {
        let sma = Sma::new("sma2", 2).unwrap();
        let mut guard = WarmupGuard::new(sma);
        guard.update_checked(&bar("10")).ok();
        guard.update_checked(&bar("20")).ok();
        assert!(guard.is_ready());
        guard.reset();
        assert!(!guard.is_ready());
        assert_eq!(guard.bars_seen(), 0);
        let result = guard.update_checked(&bar("10"));
        assert!(matches!(result, Err(WarmupError::NotReady(_))));
    }

    #[test]
    fn test_warmup_guard_not_ready_error_has_correct_name() {
        let sma = Sma::new("my_sma", 5).unwrap();
        let mut guard = WarmupGuard::new(sma);
        match guard.update_checked(&bar("100")) {
            Err(WarmupError::NotReady(e)) => {
                assert_eq!(e.name, "my_sma");
                assert_eq!(e.warmup_period, 5);
            }
            _ => panic!("expected NotReady"),
        }
    }

    #[test]
    fn test_warmup_guard_rsi_warmup_period() {
        let rsi = Rsi::new("rsi14", 14).unwrap();
        let guard = WarmupGuard::new(rsi);
        assert_eq!(guard.warmup_period(), 14);
    }

    #[test]
    fn test_warmup_guard_into_inner() {
        let sma = Sma::new("sma3", 3).unwrap();
        let guard = WarmupGuard::new(sma);
        let inner = guard.into_inner();
        assert_eq!(inner.name(), "sma3");
    }

    // ── WarmupReporter ───────────────────────────────────────────────────────

    #[test]
    fn test_warmup_reporter_all_warming_at_zero_bars() {
        let reporter = WarmupReporter::new(vec![5, 14], vec!["sma5".into(), "rsi14".into()]);
        let report = reporter.report(0);
        assert!(!report.all_ready());
        assert_eq!(report.warming_count(), 2);
        assert_eq!(report.ready_count(), 0);
    }

    #[test]
    fn test_warmup_reporter_partial_ready() {
        let reporter = WarmupReporter::new(vec![5, 14], vec!["sma5".into(), "rsi14".into()]);
        let report = reporter.report(5);
        assert!(!report.all_ready());
        assert_eq!(report.ready_count(), 1);
        assert_eq!(report.warming_count(), 1);
        assert!(report.statuses[0].is_ready);
        assert!(!report.statuses[1].is_ready);
    }

    #[test]
    fn test_warmup_reporter_all_ready() {
        let reporter = WarmupReporter::new(vec![5, 14], vec!["sma5".into(), "rsi14".into()]);
        let report = reporter.report(14);
        assert!(report.all_ready());
        assert_eq!(report.pipeline_bars_remaining(), 0);
    }

    #[test]
    fn test_warmup_reporter_tick_advances_count() {
        let mut reporter = WarmupReporter::new(vec![3], vec!["sma3".into()]);
        reporter.tick();
        reporter.tick();
        reporter.tick();
        assert_eq!(reporter.bars_consumed(), 3);
        let report = reporter.report(reporter.bars_consumed());
        assert!(report.all_ready());
    }

    #[test]
    fn test_warmup_reporter_tick_n() {
        let mut reporter = WarmupReporter::new(vec![10], vec!["sma10".into()]);
        reporter.tick_n(10);
        let report = reporter.report(reporter.bars_consumed());
        assert!(report.statuses[0].is_ready);
    }

    #[test]
    fn test_warmup_reporter_pipeline_bars_remaining() {
        let reporter = WarmupReporter::new(vec![5, 14, 20], vec!["a".into(), "b".into(), "c".into()]);
        let report = reporter.report(10);
        // c needs 20 bars, 10 consumed → 10 remaining
        assert_eq!(report.pipeline_bars_remaining(), 10);
    }

    #[test]
    fn test_warmup_reporter_max_warmup_period() {
        let reporter = WarmupReporter::new(vec![5, 14, 200], vec!["a".into(), "b".into(), "c".into()]);
        assert_eq!(reporter.max_warmup_period(), 200);
    }

    #[test]
    fn test_warmup_report_display_contains_signal_names() {
        let reporter = WarmupReporter::new(vec![5], vec!["mysig".into()]);
        let report = reporter.report(0);
        let display = report.display();
        assert!(display.contains("mysig"));
        assert!(display.contains("WARMING"));
    }

    #[test]
    fn test_warmup_report_display_shows_ready() {
        let reporter = WarmupReporter::new(vec![5], vec!["mysig".into()]);
        let report = reporter.report(5);
        let display = report.display();
        assert!(display.contains("READY"));
    }

    #[test]
    fn test_signal_warmup_status_summary_ready() {
        let status = SignalWarmupStatus {
            name: "sma5".into(),
            warmup_period: 5,
            is_ready: true,
            bars_remaining: 0,
        };
        assert!(status.summary().contains("READY"));
        assert!(status.summary().contains("sma5"));
    }

    #[test]
    fn test_signal_warmup_status_summary_warming() {
        let status = SignalWarmupStatus {
            name: "rsi14".into(),
            warmup_period: 14,
            is_ready: false,
            bars_remaining: 7,
        };
        let s = status.summary();
        assert!(s.contains("WARMING"));
        assert!(s.contains("remaining=7"));
    }

    #[test]
    fn test_warmup_reporter_reset() {
        let mut reporter = WarmupReporter::new(vec![3], vec!["sma3".into()]);
        reporter.tick_n(10);
        reporter.reset();
        assert_eq!(reporter.bars_consumed(), 0);
        let report = reporter.report(reporter.bars_consumed());
        assert!(!report.all_ready());
    }

    #[test]
    fn test_warmup_reporter_empty_reports_all_ready() {
        let reporter = WarmupReporter::new(vec![], vec![]);
        let report = reporter.report(0);
        // vacuously all ready (no signals)
        assert!(report.all_ready());
        assert_eq!(report.pipeline_bars_remaining(), 0);
    }

    #[test]
    fn test_warmup_guard_period_1_immediate() {
        let sma = Sma::new("sma1", 1).unwrap();
        let mut guard = WarmupGuard::new(sma);
        let result = guard.update_checked(&bar("42"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_not_ready_error_display() {
        let err = NotReady {
            name: "sma10".into(),
            warmup_period: 10,
            bars_remaining: 5,
        };
        let msg = err.to_string();
        assert!(msg.contains("sma10"));
        assert!(msg.contains("5 bars"));
    }

    #[test]
    fn test_warmup_guard_bars_seen_tracks_all_updates() {
        let sma = Sma::new("sma20", 20).unwrap();
        let mut guard = WarmupGuard::new(sma);
        for i in 0..7 {
            guard.update_checked(&bar("10")).ok();
            assert_eq!(guard.bars_seen(), i + 1);
        }
    }

    #[test]
    fn test_warmup_contract_ema_warmup_period_matches_period() {
        let ema = Ema::new("ema20", 20).unwrap();
        assert_eq!(ema.warmup_period(), ema.period());
    }
}
