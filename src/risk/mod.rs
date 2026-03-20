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
#[derive(Debug, Clone)]
pub struct DrawdownTracker {
    peak_equity: Decimal,
    current_equity: Decimal,
}

impl DrawdownTracker {
    /// Creates a new `DrawdownTracker` with the given initial (and peak) equity.
    pub fn new(initial_equity: Decimal) -> Self {
        Self {
            peak_equity: initial_equity,
            current_equity: initial_equity,
        }
    }

    /// Updates the tracker with the latest equity value, updating the peak if higher.
    pub fn update(&mut self, equity: Decimal) {
        if equity > self.peak_equity {
            self.peak_equity = equity;
        }
        self.current_equity = equity;
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
}
