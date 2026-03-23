//! # Module: signals::multi_tf
//!
//! ## Responsibility
//! Multi-timeframe indicator aggregation: run identical indicator logic across
//! M1, M5, M15, H1, and D1 bar streams, then emit a `Confirmed` signal only
//! when the configured agreement policy is satisfied.
//!
//! ## Design
//! - `TimeframeSignal` wraps any `f64`-valued indicator behind a thin trait.
//! - `MultiTimeframeSignal` drives one indicator instance per timeframe and
//!   aggregates their outputs using `AgreementPolicy`.
//! - The coordinator does **not** resample bars; callers must push
//!   the appropriate bar to `update_timeframe(tf, bar_close)`.
//!
//! ## NOT Responsible For
//! - Bar construction/resampling (use `OhlcvAggregator` upstream)
//! - Specific indicator implementations (callers supply via `BoxedIndicator`)

use std::collections::HashMap;

// ─── timeframes ───────────────────────────────────────────────────────────────

/// Standard trading timeframes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Timeframe {
    /// 1-minute bars.
    M1,
    /// 5-minute bars.
    M5,
    /// 15-minute bars.
    M15,
    /// 1-hour bars.
    H1,
    /// Daily bars.
    D1,
}

impl Timeframe {
    /// All standard timeframes in ascending order.
    pub const ALL: [Timeframe; 5] = [
        Timeframe::M1,
        Timeframe::M5,
        Timeframe::M15,
        Timeframe::H1,
        Timeframe::D1,
    ];

    /// Human-readable label.
    pub fn label(&self) -> &'static str {
        match self {
            Timeframe::M1 => "M1",
            Timeframe::M5 => "M5",
            Timeframe::M15 => "M15",
            Timeframe::H1 => "H1",
            Timeframe::D1 => "D1",
        }
    }
}

// ─── signal direction ─────────────────────────────────────────────────────────

/// The directional opinion emitted by a single-timeframe indicator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SignalDirection {
    /// Indicator suggests buying / going long.
    Bullish,
    /// Indicator suggests selling / going short.
    Bearish,
    /// Indicator is neutral or in a dead zone.
    Neutral,
}

// ─── indicator trait ──────────────────────────────────────────────────────────

/// A stateful single-value indicator that can be updated with a bar close price
/// and returns a directional opinion.
///
/// Implementors are responsible for their own warm-up logic; they should return
/// `None` until sufficient data has accumulated.
pub trait TimeframeIndicator: Send {
    /// Update the indicator with the latest bar close price.
    /// Returns `Some(direction)` once warmed up, or `None` during warm-up.
    fn update(&mut self, close: f64) -> Option<SignalDirection>;

    /// Reset internal state.
    fn reset(&mut self);
}

/// Boxed, owned indicator for runtime polymorphism.
pub type BoxedIndicator = Box<dyn TimeframeIndicator>;

// ─── agreement policy ─────────────────────────────────────────────────────────

/// How many timeframes must agree before a `Confirmed` signal is emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AgreementPolicy {
    /// Every active timeframe must agree (and at least one must be active).
    Unanimous,
    /// Strictly more than half of active timeframes must agree.
    MajorityVote,
    /// At least `n` of the active timeframes must agree.
    AtLeast(usize),
}

// ─── multi-timeframe result ───────────────────────────────────────────────────

/// The outcome from `MultiTimeframeSignal::update_timeframe`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum MultiTfResult {
    /// All required timeframes agree on this direction.
    Confirmed {
        /// The direction all (or majority of) timeframes agree on.
        direction: SignalDirection,
        /// Number of active timeframes that voted for this direction.
        agreement_count: usize,
        /// Total number of active (warmed-up) timeframes.
        active_count: usize,
    },
    /// Not enough timeframes are warmed up or they disagree.
    Insufficient {
        /// Current per-timeframe opinions (may include `None` for not-yet-warmed-up).
        votes: HashMap<String, Option<SignalDirection>>,
    },
}

// ─── per-timeframe state ──────────────────────────────────────────────────────

struct TfSlot {
    timeframe: Timeframe,
    indicator: BoxedIndicator,
    last_direction: Option<SignalDirection>,
}

// ─── aggregator ───────────────────────────────────────────────────────────────

