# fin-primitives vs ta, yata and black_scholes

Criterion benchmarks and an agreement check on identical inputs. This folder is a small
standalone crate (`publish = false`) that depends on fin-primitives by path and on the
crates.io releases of [`ta`](https://crates.io/crates/ta) 0.5,
[`yata`](https://crates.io/crates/yata) 0.7 and
[`black_scholes`](https://crates.io/crates/black_scholes) 0.11.

```bash
cargo run --release          # agreement check
cargo bench --bench vs       # timings
```

Input: 100,000 closing prices from a seeded random walk, rounded to cents, given to each
library one value at a time (streaming), as a live feed would. fin-primitives gets them as
`Decimal`, the others as `f64` holding the same values.

## Agreement (`cargo run --release`)

| | max difference from fin-primitives after bar 1,000 |
|---|---|
| SMA(20), ta / yata | 5.3e-13 / 9.9e-13 |
| EMA(20), ta / yata | 7.1e-14 / 4.3e-14 |
| RSI(14), ta | **20.4** (last bar: fin-primitives 43.66, ta 36.22) |
| Black-Scholes call (S=100, K=105, r=5%, sigma=20%, T=0.5), black_scholes | 7.1e-15 |

The RSI gap is a definition difference, not rounding: ta smooths gains and losses with an
EMA of factor `2/(n+1)`, while fin-primitives uses Wilder's `1/n`, which is what TA-Lib and
most charting packages call RSI (fin-primitives matches TA-Lib's RSI to 1.8e-13, see
`../talib_compare`).

## Speed (`cargo bench --bench vs`)

Run on 2026-10-03, i7-13700KF, Windows 11, rustc 1.91.0, `opt-level=3`, thin LTO. The
machine was also running other builds, so treat the numbers as ratios, not absolutes.

| benchmark | fin-primitives | ta | yata | black_scholes |
|---|---|---|---|---|
| SMA(20), 100k closes | 3.87 ms | 0.098 ms | 0.069 ms | |
| EMA(20), 100k closes | 15.9 ms | 0.138 ms | 0.152 ms | |
| RSI(14), 100k closes | 27.1 ms | 0.171 ms | | |
| Black-Scholes call price | 25 ns | | | 28 ns |
| Implied vol (from that price) | 193 ns | | | 73 ns |

The f64 crates are 40 to 160 times faster on indicators. That is the cost of exact
`Decimal` arithmetic: every add, multiply and divide is a 96-bit integer operation with
rescaling instead of one float instruction. At about 39 ns per SMA update, 160 ns per
EMA update and 270 ns per RSI update, one core still runs 4 to 25 million indicator updates
per second, but for crunching years of history in float64 the other crates (or TA-Lib) are
the faster choice.

Pick fin-primitives when exact values matter (prices that round-trip, P&L that adds up to
the cent), when you want TA-Lib-compatible definitions, typed errors and validated inputs,
or when you need the order book, positions, risk and options pieces in the same crate.

Changes measured here during 2.15: `Sma` keeps a running sum instead of re-summing the
window (7.65 ms to 3.86 ms), and `implied_vol` moved from bisection to safeguarded Newton
(1.22 us to 0.19 us; it now stops on a 1e-12 vol step instead of a 1e-8 price error).
