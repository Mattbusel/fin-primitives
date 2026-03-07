use fin_primitives::risk::{MaxDrawdownRule, MinEquityRule, RiskMonitor};
use rust_decimal_macros::dec;

#[test]
fn test_risk_monitor_breach_at_threshold() {
    let mut monitor = RiskMonitor::new(dec!(10000))
        .add_rule(MaxDrawdownRule { threshold_pct: dec!(10) })
        .add_rule(MinEquityRule { floor: dec!(8000) });

    // No breach initially.
    assert!(monitor.update(dec!(10000)).is_empty());

    // 5% drawdown — no breach.
    assert!(monitor.update(dec!(9500)).is_empty());

    // 15% drawdown below floor — both breach.
    let breaches = monitor.update(dec!(7000));
    assert_eq!(breaches.len(), 2);
}

#[test]
fn test_risk_monitor_recovers_after_new_peak() {
    let mut monitor = RiskMonitor::new(dec!(10000))
        .add_rule(MaxDrawdownRule { threshold_pct: dec!(5) });

    // Drop to 9000 (10% drawdown) — breach.
    let b1 = monitor.update(dec!(9000));
    assert!(!b1.is_empty());

    // New peak — drawdown resets to 0.
    let b2 = monitor.update(dec!(11000));
    assert!(b2.is_empty());
}