/// Runs identical indicator logic across multiple timeframes and emits a
/// confirmed signal when the `AgreementPolicy` is satisfied.
///
/// # Example
/// ```rust,no_run
/// use fin_primitives::signals::multi_tf::{
///     MultiTimeframeSignal, AgreementPolicy, Timeframe,
///     SignalDirection, TimeframeIndicator, BoxedIndicator,
/// };
///
/// struct SimpleMomentum { prev: Option<f64> }
/// impl TimeframeIndicator for SimpleMomentum {
///     fn update(&mut self, close: f64) -> Option<SignalDirection> {
///         let prev = self.prev.replace(close);
///         prev.map(|p| if close > p { SignalDirection::Bullish }
///                     else if close < p { SignalDirection::Bearish }
///                     else { SignalDirection::Neutral })
///     }
///     fn reset(&mut self) { self.prev = None; }
/// }
///
/// let mut mts = MultiTimeframeSignal::new(AgreementPolicy::Unanimous);
/// mts.add_timeframe(Timeframe::M1, Box::new(SimpleMomentum { prev: None }));
/// mts.add_timeframe(Timeframe::M5, Box::new(SimpleMomentum { prev: None }));
/// ```
pub struct MultiTimeframeSignal {
    policy: AgreementPolicy,
    slots: Vec<TfSlot>,
}

impl MultiTimeframeSignal {
    /// Create a new aggregator with the given agreement policy.
    pub fn new(policy: AgreementPolicy) -> Self {
        Self { policy, slots: Vec::new() }
    }

    /// Register an indicator for a given timeframe.
    /// If the timeframe is already registered, the old indicator is replaced.
    pub fn add_timeframe(&mut self, tf: Timeframe, indicator: BoxedIndicator) {
        if let Some(slot) = self.slots.iter_mut().find(|s| s.timeframe == tf) {
            slot.indicator = indicator;
            slot.last_direction = None;
        } else {
            self.slots.push(TfSlot { timeframe: tf, indicator, last_direction: None });
        }
    }

    /// Push a new bar close for the given timeframe.
    ///
    /// Returns `MultiTfResult::Confirmed` if the agreement policy is met across
    /// all currently warmed-up timeframes, or `MultiTfResult::Insufficient` otherwise.
    pub fn update_timeframe(&mut self, tf: Timeframe, close: f64) -> MultiTfResult {
        // Update the matching slot
        if let Some(slot) = self.slots.iter_mut().find(|s| s.timeframe == tf) {
            if let Some(dir) = slot.indicator.update(close) {
                slot.last_direction = Some(dir);
            }
        }

        // Collect votes from all warmed-up slots
        let active: Vec<(Timeframe, SignalDirection)> = self
            .slots
            .iter()
            .filter_map(|s| s.last_direction.map(|d| (s.timeframe, d)))
            .collect();

        let active_count = active.len();

        if active_count == 0 {
            let votes = self.current_vote_map();
            return MultiTfResult::Insufficient { votes };
        }

        // Count bullish vs bearish (neutral votes count against either)
        let bullish = active.iter().filter(|(_, d)| *d == SignalDirection::Bullish).count();
        let bearish = active.iter().filter(|(_, d)| *d == SignalDirection::Bearish).count();

        let (winner_dir, winner_count) = if bullish >= bearish {
            (SignalDirection::Bullish, bullish)
        } else {
            (SignalDirection::Bearish, bearish)
        };

        let threshold_met = match self.policy {
            AgreementPolicy::Unanimous => winner_count == active_count,
            AgreementPolicy::MajorityVote => winner_count * 2 > active_count,
            AgreementPolicy::AtLeast(n) => winner_count >= n,
        };

        if threshold_met && winner_count > 0 {
            MultiTfResult::Confirmed {
                direction: winner_dir,
                agreement_count: winner_count,
                active_count,
            }
        } else {
            MultiTfResult::Insufficient { votes: self.current_vote_map() }
        }
    }

    /// Reset all timeframe indicators to their initial state.
    pub fn reset_all(&mut self) {
        for slot in &mut self.slots {
            slot.indicator.reset();
            slot.last_direction = None;
        }
    }

    /// Returns how many timeframes are currently registered.
    pub fn timeframe_count(&self) -> usize {
        self.slots.len()
    }

    /// Returns how many timeframes have emitted at least one value (warmed up).
    pub fn active_count(&self) -> usize {
        self.slots.iter().filter(|s| s.last_direction.is_some()).count()
    }

