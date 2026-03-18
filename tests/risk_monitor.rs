use fin_primitives::risk::{MaxDrawdownRule, MinEquityRule, RiskMonitor, RiskRule};
use rust_decimal_macros::dec;

#[test]
fn test_risk_monitor_breach_at_threshold() {
    let mut monitor = RiskMonitor::new(dec!(10000))
        .add_rule(MaxDrawdownRule {
            threshold_pct: dec!(10),
        })
        .add_rule(MinEquityRule { floor: dec!(8000) });

    // No breach initially.
    assert!(monitor.update(dec!(10000)).is_empty());

    // 5% drawdown : no breach.
    assert!(monitor.update(dec!(9500)).is_empty());

    // 15% drawdown below floor : both breach.
    let breaches = monitor.update(dec!(7000));
    assert_eq!(breaches.len(), 2);
}

// ── Exact boundary tests (not off-by-one) ────────────────────────────────

/// MaxDrawdownRule uses strict `>`: drawdown exactly AT threshold must NOT breach.
#[test]
fn test_max_drawdown_at_exact_threshold_no_breach() {
    let rule = MaxDrawdownRule {
        threshold_pct: dec!(10),
    };
    // drawdown_pct == threshold → no breach (rule is `> threshold`)
    let breach = rule.check(dec!(9000), dec!(10));
    assert!(
        breach.is_none(),
        "drawdown exactly at threshold must not trigger breach (rule is strictly >)"
    );
}

/// MaxDrawdownRule fires one epsilon above the threshold.
#[test]
fn test_max_drawdown_one_unit_above_threshold_breaches() {
    let rule = MaxDrawdownRule {
        threshold_pct: dec!(10),
    };
    let breach = rule.check(dec!(0), dec!(10.01));
    assert!(
        breach.is_some(),
        "drawdown 0.01 above threshold must trigger breach"
    );
}

/// MinEquityRule uses strict `<`: equity exactly AT floor must NOT breach.
#[test]
fn test_min_equity_at_exact_floor_no_breach() {
    let rule = MinEquityRule { floor: dec!(5000) };
    let breach = rule.check(dec!(5000), dec!(0));
    assert!(
        breach.is_none(),
        "equity exactly at floor must not trigger breach (rule is strictly <)"
    );
}

/// MinEquityRule fires one unit below the floor.
#[test]
fn test_min_equity_one_unit_below_floor_breaches() {
    let rule = MinEquityRule { floor: dec!(5000) };
    let breach = rule.check(dec!(4999), dec!(0));
    assert!(
        breach.is_some(),
        "equity one unit below floor must trigger breach"
    );
}

/// Two rules: only one fires when equity is between the two thresholds.
#[test]
fn test_two_rules_only_one_fires_in_between() {
    let mut monitor = RiskMonitor::new(dec!(10000))
        .add_rule(MaxDrawdownRule {
            threshold_pct: dec!(5),
        }) // fires > 5%
        .add_rule(MinEquityRule { floor: dec!(8000) }); // fires < 8000
                                                        // 9% drawdown → equity ~9100; only MaxDrawdown fires, not MinEquity
    let breaches = monitor.update(dec!(9100));
    assert_eq!(breaches.len(), 1);
    assert_eq!(breaches[0].rule, "max_drawdown");
}

/// Three rules: AND logic means all three fire when all conditions are met.
#[test]
fn test_three_rules_all_fire_simultaneously() {
    let mut monitor = RiskMonitor::new(dec!(10000))
        .add_rule(MaxDrawdownRule {
            threshold_pct: dec!(5),
        })
        .add_rule(MinEquityRule { floor: dec!(9000) })
        .add_rule(MinEquityRule { floor: dec!(7000) });
    let breaches = monitor.update(dec!(6000)); // 40% DD, below 9000, below 7000
    assert_eq!(breaches.len(), 3);
}

/// No rules: monitor never breaches regardless of equity.
#[test]
fn test_no_rules_never_breaches() {
    let mut monitor = RiskMonitor::new(dec!(10000));
    assert!(monitor.update(dec!(0)).is_empty());
    assert!(monitor.update(dec!(1_000_000)).is_empty());
}

#[test]
fn test_risk_monitor_recovers_after_new_peak() {
    let mut monitor = RiskMonitor::new(dec!(10000)).add_rule(MaxDrawdownRule {
        threshold_pct: dec!(5),
    });

    // Drop to 9000 (10% drawdown) : breach.
    let b1 = monitor.update(dec!(9000));
    assert!(!b1.is_empty());

    // New peak : drawdown resets to 0.
    let b2 = monitor.update(dec!(11000));
    assert!(b2.is_empty());
}
