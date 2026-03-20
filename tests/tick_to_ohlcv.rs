use fin_primitives::ohlcv::{OhlcvAggregator, Timeframe};
use fin_primitives::tick::Tick;
use fin_primitives::types::*;
use rust_decimal_macros::dec;

#[test]
fn test_tick_to_ohlcv_produces_bars() {
    let sym = Symbol::new("BTC").unwrap();
    let mut agg = OhlcvAggregator::new(sym.clone(), Timeframe::Seconds(10)).unwrap();
    let prices = [dec!(100), dec!(102), dec!(101), dec!(105), dec!(103)];
    let mut bars = Vec::new();
    for (i, p) in prices.iter().enumerate() {
        let tick = Tick::new(
            sym.clone(),
            Price::new(*p).unwrap(),
            Quantity::new(dec!(1)).unwrap(),
            Side::Bid,
            NanoTimestamp::new(i as i64 * 5_000_000_000),
        );
        bars.extend(agg.push_tick(&tick).unwrap());
    }
    if let Some(bar) = agg.flush() {
        bars.push(bar);
    }
    assert!(!bars.is_empty());
    for bar in &bars {
        bar.validate().unwrap();
    }
}

#[test]
fn test_tick_to_ohlcv_high_low_correct() {
    let sym = Symbol::new("ETH").unwrap();
    let mut agg = OhlcvAggregator::new(sym.clone(), Timeframe::Seconds(60)).unwrap();
    let nanos_per_min = 60_000_000_000_i64;
    // Feed 3 ticks in the same bucket
    let prices_and_ts = [
        (dec!(200), 0i64),
        (dec!(210), 1_000_000_000),
        (dec!(195), 2_000_000_000),
    ];
    for (p, ts) in prices_and_ts {
        agg.push_tick(&Tick::new(
            sym.clone(),
            Price::new(p).unwrap(),
            Quantity::new(dec!(1)).unwrap(),
            Side::Ask,
            NanoTimestamp::new(ts),
        ))
        .unwrap();
    }
    // Trigger completion with a tick in the next minute
    let result = agg
        .push_tick(&Tick::new(
            sym.clone(),
            Price::new(dec!(205)).unwrap(),
            Quantity::new(dec!(1)).unwrap(),
            Side::Ask,
            NanoTimestamp::new(nanos_per_min),
        ))
        .unwrap();
    let bar = result.into_iter().next().unwrap();
    assert_eq!(bar.open.value(), dec!(200));
    assert_eq!(bar.high.value(), dec!(210));
    assert_eq!(bar.low.value(), dec!(195));
    assert_eq!(bar.close.value(), dec!(195));
    bar.validate().unwrap();
}
