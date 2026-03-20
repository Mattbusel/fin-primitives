//! # Module: risk
//!
//! ## Responsibility
//! Tracks equity drawdown and evaluates configurable risk rules on each equity update.
//!
//! ## Guarantees
//! - `DrawdownTracker::current_drawdown_pct` is always non-negative
//! - `RiskMonitor::update` returns all triggered `RiskBreach` values (empty vec if none)
//!
//! ## NOT Responsible For
//! - Position sizing
//! - Order cancellation (callers must act on returned breaches)

use rust_decimal::Decimal;

/// Tracks peak equity and computes current drawdown percentage.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DrawdownTracker {
    peak_equity: Decimal,
    current_equity: Decimal,
    worst_drawdown_pct: Decimal,
    /// Number of updates since the last new peak.
    updates_since_peak: usize,
}

impl DrawdownTracker {
    /// Creates a new `DrawdownTracker` with the given initial (and peak) equity.
    pub fn new(initial_equity: Decimal) -> Self {
        Self {
            peak_equity: initial_equity,
            current_equity: initial_equity,
            worst_drawdown_pct: Decimal::ZERO,
            updates_since_peak: 0,
        }
    }

    /// Updates the tracker with the latest equity value, updating the peak if higher.
    pub fn update(&mut self, equity: Decimal) {
        if equity > self.peak_equity {
            self.peak_equity = equity;
            self.updates_since_peak = 0;
        } else {
            self.updates_since_peak += 1;
        }
        self.current_equity = equity;
        let dd = self.current_drawdown_pct();
        if dd > self.worst_drawdown_pct {
            self.worst_drawdown_pct = dd;
        }
    }

    /// Returns the number of `update()` calls since the last new equity peak.
    ///
    /// A value of 0 means the last update set a new peak. Higher values indicate
    /// how long the portfolio has been in drawdown (in update units).
    pub fn drawdown_duration(&self) -> usize {
        self.updates_since_peak
    }

    /// Returns current drawdown as a percentage: `(peak - current) / peak * 100`.
    ///
    /// Returns `0` if `peak_equity` is zero.
    pub fn current_drawdown_pct(&self) -> Decimal {
        if self.peak_equity == Decimal::ZERO {
            return Decimal::ZERO;
        }
        (self.peak_equity - self.current_equity) / self.peak_equity * Decimal::ONE_HUNDRED
    }

    /// Returns the highest equity seen since construction.
    pub fn peak(&self) -> Decimal {
        self.peak_equity
    }

    /// Returns the current equity value.
    pub fn current_equity(&self) -> Decimal {
        self.current_equity
    }

    /// Returns `true` if the current drawdown percentage does not exceed `max_dd_pct`.
    pub fn is_below_threshold(&self, max_dd_pct: Decimal) -> bool {
        self.current_drawdown_pct() <= max_dd_pct
    }

    /// Resets the peak to the current equity value.
    ///
    /// Useful for daily or session-boundary resets where you want drawdown measured
    /// from the start of the new session rather than the all-time high.
    pub fn reset_peak(&mut self) {
        self.peak_equity = self.current_equity;
        self.updates_since_peak = 0;
    }

    /// Returns the worst (highest) drawdown percentage seen since construction or last reset.
    pub fn worst_drawdown_pct(&self) -> Decimal {
        self.worst_drawdown_pct
    }

    /// Fully resets the tracker as if it were freshly constructed with `initial` equity.
    pub fn reset(&mut self, initial: Decimal) {
        self.peak_equity = initial;
        self.current_equity = initial;
        self.worst_drawdown_pct = Decimal::ZERO;
        self.updates_since_peak = 0;
    }

    /// Returns the recovery factor: `net_profit_pct / worst_drawdown_pct`.
    ///
    /// A higher value indicates better risk-adjusted performance.
    /// Returns `None` when `worst_drawdown_pct` is zero (no drawdown has occurred).
    pub fn recovery_factor(&self, net_profit_pct: Decimal) -> Option<Decimal> {
        if self.worst_drawdown_pct.is_zero() {
            return None;
        }
        Some(net_profit_pct / self.worst_drawdown_pct)
    }

