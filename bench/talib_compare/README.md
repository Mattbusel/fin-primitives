# fin-primitives compared with TA-Lib

A reproducible check of 22 fin-primitives indicators (27 output series) against
[TA-Lib](https://ta-lib.org/), the standard C indicator library, on the same bars:
do they give the same numbers, and how fast is each one?

Short answer: wherever both libraries define an indicator the same way, they agree to
within 2e-11 (for the EMA-seeded ones, once the seed has washed out). Where the numbers
differ, it is for a reason listed below: two definitional choices, seeding that washes out,
and ADX ties that float64 cannot see. TA-Lib is much faster: 5 to 380 times faster per bar
in batch mode (median about 100 times). fin-primitives does exact decimal arithmetic one
bar at a time, and that is where the time goes.

## Run it

```bash
python -m venv venv && venv/bin/pip install TA-Lib numpy   # Windows: venv\Scripts\pip
PYTHON=venv/bin/python ./run.sh
```

`run.sh` does three things:

1. `gen_data.py` writes 100,000 seeded OHLCV bars to `data/bars.csv` (seed 20260930, no
   download). Log prices follow a slowly mean-reverting random walk with 1.2% per-bar
   volatility, so prices stay between about 40 and 260; open gaps, intrabar wicks and
   lognormal volume tied to the size of the move make the bars look like real daily data.
   Every value is written as a short decimal string (cents, whole shares), so both
   libraries read exactly the same numbers.
2. `cargo run --release` (this folder is a small standalone crate depending on
   `fin-primitives` by path) feeds the bars one at a time to each indicator's `update()`
   and writes every output to `data/fp_values.csv`, then times the same loop (best of 5
   runs over all 100,000 bars) into `data/fp_timing.csv`.
3. `compare.py` runs TA-Lib on the same arrays, compares, times TA-Lib, and prints the
   report. `data/report.md` and `data/results.json` in this folder are the recorded run
   quoted below.

## Machine and versions

13th Gen Intel(R) Core(TM) i7-13700KF, Windows 11 Pro, rustc 1.91.0 (release profile, thin
LTO), fin-primitives 2.14.3, TA-Lib 0.8.1 (C library bundled in the `TA-Lib` 0.8.1 Python
wheel), numpy 2.5.3, Python 3.12.10. Run on 2026-09-30.

## Agreement

Max absolute difference between the two libraries, over every bar where both give a value,
and again after skipping the first 1,000 bars. "First value" is the index of the first bar
each library produces a value on.

| indicator | first value (fp / TA-Lib) | all common bars | after bar 1000 | bars > 1e-9 after 1000 | vs matching formula |
|---|---|---|---|---|---|
| SMA(20) | 19 / 19 | 9.8e-13 | 9.8e-13 | 0 |  |
| EMA(20) | 19 / 19 | 5.7e-14 | 5.7e-14 | 0 |  |
| WMA(20) | 19 / 19 | 2.8e-12 | 2.8e-12 | 0 |  |
| DEMA(20) | 38 / 38 | 1.4e-13 | 1.4e-13 | 0 |  |
| TEMA(20) | 57 / 57 | 3.1e-13 | 3.1e-13 | 0 |  |
| TRIX(15) | 43 / 43 | 6.6e-02 | 2.6e-14 | 0 |  |
| KAMA(10) | 10 / 10 | 2.0e-02 | 1.1e-13 | 0 |  |
| RSI(14) | 14 / 14 | 1.8e-13 | 1.8e-13 | 0 |  |
| MACD(12,26,9) histogram | 33 / 33 | 3.6e-02 | 5.0e-14 | 0 |  |
| Bollinger(20, 2) upper | 19 / 19 | 1.6e-11 | 1.6e-11 | 0 |  |
| Bollinger(20, 2) middle | 19 / 19 | 9.8e-13 | 9.8e-13 | 0 |  |
| Bollinger(20, 2) lower | 19 / 19 | 1.6e-11 | 1.6e-11 | 0 |  |
| Bollinger(20, 2) %B | 19 / 19 | 3.4e-12 | 3.4e-12 | 0 |  |
| ATR(14) | 14 / 14 | 7.1e-01 | 7.1e-01 | 99000 | SMA(TRANGE, 14): 1.2e-14 |
| NATR(14) | 14 / 14 | 4.9e-01 | 4.9e-01 | 99000 | SMA(TRANGE, 14) / close * 100: 8.4e-15 |
| Stochastic fast %K(14) | 13 / 15 | 6.7e-13 | 6.7e-13 | 0 |  |
| Stochastic fast %D(14, 3) | 15 / 15 | 1.3e-12 | 1.3e-12 | 0 |  |
| Williams %R(14) | 13 / 13 | 6.7e-13 | 6.7e-13 | 0 |  |
| CCI(20) | 19 / 19 | 1.0e-11 | 1.0e-11 | 0 |  |
| ROC(10) | 10 / 10 | 3.1e-14 | 3.1e-14 | 0 |  |
| Momentum(10) | 10 / 10 | 2.8e-14 | 2.8e-14 | 0 |  |
| MFI(14) | 14 / 14 | 1.3e-12 | 1.3e-12 | 0 |  |
| ADX(14) | 28 / 27 | 2.3e+01 | 2.4e+00 | 11375 |  |
| Aroon oscillator(14) | 14 / 14 | 1.4e-14 | 1.4e-14 | 0 |  |
| Aroon up(14) | 14 / 14 | 1.4e-14 | 1.4e-14 | 0 |  |
| Aroon down(14) | 14 / 14 | 1.4e-14 | 1.4e-14 | 0 |  |
| OBV | 0 / 0 | 1.1e+06 | 1.1e+06 | 99000 | OBV - volume[0]: 0 |

Prices are around 40 to 260, so differences of 2e-11 and below are float64 rounding.
`compare.py` checks which side it is on for the Bollinger upper band, the largest of them:
against a 40-digit Python `decimal` reference on every 37th bar, TA-Lib is off by up to
1.2e-11 and fin-primitives by 0 (it computes in 28-digit decimals).

### Why some numbers differ

- **ATR and NATR: definition.** fin-primitives' `Atr` is the simple moving average of true
  range over the period (its docs say so). TA-Lib's `ATR` is Wilder's smoothed average. The
  last column shows fin-primitives matches TA-Lib's own `SMA(TRANGE(...), 14)` to 1.2e-14.
  If you need Wilder's ATR, it is not the `Atr` type.
- **OBV: starting value.** fin-primitives starts the running total at 0. TA-Lib starts it at
  the first bar's volume. Every step is identical: fin-primitives equals TA-Lib's OBV minus
  `volume[0]` exactly (difference 0).
- **MACD, TRIX and KAMA: seeding.** The recursions start from different seed values. For
  MACD, TA-Lib seeds the 12-bar EMA with the average of bars 14 to 25, where the 26-bar EMA
  is seeded, while fin-primitives seeds it with bars 0 to 11 (reproducing TA-Lib's choice in
  numpy matches its MACD line to 4e-14). TRIX and KAMA also differ in their first values.
  Early values differ by up to 0.07; the gap decays geometrically and is below 3e-13 after
  bar 1,000.
- **ADX: seeding, then ties.** fin-primitives emits its first ADX one bar later (bar 28 vs
  27) from a differently seeded directional-movement sum, which decays like the EMA seeds.
  After that, ADX differs only after bars where the up move and the down move are exactly
  equal. With exact decimals, `up == down` gives +DM = -DM = 0 (Wilder's rule). In float64,
  `high[t] - high[t-1]` and `low[t-1] - low[t]` for the same cent amount can differ in the
  last bit, and TA-Lib then credits one side. This series has 128 exact ties; 39 of them
  compare unequal in float64, and each one sends TA-Lib's ADX off by up to 2.4 points for a
  few hundred bars. More than 600 bars away from such a tie, the two agree to 3.7e-13.
  Here the difference is a float64 artefact in TA-Lib, not a different definition.
- **Stochastic %K first bar.** TA-Lib's `STOCHF` returns %K only once %D exists (bar 15);
  fin-primitives' `StochasticK` returns it as soon as it has 14 bars (bar 13). The values
  match wherever both exist.

Running this comparison found one real bug, fixed in 2.14.3: `Aroon::down()` broke ties
toward the *oldest* low (Rust's `min_by` returns the first minimum, while `max_by` returns
the last maximum), so Aroon Down read 0 instead of 100 when the newest bar tied the
window's oldest low. Before the fix it differed from TA-Lib by 100 on some bars; now it
matches to 1.4e-14.

## Throughput

Nanoseconds per bar, lower is better. fin-primitives: one `update()` call per bar on a fresh
indicator, best of 5 passes over 100,000 bars. TA-Lib batch: one call over the whole
100,000-bar array, best of 20, divided by 100,000. TA-Lib per bar from Python: calling
`talib.stream.<name>` once per bar on the trailing 200 bars, for 5,000 bars. TA-Lib keeps no
state between calls, so this is how you use it bar by bar from Python.

| indicator | fin-primitives streaming `update()` | TA-Lib batch (whole array) | TA-Lib per bar from Python |
|---|---|---|---|
| SMA(20) | 74 | 0.8 | 650 |
| EMA(20) | 149 | 1.3 | 716 |
| WMA(20) | 106 | 0.8 |  |
| DEMA(20) | 355 | 1.5 |  |
| TEMA(20) | 569 | 3.3 |  |
| TRIX(15) | 305 | 1.7 |  |
| KAMA(10) | 401 | 2.9 |  |
| RSI(14) | 252 | 2.1 | 877 |
| MACD(12,26,9) | 362 | 1.7 | 1044 |
| Bollinger(20, 2) | 1023 | 2.7 | 1743 |
| ATR(14) | 86 | 0.8 | 749 |
| NATR(14) | 141 | 1.1 |  |
| Stochastic %K(14) | 174 | 7.7 | 1708 |
| Stochastic %D(14, 3) | 259 | 7.7 |  |
| Williams %R(14) | 170 | 1.9 |  |
| CCI(20) | 1060 | 12.2 |  |
| ROC(10) | 38 | 0.8 |  |
| Momentum(10) | 7 | 0.2 |  |
| MFI(14) | 589 | 5.9 |  |
| ADX(14) | 794 | 6.2 |  |
| Aroon(14) | 205 | 6.6 |  |
| OBV | 13 | 2.8 | 710 |

Timings move by a few percent between runs (the Python per-bar column by up to about 25%).

What this means:

- **TA-Lib is faster, by a lot.** Over a whole array it takes 0.2 to 12 ns per bar;
  fin-primitives takes 7 to 1,060 ns per bar: a median of about 100 times slower, from
  5 times (OBV) to 380 times (Bollinger). Two reasons: every
  fin-primitives operation is exact 96-bit decimal arithmetic (`rust_decimal`) with overflow
  checks, while TA-Lib runs float64 loops over contiguous arrays; and several
  fin-primitives indicators (SMA, WMA, Bollinger, CCI, the stochastics) re-scan their
  window on every bar instead of keeping a running sum. For a backtest over millions of
  bars where float64 is acceptable, TA-Lib is the faster tool.
- **Bar by bar, fin-primitives is the faster of the two here**, but only because calling
  TA-Lib from Python costs about 0.65 to 1.75 microseconds per call, mostly interpreter and
  array-slicing overhead. That column measures Python, not TA-Lib's C code; from C, calling
  TA-Lib on a trailing window per bar would be much cheaper.
- **Why use fin-primitives anyway:** it keeps state, so each new bar costs one `update()`
  with no window to re-pass; prices never become floats, so ties and cent values are exact
  (see ADX above); and it is plain Rust with no C library to build or ship.

Even the slowest indicator here (CCI, about 1.1 microseconds per bar) handles roughly
900,000 bars per second on one core.
