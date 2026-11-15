# fin-primitives for Python

Trading indicators computed in exact decimal arithmetic by the
[fin-primitives](https://github.com/Mattbusel/fin-primitives) Rust crate. All 743 of them
(RSI, EMA, SMA, MACD, Bollinger, ATR, ADX, VWAP, Ichimoku, SuperTrend and the rest) through
one class, with streaming updates and batch helpers for lists, numpy, pandas and polars.

```bash
pip install fin-primitives
```

One abi3 wheel per platform covers CPython 3.9 and every later version: Linux x86_64 and
aarch64, macOS x86_64 and arm64, Windows x86_64. No runtime dependencies. numpy, pandas and
polars are used only if you pass their types in.

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
