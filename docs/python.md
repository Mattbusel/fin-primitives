# fin-primitives for Python

Trading indicators computed in exact decimal arithmetic by the
[fin-primitives](https://github.com/Mattbusel/fin-primitives) Rust crate. All 743 of them
(RSI, EMA, SMA, MACD, Bollinger, ATR, ADX, VWAP, Ichimoku, SuperTrend and the rest) through
one class, with streaming updates and batch helpers for lists, numpy, pandas and polars.

```bash
pip install fin-primitives
```

One abi3 wheel per platform covers CPython 3.9 and every later version: Linux x86_64 and
aarch64 (manylinux and musllinux), macOS x86_64 and arm64, Windows x86_64. No runtime
dependencies. numpy, pandas and polars are used only if you pass their types in.

Replacing pandas-ta in an existing project? See [`fin_primitives.ta`](#pandas-ta-functions-fin_primitivesta)
below and the [integration guide](https://github.com/Mattbusel/fin-primitives/blob/main/docs/INTEGRATION.md).

## Streaming

```python
from decimal import Decimal
from fin_primitives import Indicator

rsi = Indicator("rsi", 14)
for close in ["101.25", 101.5, 102, Decimal("101.75")]:   # str, int, float or Decimal
    value = rsi.update(close)    # None while warming up, then a decimal.Decimal
```

Indicators that need the whole bar (ATR, ADX, MFI, OBV, VWAP, ...) take it with `update_bar`:

```python
atr = Indicator("atr", 14)
atr.update_bar(close="101.5", high="102.0", low="100.9")
# open defaults to close, volume to 0
```

Parameters can be positional, keywords, or left out for the common ones:

```python
Indicator("macd", 12, 26, 9)
Indicator("macd", fast_period=12, slow_period=26, signal_period=9)
Indicator("macd")                       # fin_primitives.DEFAULTS["macd"] == (12, 26, 9)
Indicator("bollinger_b", 20, "2")       # decimal parameters accept str/int/float/Decimal
```

`reset()`, `is_ready`, `period`, `key`, `params` and `label` (e.g. `"macd_12_26_9"`) are there too.

## Exact mode and float mode

`mode="exact"` (the default) takes `str`, `int`, `float` or `decimal.Decimal` and returns
`decimal.Decimal`. A float is read as the shortest decimal that prints as that float, so
`0.1` is exactly `0.1`:

```python
>>> fp.sma([0.1, 0.2], 2)[-1]
Decimal('0.150')                        # float arithmetic gives 0.15000000000000002
```

`mode="float"` takes and returns floats. The arithmetic inside is the same exact decimal
code, and each result is the float nearest to the exact value (`float(Decimal)`), so the two
modes never disagree. Float mode is faster because it skips building `decimal.Decimal`
objects, and its batch path reads float64 buffers directly and releases the GIL.

```python
fast = Indicator("rsi", 14, mode="float")
fast.update(101.25)       # None, then floats
```

## Batch: lists, numpy, pandas, polars

```python
import fin_primitives as fp

fp.rsi(df["close"], 14)                          # pandas Series in, Series out (same index)
fp.macd(np_closes)                               # numpy float64 array in, float64 array out
fp.bollinger_b(pl_series, 20, 2)                 # polars Series in, polars Series out
fp.atr(df["high"], df["low"], df["close"], 14)
fp.compute("super_trend", df["close"], 10, 3, high=df["high"], low=df["low"])
```

| input | default mode | output | warm-up values |
|---|---|---|---|
| numpy float/int array | float | float64 array | `NaN` |
| pandas numeric Series | float | float64 Series, same index, named e.g. `rsi_14` | `NaN` |
| polars numeric Series | float | Float64 Series | null |
| list / tuple / iterable, object columns of `Decimal` | exact | same type of `Decimal` | `None` |

Pass `mode="exact"` or `mode="float"` to choose. In exact mode numpy and pandas give
object-dtype results holding `Decimal`; polars gives a Decimal Series. polars stores a
Decimal column with one precision (at most 38 digits) and one scale for every row, so when
the results need more than that (28 decimal places in one row and 11 digits before the
point in another), the result is an Object Series of `decimal.Decimal` instead. Values are
never turned into null.

`Indicator.batch(close, high=None, low=None, open=None, volume=None)` does the same on an
existing indicator and keeps its state, so a long series can be fed in chunks and live
updates can follow a historical warm-up.

Missing values (`NaN`, `None`, null) raise `ValueError` naming the row: drop or fill them first.

Columns are paired by position. When `close` and another column are pandas Series, their
indexes must be equal, or `ValueError` is raised; take them from one DataFrame, or line them
up first (`close, high = close.align(high, join="inner")`). The result keeps `close`'s index.
numpy input must be 1-D (a 2-D array raises `ValueError` rather than being flattened).

Inputs must fit a 28-decimal-place decimal (magnitude below about 7.9e28). Extra decimal
places are rounded, but a non-zero value that would round to zero, such as `1e-30`, raises
`ValueError` instead of becoming `0`.

An `Indicator` holds native Rust state, so it cannot be pickled or deep-copied. Build a new
one with `Indicator(ind.key, *ind.params, mode=ind.mode)` and replay the data instead.
`fp.catalogue()` returns a fresh copy each call.

## Fast whole-series float64: `fin_primitives.fast`

For backtests that compute an indicator over a whole price history at once. Plain float64
instead of exact decimals: each function borrows the numpy array without copying it,
releases the GIL, runs one O(n) loop in Rust and returns new numpy arrays. The numbers
follow pandas-ta-classic 0.9.0, and so TA-Lib's seeding: the same leading `NaN`s, SMA seeds
and Wilder smoothing, with values identical to pandas-ta's (bit for bit for SMA, EMA, RSI,
ATR and MACD; within a few units in the last place for the rolling standard deviation).
Needs numpy.

```python
from fin_primitives import fast

fast.sma(close, 20)                              # first value at index 19
fast.ema(close, 20)                              # SMA seed, alpha = 2 / 21
fast.rsi(close, 14)                              # Wilder, first value at index 14
fast.atr(high, low, close, 14)                   # Wilder, TA-Lib seed (first value at 14)
fast.atr(high, low, close, 14, seed="tradingview")   # ta.atr: bar 0 is high - low (first at 13)
line, signal, hist = fast.macd(close, 12, 26, 9)
lower, mid, upper = fast.bbands(close, 20, 2.0)  # ddof=0, as pandas-ta and TA-Lib
fast.stdev(close, 20, ddof=0)                    # one-pass rolling standard deviation
fast.variance(close, 20), fast.rma(close, 14), fast.true_range(high, low, close)
```

| function | pandas-ta-classic | first value at |
|---|---|---|
| `sma(close, period)` | `ta.sma` | `period - 1` |
| `ema(close, period)` | `ta.ema` | `period - 1` |
| `rma(close, period)` | `ta.rma` | `period - 1` |
| `rsi(close, period)` | `ta.rsi` | `period` |
| `atr(high, low, close, period, seed="talib")` | `ta.atr` | `period` (`period - 1` with `seed="tradingview"`) |
| `true_range(high, low, close, seed="talib")` | `ta.true_range` | `1` (`0` with `seed="tradingview"`) |
| `macd(close, fast, slow, signal)` | `ta.macd` | line `slow - 1`, signal and histogram `slow + signal - 2` |
| `bbands(close, period, std, ddof=0)` | `ta.bbands` (lower, mid, upper) | `period - 1` |
| `stdev(close, period, ddof=0)` / `variance(...)` | `ta.stdev` / `ta.variance` | `period - 1` |

Input can be a numpy array, a pandas Series (the result is a Series with the same index) or
anything `numpy.asarray` accepts. Input shorter than the period gives all `NaN`, as in
pandas-ta. `NaN` input is handled the way pandas-ta handles it: a leading `NaN` run is
skipped and the seed starts after it, a window holding a `NaN` reads `NaN` (SMA, standard
deviation), a missing bar reads `NaN` while the average carries across it (EMA, RSI, ATR),
and MACD skips non-finite rows.

100,000 bars, median of 7 runs, Python 3.12 on Windows x86_64, against pandas-ta-classic
0.9.0 (pandas 3.1, numpy 2.5, no numba or TA-Lib):

| indicator | pandas-ta-classic | `fin_primitives.fast` | speed-up |
|---|---:|---:|---:|
| sma(20) | 1.4 ms | 0.2 ms | 7x |
| ema(20) | 1.6 ms | 0.2 ms | 9x |
| rsi(14) | 5.8 ms | 1.5 ms | 4x |
| macd(12, 26, 9) | 77.5 ms | 1.2 ms | 65x |
| bbands(20, 2) | 15.4 ms | 2.1 ms | 7x |
| atr(14) | 3.3 ms | 0.3 ms | 12x |

## pandas-ta functions: `fin_primitives.ta`

pandas-ta's function API (3.8.0). Code that calls `ta.rsi(df["close"], length=14)` moves
over by changing the import; the function names, the input series order, the parameter
names and defaults, and the output column names are pandas-ta's:

```python
from fin_primitives import ta          # was: import pandas_ta as ta

df["RSI_14"] = ta.rsi(df["close"], length=14)
macd = ta.macd(df["close"])            # MACD_12_26_9, MACDh_12_26_9, MACDs_12_26_9
st = ta.supertrend(df["high"], df["low"], df["close"], length=10, multiplier=3)
obv = ta.obv(df["close"], df["volume"])
```

It has the 65 indicators of `df.fp` below except `ichimoku`, whose pandas-ta function
returns a tuple with a forward span frame (use `df.fp.ichimoku()`). Every function is typed,
so mypy and pyright check the calls. The values are `df.fp`'s, and
`tests/test_ta.py` compares every function with the pandas-ta-classic 0.9.0 function of the
same name on two fixed 5,000 and 6,000 bar series. On 100,000 bars, RSI, MACD, Bollinger
Bands, ATR, ADX and MFI together take about 60 ms against about 880 ms for
pandas-ta-classic (`python/examples/pandas_ta_dropin.py`, Windows x86_64, Python 3.12).

Where it differs from pandas-ta: input shorter than the period gives `NaN` rather than
`None`; `talib=`, `mamode=`, `offset=`, `drift=` and `fillna=` raise `TypeError` instead of
changing the formula; series with different indexes raise `ValueError` instead of being
aligned; and Bollinger %B on a window of zero width is `NaN`, not `0`. numpy arrays and lists
are accepted too and give a result with a `RangeIndex`.

To offer it as an optional backend and keep pandas-ta as the fallback:

```python
try:
    from fin_primitives import ta
except ImportError:
    import pandas_ta as ta
```

## pandas-ta style: `df.fp` and Polars `.fp`

Code written for pandas-ta's `df.ta` moves over by importing one module and changing `ta`
to `fp`. Method names, defaults and output column names are pandas-ta's; the numbers come
from `fin_primitives.fast` and match pandas-ta-classic 0.9.0 (`tests/test_accessors.py`
compares every method with `df.ta`):

```python
import fin_primitives.pandas          # registers df.fp and series.fp

df.fp.rsi()                           # Series "RSI_14"
df.fp.ema(50, append=True)            # adds column "EMA_50"
df.fp.macd()                          # MACD_12_26_9, MACDh_12_26_9, MACDs_12_26_9
df.fp.bbands(length=20, std=2)        # BBL_20_2.0, BBM_20_2.0, BBU_20_2.0, BBB_20_2.0, BBP_20_2.0
df.fp.atr()                           # ATRr_14 (high, low, close found in any case)
df["close"].fp.sma(20)                # on a Series
df.fp.compute("kama", 10)             # any of the 743 exact indicators by key
```

Methods (pandas-ta's defaults throughout):

| group | methods |
|---|---|
| overlap | `sma`, `ema`, `rma`, `wma`, `hma`, `dema`, `tema`, `t3`, `trima`, `zlma`, `vwma`, `vwap`, `midpoint`, `midprice`, `linreg`, `kama`, `hl2`, `hlc3`, `ohlc4`, `supertrend`, `ichimoku` |
| momentum | `rsi`, `macd`, `mom`, `eri`, `roc`, `cmo`, `stoch`, `stochrsi`, `willr`, `cci`, `ao`, `apo`, `ppo`, `trix`, `tsi`, `uo`, `bop`, `qqe` |
| trend | `adx`, `aroon`, `psar`, `vortex`, `chop`, `dpo` |
| volatility | `atr`, `natr`, `true_range`, `bbands`, `kc`, `donchian` |
| statistics | `stdev`, `variance`, `mad`, `zscore`, `log_return`, `percent_return` |
| volume | `obv`, `mfi`, `ad`, `adosc`, `cmf`, `efi`, `pvt` |

Each takes pandas-ta's parameters by name or position (`df.fp.stoch(14, 3, 3)`,
`df.fp.supertrend(length=10, multiplier=2.5)`) and returns pandas-ta's columns in
pandas-ta's order (`SUPERT_7_3.0`, `SUPERTd_7_3.0`, `SUPERTl_7_3.0`, `SUPERTs_7_3.0`).
`tests/test_accessors_more.py` checks every one against pandas-ta-classic 0.9.0 on two
fixed 5,000 and 6,000 bar series, at the defaults and at one other setting. Arguments that
select another formula (`talib=`, `mamode=`, `offset=`, `drift=`) raise `TypeError`
instead of being ignored. `ichimoku` returns the in-sample frame (pandas-ta's
`append_span=False`); `vwap` needs a sorted DatetimeIndex, as in pandas-ta.

Several at once, as pandas-ta's `df.ta.strategy` (3.7.0):

```python
from fin_primitives.pandas import Strategy, CommonStrategy

df.fp.strategy()                                  # every method above, appended to df
df.fp.strategy("momentum", exclude=["qqe"])       # one pandas-ta category
df.fp.strategy(CommonStrategy)                    # SMA_10, SMA_20, SMA_50, SMA_200, VOL_SMA_20
df.fp.strategy(Strategy(name="mine", ta=[
    {"kind": "rsi", "length": 7},
    {"kind": "macd", "fast": 8, "slow": 21},
    {"kind": "sma", "close": "volume", "length": 20, "prefix": "VOL"},
]))
```

Entries take pandas-ta's `params`, `prefix`, `suffix`, `delimiter` and `col_names`;
`df.fp.study` is the older name, `df.fp.categories` and `df.fp.indicators()` list what runs,
and `cores=` / `df.fp.cores` are accepted (the Rust kernels run serially). `vwap` is left out
without a DatetimeIndex and indicators needing a missing column (`volume`) are skipped, as in
pandas-ta. `tests/test_strategy.py` checks a custom Strategy, `CommonStrategy` and every
category against pandas-ta-classic 0.9.0's `df.ta.strategy`: the same columns in the same
order with the same values. `"all"` covers the 70 methods above, so it adds about a quarter
of the columns pandas-ta's `"all"` does (no candles, cycles or the less common indicators).

Polars gets an expression namespace for single-column indicators, so they work per group
with `.over()`, and a DataFrame namespace for the ones that need several columns or return
several:

```python
import polars as pl
import fin_primitives.polars          # registers pl.col(...).fp and df.fp

df.with_columns(
    pl.col("close").fp.rsi(14).over("symbol").alias("rsi"),
    pl.col("close").fp.ema(50).over("symbol").alias("ema50"),
)
df.fp.with_indicators(df.fp.atr(14), df.fp.macd(), df.fp.bbands(20, 2.0))
df.fp.supertrend()                    # every df.fp method above except vwap
```

Close-only, single-column indicators (`wma`, `hma`, `dema`, `tema`, `t3`, `trima`, `zlma`,
`midpoint`, `linreg`, `mom`, `roc`, `cmo`, `mad`, `zscore`, `log_return`,
`percent_return`, plus the earlier `sma`, `ema`, `rma`, `rsi`, `stdev`, `variance`) are
also expressions: `pl.col("close").fp.hma(21).over("symbol")`.

## The catalogue

```python
>>> len(fp.catalogue())
743
>>> [e for e in fp.catalogue() if e["key"] == "macd"][0]
{'key': 'macd', 'type': 'Macd',
 'params': [('fast_period', 'int'), ('slow_period', 'int'), ('signal_period', 'int')],
 'doc': 'MACD indicator returning the histogram: `(fast_ema - slow_ema) - signal_ema`.'}
```

Keys are the snake_case names of the Rust types in `fin_primitives::signals::indicators`
(`BollingerB` is `"bollinger_b"`). Module names are accepted as aliases (`"bollinger"`,
`"psar"`, `"stochastic"`, `"hullma"`, ...): see `fp.aliases()`. `fp.resolve(name)` gives the
key, and an unknown name raises `KeyError` with suggestions.

The catalogue is generated from the Rust sources (`python/tools/gen_registry.py`), so every
indicator in the crate is in it. Each value is the indicator's single scalar output: MACD
returns the histogram, Bollinger `"bollinger_b"` returns %B and `"bollinger_width"` the band
width.

## Prices, quantities, order book

```python
fp.Price("64250.50").decimal      # Decimal('64250.50'); zero or negative raises ValueError
fp.Quantity(0.8).decimal          # Decimal('0.8'); negative raises ValueError
book = fp.OrderBook("BTC-USD")
book.apply_delta("bid", "64250.00", "1.2", True, 1)
book.apply_delta("ask", "64250.50", "0.8", True, 2)
book.mid_price()                  # 64250.25; a delta that would cross the book raises
```

The 2.15 classes `PySMA`, `PyEMA`, `PyRSI`, `PyPrice`, `PyQuantity`, `PyOHLCV` and
`PyOrderBook` are still exported (from `fin_primitives` and `fin_primitives._fin_primitives`).

## Backtest validation (CPCV)

`fin_primitives.validation` (pure Python, needs numpy) has `CombinatorialPurgedKFold`, a
combinatorial purged cross-validation splitter with scikit-learn's `split` / `get_n_splits`
protocol, and `walk_forward_paths`, which stitches the out-of-sample results into
`C(n_groups - 1, n_test_groups - 1)` backtest paths. See
[python-validation.md](https://github.com/Mattbusel/fin-primitives/blob/main/docs/python-validation.md).

```python
from fin_primitives.validation import CombinatorialPurgedKFold, walk_forward_paths

cv = CombinatorialPurgedKFold(n_groups=6, n_test_groups=2, purge=2, embargo=1)  # 15 splits, 5 paths
res = walk_forward_paths(cv, returns, fit_predict, metric=sharpe)
res.metrics   # one Sharpe ratio per out-of-sample path
```

## Accuracy

`python/tests/test_reference.py` checks RSI, EMA, SMA, MACD, Bollinger %B and ATR against
values printed by the Rust crate itself (`cargo run --example indicator_reference`) on 120
seeded bars: exact mode matches digit for digit, float mode equals `float()` of each value.
Another test builds every catalogue entry and runs 80 bars through it in both modes.
`python/tests/test_fast.py` checks `fin_primitives.fast` against numpy references and,
when pandas-ta-classic is installed, against pandas-ta-classic itself on random walks and
on data with regime changes, jumps, flat runs and missing values: identical `NaN` placement
and values within 1e-12 relative.

## Building from source

```bash
cd python
pip install "maturin>=1.7,<2"
maturin build               # wheel in ../target/wheels, built with the release-python profile
pip install ../target/wheels/fin_primitives-*.whl
pip install pytest numpy pandas polars && pytest
```

`maturin build` uses the crate's `release-python` Cargo profile: the same optimisation as
`release`, but a Rust panic becomes a Python exception instead of aborting the interpreter.
Do not add `--release` (to `maturin build` or `maturin develop`): it selects the crate's
plain `release` profile, which has `panic = "abort"`, so a panic would kill the interpreter.

`python/LICENSE` is a copy of the repository's `LICENSE`, so that maturin ships it in the
wheel (`.dist-info/licenses/LICENSE`); CI checks the two are identical.

Rust 1.83 or newer. After adding or changing an indicator in Rust, regenerate the
catalogue with `python python/tools/gen_registry.py`.

## License

MIT
