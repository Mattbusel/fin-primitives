use fin_primitives::ohlcv::OhlcvBar;
use fin_primitives::signals::indicators::{Ema, Rsi, Sma};
use fin_primitives::signals::pipeline::SignalPipeline;
use fin_primitives::signals::SignalValue;
use fin_primitives::types::*;
use rust_decimal::Decimal;

fn bar(close: &str) -> OhlcvBar {
    let p = Price::new(close.parse::<Decimal>().unwrap()).unwrap();
    OhlcvBar {
        symbol: Symbol::new("X").unwrap(),
        open: p,
        high: p,
        low: p,
        close: p,
        volume: Quantity::zero(),
        ts_open: NanoTimestamp::new(0),
        ts_close: NanoTimestamp::new(1),
        tick_count: 1,
    }
}

#[test]
fn test_signal_pipeline_all_ready_after_period() {
    let period = 5;
    let mut pipeline = SignalPipeline::new()
        .add(Sma::new("sma", period).unwrap())
        .add(Ema::new("ema", period).unwrap())
        .add(Rsi::new("rsi", period).unwrap());

    let prices = [
        "100", "102", "101", "103", "105", "107", "104", "106", "108", "110",
    ];
    let mut last_map = None;
    for p in &prices {
        last_map = Some(pipeline.update(&bar(p)));
    }

    // After enough bars, all three should have scalar values.
    let map = last_map.unwrap();
    for name in &["sma", "ema", "rsi"] {
        match map.get(name).unwrap() {
            SignalValue::Scalar(_) => {}
            SignalValue::Unavailable => panic!("{name} should be ready"),
        }
    }
    assert_eq!(pipeline.ready_count(), 3);
}

#[test]
fn test_signal_pipeline_not_ready_before_period() {
    let mut pipeline = SignalPipeline::new().add(Sma::new("sma5", 5).unwrap());

    // Feed only 4 bars.
    for p in &["100", "102", "104", "106"] {
        pipeline.update(&bar(p));
    }
    assert_eq!(pipeline.ready_count(), 0);
}