    /// Returns the Calmar ratio: `annualized_return / worst_drawdown_pct`.
    ///
    /// Higher values indicate better risk-adjusted performance. Returns `None` when
    /// `worst_drawdown_pct` is zero (no drawdown has occurred).
    pub fn calmar_ratio(&self, annualized_return: Decimal) -> Option<Decimal> {
        if self.worst_drawdown_pct.is_zero() {
            return None;
        }
        Some(annualized_return / self.worst_drawdown_pct)
    }

    /// Returns `true` if the current equity is strictly below the peak (i.e. in drawdown).
    pub fn in_drawdown(&self) -> bool {
        self.current_equity < self.peak_equity
    }

    /// Applies a sequence of equity values in order, as if each were an individual `update` call.
    ///
    /// Useful for batch processing historical equity curves without a manual loop.
    pub fn update_with_returns(&mut self, equities: &[Decimal]) {
        for &eq in equities {
            self.update(eq);
        }
    }

    /// Returns the number of consecutive updates where equity was below the peak.
    ///
    /// Equivalent to [`DrawdownTracker::drawdown_duration`]. Provided as a semantic
    /// alias for call sites that prefer "count" over "duration".
    pub fn drawdown_count(&self) -> usize {
        self.updates_since_peak
    }
}

impl std::fmt::Display for DrawdownTracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "equity={} peak={} drawdown={:.2}%",
            self.current_equity,
            self.peak_equity,
            self.current_drawdown_pct()
        )
    }
}

/// A triggered risk rule violation.
#[derive(Debug, Clone, PartialEq)]
pub struct RiskBreach {
    /// The name of the rule that triggered.
    pub rule: String,
    /// Human-readable detail of the violation.
    pub detail: String,
}

/// A risk rule that can be checked against current equity and drawdown.
pub trait RiskRule: Send {
    /// Returns the rule's name.
    fn name(&self) -> &str;

    /// Returns `Some(RiskBreach)` if the rule is violated, or `None` if compliant.
    ///
    /// # Arguments
    /// * `equity` - current portfolio equity
    /// * `drawdown_pct` - current drawdown percentage from peak
    fn check(&self, equity: Decimal, drawdown_pct: Decimal) -> Option<RiskBreach>;
}

/// Triggers a breach when drawdown exceeds a threshold percentage.
pub struct MaxDrawdownRule {
    /// The maximum allowed drawdown percentage (e.g., `dec!(10)` = 10%).
    pub threshold_pct: Decimal,
}

impl RiskRule for MaxDrawdownRule {
    fn name(&self) -> &str {
        "max_drawdown"
    }

    fn check(&self, _equity: Decimal, drawdown_pct: Decimal) -> Option<RiskBreach> {
        if drawdown_pct > self.threshold_pct {
            Some(RiskBreach {
                rule: self.name().to_owned(),
                detail: format!("drawdown {drawdown_pct:.2}% > {:.2}%", self.threshold_pct),
            })
        } else {
            None
        }
    }
}

/// Triggers a breach when equity falls below a floor.
pub struct MinEquityRule {
    /// The minimum acceptable equity.
    pub floor: Decimal,
}

impl RiskRule for MinEquityRule {
    fn name(&self) -> &str {
        "min_equity"
    }

    fn check(&self, equity: Decimal, _drawdown_pct: Decimal) -> Option<RiskBreach> {
        if equity < self.floor {
            Some(RiskBreach {
                rule: self.name().to_owned(),
                detail: format!("equity {equity} < floor {}", self.floor),
            })
        } else {
            None
        }
    }
}

/// Evaluates multiple `RiskRule`s on each equity update and returns all breaches.
pub struct RiskMonitor {
    rules: Vec<Box<dyn RiskRule>>,
    tracker: DrawdownTracker,
}

