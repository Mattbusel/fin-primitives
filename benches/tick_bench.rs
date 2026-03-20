use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use fin_primitives::ohlcv::{OhlcvAggregator, Timeframe};
use fin_primitives::orderbook::{BookDelta, DeltaAction, OrderBook};
use fin_primitives::tick::{Tick, TickFilter};
use fin_primitives::types::*;
use rust_decimal_macros::dec;

fn bench_tick_filter(c: &mut Criterion) {
    let sym = Symbol::new("BTC").unwrap();
    let tick = Tick::new(
        sym.clone(),
        Price::new(dec!(100)).unwrap(),
        Quantity::new(dec!(1)).unwrap(),
        Side::Bid,
        NanoTimestamp::new(0),
    );
    let filter = TickFilter::new().symbol(sym);
    c.bench_function("tick_filter_match", |b| b.iter(|| filter.matches(&tick)));
}

fn bench_orderbook_delta(c: &mut Criterion) {
    c.bench_function("orderbook_apply_delta", |b| {
        b.iter_batched(
            || {
                let mut book = OrderBook::new(Symbol::new("BTC").unwrap());
                book.apply_delta(BookDelta {
                    side: Side::Bid,
                    price: Price::new(dec!(100)).unwrap(),
                    quantity: Quantity::new(dec!(5)).unwrap(),
                    action: DeltaAction::Set,
                    sequence: 1,
                })
                .unwrap();
                book
            },
            |mut book| {
                let _ = book.apply_delta(BookDelta {
                    side: Side::Ask,
                    price: Price::new(dec!(101)).unwrap(),
                    quantity: Quantity::new(dec!(3)).unwrap(),
                    action: DeltaAction::Set,
                    sequence: 2,
                });
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_ohlcv_push_tick(c: &mut Criterion) {
    let sym = Symbol::new("BTC").unwrap();
    let tick = Tick::new(
        sym.clone(),
        Price::new(dec!(100)).unwrap(),
        Quantity::new(dec!(1)).unwrap(),
        Side::Bid,
        NanoTimestamp::new(0),
    );
    c.bench_function("ohlcv_push_tick", |b| {
        b.iter_batched(
            || OhlcvAggregator::new(sym.clone(), Timeframe::Seconds(60)).unwrap(),
            |mut agg| {
                let _ = agg.push_tick(&tick);
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(
    benches,
    bench_tick_filter,
    bench_orderbook_delta,
    bench_ohlcv_push_tick
);
criterion_main!(benches);