    fn current_vote_map(&self) -> HashMap<String, Option<SignalDirection>> {
        self.slots
            .iter()
            .map(|s| (s.timeframe.label().to_owned(), s.last_direction))
            .collect()
    }
}

// ─── built-in simple momentum indicator ───────────────────────────────────────

/// A minimal momentum indicator: bullish if close > previous close, bearish if lower.
/// Useful as a default indicator when no custom logic is needed.
#[derive(Debug, Default)]
pub struct MomentumIndicator {
    prev: Option<f64>,
}

impl MomentumIndicator {
    /// Create a new momentum indicator.
    pub fn new() -> Self {
        Self { prev: None }
    }
}

impl TimeframeIndicator for MomentumIndicator {
    fn update(&mut self, close: f64) -> Option<SignalDirection> {
        let prev = self.prev.replace(close)?;
        if close > prev {
            Some(SignalDirection::Bullish)
        } else if close < prev {
            Some(SignalDirection::Bearish)
        } else {
            Some(SignalDirection::Neutral)
        }
    }

    fn reset(&mut self) {
        self.prev = None;
    }
}

// ─── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_mts(policy: AgreementPolicy) -> MultiTimeframeSignal {
        let mut mts = MultiTimeframeSignal::new(policy);
        for tf in Timeframe::ALL {
            mts.add_timeframe(tf, Box::new(MomentumIndicator::new()));
        }
        mts
    }

    fn seed(mts: &mut MultiTimeframeSignal, price: f64) {
        for tf in Timeframe::ALL {
            mts.update_timeframe(tf, price);
        }
    }

    #[test]
    fn test_insufficient_before_warmup() {
        let mut mts = make_mts(AgreementPolicy::Unanimous);
        // No prices pushed yet
        let result = mts.update_timeframe(Timeframe::M1, 100.0);
        assert!(matches!(result, MultiTfResult::Insufficient { .. }));
    }

    #[test]
    fn test_unanimous_bullish_confirmed() {
        let mut mts = make_mts(AgreementPolicy::Unanimous);
        seed(&mut mts, 100.0); // first observation, no direction yet
        // All rise → all bullish
        for tf in Timeframe::ALL {
            mts.update_timeframe(tf, 101.0);
        }
        let result = mts.update_timeframe(Timeframe::D1, 101.0);
        assert!(
            matches!(result, MultiTfResult::Confirmed { direction: SignalDirection::Bullish, .. }),
            "expected Confirmed Bullish, got {result:?}"
        );
    }

    #[test]
    fn test_majority_vote_with_one_disagreement() {
        let mut mts = make_mts(AgreementPolicy::MajorityVote);
        seed(&mut mts, 100.0);
        // 4 bullish, 1 bearish
        for &tf in &[Timeframe::M1, Timeframe::M5, Timeframe::M15, Timeframe::H1] {
            mts.update_timeframe(tf, 105.0);
        }
        let result = mts.update_timeframe(Timeframe::D1, 95.0); // bearish on D1
        assert!(
            matches!(result, MultiTfResult::Confirmed { direction: SignalDirection::Bullish, .. }),
            "expected Confirmed Bullish majority, got {result:?}"
        );
    }

    #[test]
    fn test_unanimous_fails_on_disagreement() {
        let mut mts = make_mts(AgreementPolicy::Unanimous);
        seed(&mut mts, 100.0);
        // 4 bullish, 1 bearish
        for &tf in &[Timeframe::M1, Timeframe::M5, Timeframe::M15, Timeframe::H1] {
            mts.update_timeframe(tf, 110.0);
        }
        let result = mts.update_timeframe(Timeframe::D1, 90.0);
        assert!(matches!(result, MultiTfResult::Insufficient { .. }));
    }

    #[test]
    fn test_timeframe_count() {
        let mts = make_mts(AgreementPolicy::MajorityVote);
        assert_eq!(mts.timeframe_count(), 5);
    }

    #[test]
    fn test_reset_all() {
        let mut mts = make_mts(AgreementPolicy::Unanimous);
        seed(&mut mts, 100.0);
        seed(&mut mts, 105.0);
        assert!(mts.active_count() > 0);
        mts.reset_all();
        assert_eq!(mts.active_count(), 0);
    }
}