impl RiskMonitor {
    /// Creates a new `RiskMonitor` with no rules and the given initial equity.
    pub fn new(initial_equity: Decimal) -> Self {
        Self {
            rules: Vec::new(),
            tracker: DrawdownTracker::new(initial_equity),
        }
    }

    /// Adds a rule to the monitor (builder pattern).
    #[must_use]
    pub fn add_rule(mut self, rule: impl RiskRule + 'static) -> Self {
        self.rules.push(Box::new(rule));
        self
    }

    /// Updates equity and returns all triggered breaches.
    pub fn update(&mut self, equity: Decimal) -> Vec<RiskBreach> {
        self.tracker.update(equity);
        let dd = self.tracker.current_drawdown_pct();
        self.rules
            .iter()
            .filter_map(|r| r.check(equity, dd))
            .collect()
    }

    /// Returns the current drawdown percentage without triggering an update.
    pub fn drawdown_pct(&self) -> Decimal {
        self.tracker.current_drawdown_pct()
    }

    /// Returns the current equity value without triggering an update.
    pub fn current_equity(&self) -> Decimal {
        self.tracker.current_equity()
    }

    /// Returns the peak equity seen so far.
    pub fn peak_equity(&self) -> Decimal {
        self.tracker.peak()
    }

    /// Resets the internal drawdown tracker to `initial_equity`.
    pub fn reset(&mut self, initial_equity: Decimal) {
        self.tracker.reset(initial_equity);
    }

    /// Returns the number of rules registered with this monitor.
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// Resets the drawdown peak to the current equity.
    ///
    /// Delegates to [`DrawdownTracker::reset_peak`]. Useful at session boundaries
    /// when you want drawdown measured from the current level, not the all-time high.
    pub fn reset_peak(&mut self) {
        self.tracker.reset_peak();
    }

    /// Returns `true` if equity is currently below the recorded peak (i.e. in drawdown).
    pub fn is_in_drawdown(&self) -> bool {
        self.tracker.current_drawdown_pct() > Decimal::ZERO
    }

    /// Returns the worst (highest) drawdown percentage seen since construction or last reset.
    pub fn worst_drawdown_pct(&self) -> Decimal {
        self.tracker.worst_drawdown_pct()
    }

    /// Returns a shared reference to the internal [`DrawdownTracker`].
    ///
    /// Useful when callers need direct access to tracker state (e.g., worst drawdown)
    /// without going through the monitor's forwarding accessors.
    pub fn drawdown_tracker(&self) -> &DrawdownTracker {
        &self.tracker
    }

    /// Checks all rules against `equity` without updating the peak or current equity.
    ///
    /// Useful for prospective checks (e.g., "would this trade breach a rule?") where
    /// you do not want to alter tracked state.
    pub fn check(&self, equity: Decimal) -> Vec<RiskBreach> {
        let dd = if self.tracker.peak() == Decimal::ZERO {
            Decimal::ZERO
        } else {
            (self.tracker.peak() - equity) / self.tracker.peak() * Decimal::ONE_HUNDRED
        };
        self.rules
            .iter()
            .filter_map(|r| r.check(equity, dd))
            .collect()
    }

    /// Returns `true` if any rule would breach at the given `equity` level.
    ///
    /// Equivalent to `!self.check(equity).is_empty()` but short-circuits on the
    /// first breach and avoids allocating a `Vec`.
    pub fn has_breaches(&self, equity: Decimal) -> bool {
        !self.check(equity).is_empty()
    }

