//! fin-primitives vs ta, yata and black_scholes on identical inputs.
//! Run: `cargo bench` in this folder. Every indicator group processes the same
//! 100,000 closes one value at a time (streaming), the way a live feed would.

use competitors::closes;
use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use fin_primitives::options::greeks::{bsm_price_q, implied_vol, BSMInputs, OptionType};
use fin_primitives::signals::indicators::{Ema, Rsi, Sma};
use fin_primitives::signals::{BarInput, Signal};
use ta::Next;
use yata::core::Method;

const N: usize = 100_000;

fn indicators(c: &mut Criterion) {
    let (d, f) = closes(N);
    let bars: Vec<BarInput> = d.iter().map(|&x| BarInput::from_close(x)).collect();

    let mut g = c.benchmark_group("sma20");
    g.throughput(Throughput::Elements(N as u64));
    g.bench_function("fin-primitives (Decimal)", |b| {
        b.iter(|| {
            let mut s = Sma::new("sma", 20).unwrap();
            for x in &bars {
                black_box(s.update(x).unwrap());
            }
        })
    });
    g.bench_function("ta (f64)", |b| {
        b.iter(|| {
            let mut s = ta::indicators::SimpleMovingAverage::new(20).unwrap();
            for &x in &f {
                black_box(s.next(x));
            }
        })
    });
    g.bench_function("yata (f64)", |b| {
        b.iter(|| {
            let mut s = yata::methods::SMA::new(20, &f[0]).unwrap();
            for x in &f {
                black_box(s.next(x));
            }
        })
    });
    g.finish();

    let mut g = c.benchmark_group("ema20");
    g.throughput(Throughput::Elements(N as u64));
    g.bench_function("fin-primitives (Decimal)", |b| {
        b.iter(|| {
            let mut s = Ema::new("ema", 20).unwrap();
            for x in &bars {
                black_box(s.update(x).unwrap());
            }
        })
    });
    g.bench_function("ta (f64)", |b| {
        b.iter(|| {
            let mut s = ta::indicators::ExponentialMovingAverage::new(20).unwrap();
            for &x in &f {
                black_box(s.next(x));
            }
        })
    });
    g.bench_function("yata (f64)", |b| {
        b.iter(|| {
            let mut s = yata::methods::EMA::new(20, &f[0]).unwrap();
            for x in &f {
                black_box(s.next(x));
            }
        })
    });
    g.finish();

    let mut g = c.benchmark_group("rsi14");
    g.throughput(Throughput::Elements(N as u64));
    g.bench_function("fin-primitives (Decimal, Wilder)", |b| {
        b.iter(|| {
            let mut s = Rsi::new("rsi", 14).unwrap();
            for x in &bars {
                black_box(s.update(x).unwrap());
            }
        })
    });
    g.bench_function("ta (f64, EMA smoothing)", |b| {
        b.iter(|| {
            let mut s = ta::indicators::RelativeStrengthIndex::new(14).unwrap();
            for &x in &f {
                black_box(s.next(x));
            }
        })
    });
    g.finish();
}

fn options(c: &mut Criterion) {
    let inp = BSMInputs { S: 100.0, K: 105.0, r: 0.05, q: 0.0, sigma: 0.2, T: 0.5 };
    let price = bsm_price_q(&inp, OptionType::Call);

    let mut g = c.benchmark_group("bs_call_price");
    g.bench_function("fin-primitives", |b| b.iter(|| bsm_price_q(black_box(&inp), OptionType::Call)));
    g.bench_function("black_scholes", |b| {
        b.iter(|| black_scholes::call(black_box(100.0), 105.0, 0.05, 0.2, 0.5))
    });
    g.finish();

    let mut g = c.benchmark_group("bs_implied_vol");
    let guess = BSMInputs { sigma: 0.5, ..inp };
    g.bench_function("fin-primitives (safeguarded Newton)", |b| {
        b.iter(|| implied_vol(black_box(price), &guess, OptionType::Call))
    });
    g.bench_function("black_scholes", |b| {
        b.iter(|| black_scholes::call_iv(black_box(price), 100.0, 105.0, 0.05, 0.5))
    });
    g.finish();
}

criterion_group!(benches, indicators, options);
criterion_main!(benches);
