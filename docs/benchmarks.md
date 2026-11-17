# Benchmarks: `fin_primitives.fast` against other Python indicator libraries

One whole-series call per indicator on 100,000 bars, the same float64 numpy arrays for every
library. Measured on 2026-11-16.

```bash
pip install fin-primitives numpy pandas polars TA-Lib kand talipp polars-talib "pandas-ta-classic==0.9.0"
python bench/python/bench_fast.py
```

The script prints both tables below. Input: a seeded random walk (seed 20261116), prices
rounded to cents, highs and lows around them. Each cell is the best of 7 runs (talipp: of 2,
it is fed the whole list once and updates one value at a time). pandas-ta-classic runs its
own implementation (`talib=False`); polars-talib calls TA-Lib's C code through Polars.

## Time per call (milliseconds, lower is better)

| indicator | fin_primitives.fast | TA-Lib | kand | pandas-ta-classic | polars-talib | talipp |
|---|---|---|---|---|---|---|
| SMA(20) | 0.21 | 0.08 | 0.09 | 1.00 | 0.12 | 39.51 |
| EMA(20) | 0.17 | 0.08 | 0.14 | 1.15 | 0.18 | 53.96 |
| RSI(14) | 0.77 | 0.22 | 0.48 | 4.52 | 0.26 | 82.58 |
| ATR(14) | 0.24 | 0.10 | 0.36 | 2.69 | 0.19 | 65.97 |
| MACD(12,26,9) | 1.13 | 0.21 | 1.78 | 60.83 | 0.27 | 301.49 |
| BBANDS(20,2) | 1.16 | 0.35 | 1.49 | 14.49 | 0.40 | 339.24 |

What the numbers say:

- **TA-Lib is the fastest**, 2 to 5 times faster than `fin_primitives.fast` here, and
  polars-talib (TA-Lib underneath) is close to it. If raw speed in float64 is all you need and
  you can install TA-Lib's C library, use it.
- `fin_primitives.fast` and kand (also Rust) are within a factor of about 2.3 of each other,
  each ahead on some rows.
- `fin_primitives.fast` is 5 to 54 times faster than pandas-ta-classic, whose numbers it
  reproduces (same warm-up, same seeds), and 107 to 317 times faster than talipp.

## Agreement with TA-Lib (largest absolute difference, bars 1,000 onward)

| indicator | fin_primitives.fast | kand | pandas-ta-classic | polars-talib | talipp |
|---|---|---|---|---|---|
| SMA(20) | 3.0e-12 | 4.5e-12 | 3.0e-12 | 0.0e+00 | 4.0e-12 |
| EMA(20) | 3.4e-13 | 1.1e-13 | 3.4e-13 | 1.1e-13 | 3.4e-13 |
| RSI(14) | 6.4e-14 | 5.7e-14 | 6.4e-14 | 0.0e+00 | 5.7e-14 |
| ATR(14) | 8.9e-15 | 5.3e-15 | 8.9e-15 | 0.0e+00 | 7.1e-15 |
| MACD(12,26,9) | 4.5e-13 | 2.3e-13 | 4.5e-13 | 2.3e-13 | 4.5e-13 |
| BBANDS(20,2) | 3.0e-12 | 4.5e-12 | 3.0e-12 | 0.0e+00 | 4.0e-12 |

Every library agrees with TA-Lib to float rounding once the seeds have washed out;
`fin_primitives.fast` and pandas-ta-classic give the same differences because they compute the
same way.

## Machine and versions

13th Gen Intel Core i7-13700KF (24 logical processors), Windows 11 Pro (10.0.26200), Python
3.12.10. fin-primitives 3.4.1 (wheel built locally with `maturin build --release`), TA-Lib
0.8.3, kand 0.2.2, pandas-ta-classic 0.9.0, talipp 2.7.0, polars-talib 0.2.1, numpy 2.5.4,
pandas 3.1.1, polars 2.1.0.

## How repeatable it is

The machine was running other builds during the measurement. Two back-to-back runs of the
script agreed within 10% on most cells, but not all: `fin_primitives.fast` RSI(14) read 0.55
and 0.77 ms, kand MACD 0.90 and 1.78 ms. Treat the tables as ratios between libraries, and
rerun the script on your own machine for absolute numbers.

For the exact-decimal side of the crate (streaming `Decimal` indicators against `ta`, `yata`
and TA-Lib in Rust), see `bench/competitors` and `bench/talib_compare`.