    /// Returns the absolute loss implied by `pct` percent drawdown from current peak equity.
    ///
    /// Useful for position-sizing calculations: "how much can I lose at X% drawdown?"
    /// Returns `Decimal::ZERO` when peak equity is zero.
    pub fn equity_at_risk(&self, pct: Decimal) -> Decimal {
        self.tracker.peak() * pct / Decimal::ONE_HUNDRED
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_drawdown_tracker_zero_at_peak() {
        let t = DrawdownTracker::new(dec!(10000));
        assert_eq!(t.current_drawdown_pct(), dec!(0));
    }

    #[test]
    fn test_drawdown_tracker_increases_below_peak() {
        let mut t = DrawdownTracker::new(dec!(10000));
        t.update(dec!(9000));
        assert_eq!(t.current_drawdown_pct(), dec!(10));
    }

    #[test]
    fn test_drawdown_tracker_peak_updates() {
        let mut t = DrawdownTracker::new(dec!(10000));
        t.update(dec!(12000));
        assert_eq!(t.peak(), dec!(12000));
    }

    #[test]
    fn test_drawdown_tracker_current_equity() {
        let mut t = DrawdownTracker::new(dec!(10000));
        t.update(dec!(9500));
        assert_eq!(t.current_equity(), dec!(9500));
    }

    #[test]
    fn test_drawdown_tracker_is_below_threshold_true() {
        let mut t = DrawdownTracker::new(dec!(10000));
        t.update(dec!(9500));
        assert!(t.is_below_threshold(dec!(10)));
    }

    #[test]
    fn test_drawdown_tracker_is_below_threshold_false() {
        let mut t = DrawdownTracker::new(dec!(10000));
        t.update(dec!(8000));
        assert!(!t.is_below_threshold(dec!(10)));
    }

    #[test]
    fn test_drawdown_tracker_never_negative() {
        let mut t = DrawdownTracker::new(dec!(10000));
        t.update(dec!(11000));
        assert_eq!(t.current_drawdown_pct(), dec!(0));
    }

    #[test]
    fn test_max_drawdown_rule_triggers_breach() {
        let rule = MaxDrawdownRule {
            threshold_pct: dec!(10),
        };
        let breach = rule.check(dec!(8000), dec!(20));
        assert!(breach.is_some());
    }

    #[test]
    fn test_max_drawdown_rule_no_breach_within_limit() {
        let rule = MaxDrawdownRule {
            threshold_pct: dec!(10),
        };
        let breach = rule.check(dec!(9500), dec!(5));
        assert!(breach.is_none());
    }

    #[test]
    fn test_max_drawdown_rule_at_exact_threshold_no_breach() {
        let rule = MaxDrawdownRule {
            threshold_pct: dec!(10),
        };
        let breach = rule.check(dec!(9000), dec!(10));
        assert!(breach.is_none());
    }

    #[test]
    fn test_min_equity_rule_breach() {
        let rule = MinEquityRule { floor: dec!(5000) };
        let breach = rule.check(dec!(4000), dec!(0));
        assert!(breach.is_some());
    }

    #[test]
    fn test_min_equity_rule_no_breach() {
        let rule = MinEquityRule { floor: dec!(5000) };
        let breach = rule.check(dec!(6000), dec!(0));
        assert!(breach.is_none());
    }

    #[test]
    fn test_risk_monitor_returns_all_breaches() {
        let mut monitor = RiskMonitor::new(dec!(10000))
            .add_rule(MaxDrawdownRule {
                threshold_pct: dec!(5),
            })
            .add_rule(MinEquityRule { floor: dec!(9000) });
        let breaches = monitor.update(dec!(8000));
        assert_eq!(breaches.len(), 2);
    }

    #[test]
    fn test_risk_monitor_no_breach_at_start() {
        let mut monitor = RiskMonitor::new(dec!(10000)).add_rule(MaxDrawdownRule {
            threshold_pct: dec!(10),
        });
        let breaches = monitor.update(dec!(10000));
        assert!(breaches.is_empty());
    }

    #[test]
    fn test_risk_monitor_partial_breach() {
        let mut monitor = RiskMonitor::new(dec!(10000))
            .add_rule(MaxDrawdownRule {
                threshold_pct: dec!(5),
            })
            .add_rule(MinEquityRule { floor: dec!(5000) });
        let breaches = monitor.update(dec!(9000));
        assert_eq!(breaches.len(), 1);
        assert_eq!(breaches[0].rule, "max_drawdown");
    }

    #[test]
    fn test_drawdown_recovery() {
        let mut monitor = RiskMonitor::new(dec!(10000)).add_rule(MaxDrawdownRule {
            threshold_pct: dec!(10),
        });
        let breaches = monitor.update(dec!(8000));
        assert_eq!(breaches.len(), 1);
        let breaches = monitor.update(dec!(10000));
        assert!(breaches.is_empty(), "no breach after recovery to peak");
        let breaches = monitor.update(dec!(12000));
        assert!(breaches.is_empty(), "no breach after rising above old peak");
        let breaches = monitor.update(dec!(11500));
        assert!(
            breaches.is_empty(),
            "small dip from new peak should not breach"
        );
    }

    #[test]
    fn test_drawdown_flat_series_is_zero() {
        let mut t = DrawdownTracker::new(dec!(10000));
        for _ in 0..10 {
            t.update(dec!(10000));
        }
        assert_eq!(t.current_drawdown_pct(), dec!(0));
    }

    #[test]
    fn test_drawdown_monotonic_decline_full_loss() {
        let mut t = DrawdownTracker::new(dec!(10000));
        t.update(dec!(5000));
        t.update(dec!(2500));
        t.update(dec!(1000));
        t.update(dec!(0));
        assert_eq!(t.current_drawdown_pct(), dec!(100));
    }

    #[test]
    fn test_risk_monitor_multiple_rules_all_must_pass() {
        let mut monitor = RiskMonitor::new(dec!(10000))
            .add_rule(MaxDrawdownRule {
                threshold_pct: dec!(5),
            })
            .add_rule(MinEquityRule { floor: dec!(9500) });
        let breaches = monitor.update(dec!(9400));
        assert_eq!(breaches.len(), 2, "both rules should trigger");
        let breaches = monitor.update(dec!(10000));
        assert!(breaches.is_empty(), "all rules pass at peak");
        let breaches = monitor.update(dec!(9600));
        assert!(
            breaches.is_empty(),
            "9600 is above the 9500 floor and within 5% drawdown"
        );
        let breaches = monitor.update(dec!(9400));
        assert_eq!(
            breaches.len(),
            2,
            "both rules fire when equity drops to 9400 again"
        );
    }

    #[test]
    fn test_risk_monitor_drawdown_pct_accessor() {
        let mut monitor = RiskMonitor::new(dec!(10000)).add_rule(MaxDrawdownRule {
            threshold_pct: dec!(20),
        });
        monitor.update(dec!(8000));
        assert_eq!(monitor.drawdown_pct(), dec!(20));
    }

    #[test]
    fn test_risk_monitor_current_equity_accessor() {
        let mut monitor = RiskMonitor::new(dec!(10000)).add_rule(MaxDrawdownRule {
            threshold_pct: dec!(20),
        });
        monitor.update(dec!(9500));
        assert_eq!(monitor.current_equity(), dec!(9500));
    }

    #[test]
    fn test_risk_rule_name_returns_str() {
        let rule: &dyn RiskRule = &MaxDrawdownRule {
            threshold_pct: dec!(10),
        };
        let name: &str = rule.name();
        assert_eq!(name, "max_drawdown");
    }

    #[test]
    fn test_drawdown_tracker_reset_clears_peak() {
        let mut t = DrawdownTracker::new(dec!(10000));
        t.update(dec!(8000));
        assert_eq!(t.current_drawdown_pct(), dec!(20));
        t.reset(dec!(5000));
        assert_eq!(t.peak(), dec!(5000));
        assert_eq!(t.current_equity(), dec!(5000));
        assert_eq!(t.current_drawdown_pct(), dec!(0));
    }

    #[test]
    fn test_drawdown_tracker_reset_then_update() {
        let mut t = DrawdownTracker::new(dec!(10000));
        t.reset(dec!(2000));
        t.update(dec!(1800));
        assert_eq!(t.current_drawdown_pct(), dec!(10));
    }

    #[test]
    fn test_drawdown_tracker_worst_drawdown_pct_accumulates() {
        let mut t = DrawdownTracker::new(dec!(10000));
        t.update(dec!(9000)); // 10% drawdown
        t.update(dec!(9500)); // partial recovery, worst still 10%
        t.update(dec!(10100)); // new peak
        t.update(dec!(9595)); // ~5% drawdown from new peak
        assert_eq!(t.worst_drawdown_pct(), dec!(10));
    }

    #[test]
    fn test_drawdown_tracker_worst_resets_on_full_reset() {
        let mut t = DrawdownTracker::new(dec!(10000));
        t.update(dec!(8000)); // 20% drawdown
        assert_eq!(t.worst_drawdown_pct(), dec!(20));
        t.reset(dec!(5000));
        assert_eq!(t.worst_drawdown_pct(), dec!(0));
    }

    #[test]
    fn test_risk_monitor_reset_clears_drawdown_state() {
        let mut monitor = RiskMonitor::new(dec!(10000))
            .add_rule(MaxDrawdownRule { threshold_pct: dec!(15) });
        monitor.update(dec!(8000)); // 20% drawdown → breach
        let breaches = monitor.update(dec!(8000));
        assert!(!breaches.is_empty());
        monitor.reset(dec!(10000));
        let breaches_after = monitor.update(dec!(9800)); // 2% drawdown
        assert!(breaches_after.is_empty());
    }

    #[test]
    fn test_risk_monitor_reset_restores_peak() {
        let mut monitor = RiskMonitor::new(dec!(10000));
        monitor.update(dec!(9000));
        monitor.reset(dec!(5000));
        assert_eq!(monitor.peak_equity(), dec!(5000));
        assert_eq!(monitor.current_equity(), dec!(5000));
    }

    #[test]
    fn test_risk_monitor_worst_drawdown_tracks_maximum() {
        let mut monitor = RiskMonitor::new(dec!(10000));
        monitor.update(dec!(9000)); // 10% drawdown
        monitor.update(dec!(8000)); // 20% drawdown
        monitor.update(dec!(9500)); // recovery — worst is still 20%
        assert_eq!(monitor.worst_drawdown_pct(), dec!(20));
    }

    #[test]
    fn test_risk_monitor_worst_drawdown_zero_at_start() {
        let monitor = RiskMonitor::new(dec!(10000));
        assert_eq!(monitor.worst_drawdown_pct(), dec!(0));
    }

    #[test]
    fn test_drawdown_tracker_display() {
        let mut t = DrawdownTracker::new(dec!(10000));
        t.update(dec!(9000));
        let s = format!("{t}");
        assert!(s.contains("9000"), "display should include current equity");
        assert!(s.contains("10000"), "display should include peak");
        assert!(s.contains("10.00"), "display should include drawdown pct");
    }

    #[test]
    fn test_drawdown_tracker_recovery_factor() {
        let mut t = DrawdownTracker::new(dec!(10000));
        t.update(dec!(9000)); // 10% worst drawdown
        // net profit 20% / worst_dd 10% = 2.0
        let rf = t.recovery_factor(dec!(20)).unwrap();
        assert_eq!(rf, dec!(2));
    }

    #[test]
    fn test_drawdown_tracker_recovery_factor_no_drawdown() {
        let t = DrawdownTracker::new(dec!(10000));
        assert!(t.recovery_factor(dec!(20)).is_none());
    }

    #[test]
    fn test_risk_monitor_check_non_mutating() {
        let monitor = RiskMonitor::new(dec!(10000))
            .add_rule(MaxDrawdownRule { threshold_pct: dec!(15) });
        // check with 20% drawdown from peak — should breach
        let breaches = monitor.check(dec!(8000));
        assert_eq!(breaches.len(), 1);
        // but peak hasn't changed
        assert_eq!(monitor.peak_equity(), dec!(10000));
        assert_eq!(monitor.current_equity(), dec!(10000));
    }

    #[test]
    fn test_risk_monitor_check_no_breach() {
        let monitor = RiskMonitor::new(dec!(10000))
            .add_rule(MaxDrawdownRule { threshold_pct: dec!(15) });
        let breaches = monitor.check(dec!(9000)); // 10% drawdown < 15%
        assert!(breaches.is_empty());
    }

    #[test]
    fn test_drawdown_tracker_in_drawdown_false_at_peak() {
        let tracker = DrawdownTracker::new(dec!(10000));
        assert!(!tracker.in_drawdown());
    }

    #[test]
    fn test_drawdown_tracker_in_drawdown_true_below_peak() {
        let mut tracker = DrawdownTracker::new(dec!(10000));
        tracker.update(dec!(9000));
        assert!(tracker.in_drawdown());
    }

    #[test]
    fn test_drawdown_tracker_in_drawdown_false_at_new_peak() {
        let mut tracker = DrawdownTracker::new(dec!(10000));
        tracker.update(dec!(11000));
        assert!(!tracker.in_drawdown());
    }

    #[test]
    fn test_drawdown_tracker_drawdown_count_increases() {
        let mut tracker = DrawdownTracker::new(dec!(10000));
        tracker.update(dec!(9500));
        tracker.update(dec!(9000));
        assert_eq!(tracker.drawdown_count(), 2);
    }

    #[test]
    fn test_drawdown_tracker_drawdown_count_resets_on_peak() {
        let mut tracker = DrawdownTracker::new(dec!(10000));
        tracker.update(dec!(9000));
        tracker.update(dec!(11000)); // new peak
        assert_eq!(tracker.drawdown_count(), 0);
    }

    #[test]
    fn test_risk_monitor_has_breaches_true() {
        let monitor = RiskMonitor::new(dec!(10000))
            .add_rule(MaxDrawdownRule { threshold_pct: dec!(5) });
        assert!(monitor.has_breaches(dec!(9000))); // 10% > 5%
    }

    #[test]
    fn test_risk_monitor_has_breaches_false() {
        let monitor = RiskMonitor::new(dec!(10000))
            .add_rule(MaxDrawdownRule { threshold_pct: dec!(15) });
        assert!(!monitor.has_breaches(dec!(9000))); // 10% < 15%
    }

    #[test]
    fn test_risk_monitor_is_in_drawdown_true() {
        let mut monitor = RiskMonitor::new(dec!(10000)).add_rule(MaxDrawdownRule { threshold_pct: dec!(50) });
        monitor.update(dec!(9000));
        assert!(monitor.is_in_drawdown());
    }

    #[test]
    fn test_risk_monitor_is_in_drawdown_false_at_peak() {
        let mut monitor = RiskMonitor::new(dec!(10000)).add_rule(MaxDrawdownRule { threshold_pct: dec!(50) });
        monitor.update(dec!(10000));
        assert!(!monitor.is_in_drawdown());
    }

    #[test]
    fn test_risk_monitor_is_in_drawdown_false_above_peak() {
        let mut monitor = RiskMonitor::new(dec!(10000)).add_rule(MaxDrawdownRule { threshold_pct: dec!(50) });
        monitor.update(dec!(11000));
        assert!(!monitor.is_in_drawdown());
    }

    #[test]
    fn test_calmar_ratio_with_drawdown() {
        let mut tracker = DrawdownTracker::new(dec!(10000));
        tracker.update(dec!(9000)); // 10% drawdown
        // annualized_return = 20%, worst_dd = 10% → calmar = 2
        let ratio = tracker.calmar_ratio(dec!(20)).unwrap();
        assert_eq!(ratio, dec!(2));
    }

    #[test]
    fn test_calmar_ratio_none_when_no_drawdown() {
        let tracker = DrawdownTracker::new(dec!(10000));
        // worst_drawdown_pct is 0 → None
        assert!(tracker.calmar_ratio(dec!(20)).is_none());
    }
}
