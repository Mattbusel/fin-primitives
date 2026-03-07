use fin_primitives::ohlcv::OhlcvBar;
use fin_primitives::risk::{DrawdownTracker, MaxDrawdownRule, RiskRule};
use fin_primitives::signals::indicators::{Rsi, Sma};
use fin_primitives::signals::Signal;
use fin_primitives::signals::SignalValue;
use fin_primitives::types::*;
use rust_decimal::Decimal;
use proptest::prelude::*;

fn bar(close: Decimal) -> OhlcvBar {
    let p = Price::new(close).unwrap_or_else(|_| Price::new(Decimal::ONE).unwrap());
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

proptest! {
    #[test]
    fn test_drawdown_pct_always_non_negative(
        initial in 1u64..=1_000_000,
        current in 0u64..=1_000_000,
    ) {
        let init = Decimal::from(initial);
        let curr = Decimal::from(current);
        let mut tracker = DrawdownTracker::new(init);
        tracker.update(curr);
        prop_assert!(tracker.current_drawdown_pct() >= Decimal::ZERO);
    }

    #[test]
    fn test_rsi_always_in_0_to_100(closes in prop::collection::vec(1u32..=500, 16..=30)) {
        let mut rsi = Rsi::new("rsi14", 14);
        let mut last_val: Option<Decimal> = None;
        for c in &closes {
            let b = bar(Decimal::from(*c));
            if let Ok(SignalValue::Scalar(v)) = rsi.update(&b) {
                last_val = Some(v);
            }
        }
        if let Some(v) = last_val {
            prop_assert!(v >= Decimal::ZERO, "RSI below 0: {v}");
            prop_assert!(v <= Decimal::ONE_HUNDRED, "RSI above 100: {v}");
        }
    }

    #[test]
    fn test_sma_value_bounded_by_input_range(
        closes in prop::collection::vec(1u32..=1000, 5..=10),
    ) {
        let mut sma = Sma::new("sma5", 5);
        let min_val = Decimal::from(*closes.iter().min().unwrap());
        let max_val = Decimal::from(*closes.iter().max().unwrap());
        let mut last_val: Option<Decimal> = None;
        for c in &closes {
            let b = bar(Decimal::from(*c));
            if let Ok(SignalValue::Scalar(v)) = sma.update(&b) {
                last_val = Some(v);
            }
        }
        if let Some(v) = last_val {
            prop_assert!(v >= min_val, "SMA {v} below min {min_val}");
            prop_assert!(v <= max_val, "SMA {v} above max {max_val}");
        }
    }

    #[test]
    fn test_ohlcv_bar_invariant_high_gte_low(
        low_cents in 1u32..=10000,
        range_cents in 0u32..=5000,
    ) {
        let low = Decimal::from(low_cents);
        let high = Decimal::from(low_cents + range_cents);
        let bar = OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(low).unwrap(),
            high: Price::new(high).unwrap(),
            low: Price::new(low).unwrap(),
            close: Price::new(high).unwrap(),
            volume: Quantity::zero(),
            ts_open: NanoTimestamp(0),
            ts_close: NanoTimestamp(1),
            tick_count: 1,
        };
        prop_assert!(bar.validate().is_ok());
        prop_assert!(bar.high.value() >= bar.low.value());
    }

    #[test]
    fn test_symbol_roundtrip_display(s in "[A-Z]{1,8}") {
        let sym = Symbol::new(s.clone()).unwrap();
        prop_assert_eq!(format!("{sym}"), s);
    }
}
