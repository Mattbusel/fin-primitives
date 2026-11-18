# Depending on fin-primitives

What a project needs to know before it takes fin-primitives as a dependency: the small
surface to use, what it pulls in, what stays stable, and how to add it without forcing it
on every user.

Maintained by Matt Busel. MIT licensed (Rust crate and Python wheel). Bug reports go to
[GitHub issues](https://github.com/Mattbusel/fin-primitives/issues).

## Python: replacing pandas-ta calls

`fin_primitives.ta` has pandas-ta's function API: the same function names, the same input
order, the same parameter names and defaults, the same column names. Computed by Rust
kernels, with values that the test suite checks against pandas-ta-classic 0.9.0.

```python
from fin_primitives import ta          # was: import pandas_ta as ta

df["RSI_14"] = ta.rsi(df["close"], length=14)
macd = ta.macd(df["close"])            # MACD_12_26_9, MACDh_12_26_9, MACDs_12_26_9
bands = ta.bbands(df["close"], length=20, std=2)
atr = ta.atr(df["high"], df["low"], df["close"], length=14)
```

68 functions: `sma ema rma wma hma dema tema t3 trima zlma vwma vwap midpoint midprice linreg
kama hl2 hlc3 ohlc4 supertrend ichimoku rsi macd mom eri kdj fisher bias slope roc cmo stoch
stochrsi willr cci ao apo ppo trix tsi uo bop qqe adx aroon psar vortex chop dpo atr natr
true_range bbands kc donchian stdev variance mad zscore log_return percent_return obv mfi ad
adosc cmf efi pvt`.

Every function is typed (the package ships `py.typed`), so mypy and pyright check the
arguments. Where it differs from pandas-ta:

| pandas-ta | `fin_primitives.ta` |
|---|---|
| input shorter than the period returns `None` | returns a result of `NaN` |
| `talib=`, `mamode=`, `offset=`, `drift=`, `fillna=` change or post-process the result | raise `TypeError` (only the default formula is computed; `bias` takes `mamode` `"sma"`, `"ema"`, `"rma"` or `"wma"`) |
| input series with different indexes are aligned | raise `ValueError`; take them from one DataFrame |
| `ichimoku(..., append_span=True)` appends the forward span rows | raises `TypeError`; the in-sample frame is returned, as pandas-ta-classic 0.9.0 does by default |
| Bollinger %B on a window with zero width is `0` | `NaN` (the deviation is exactly zero) |

The DataFrame accessor (`import fin_primitives.pandas`, then `df.fp.rsi()`) and the
Polars namespace (`import fin_primitives.polars`, then `pl.col("close").fp.rsi(14)`) give
the same numbers. See [python.md](python.md).

### As an optional backend

A project that already depends on pandas-ta can offer fin-primitives as an extra and keep
pandas-ta as the fallback, so nothing changes for users who do not install it:

```toml
# pyproject.toml
[project.optional-dependencies]
fast-ta = ["fin-primitives>=3.9"]
```

```python
try:
    from fin_primitives import ta
except ImportError:  # pip install "yourproject[fast-ta]" for the Rust kernels
    import pandas_ta as ta
```

### What it installs

One abi3 wheel per platform (CPython 3.9 and later): Linux x86_64 and aarch64 (manylinux
and musllinux), macOS x86_64 and arm64, Windows x86_64, plus a Pyodide wheel. No runtime
dependencies; `fin_primitives.ta` and `df.fp` need pandas and numpy, which a pandas-ta
user already has (`pip install "fin-primitives[pandas]"`).

## Rust: indicators only

The default features build every domain module. A project that wants the indicators turns
the rest off:

```toml
[dependencies]
fin-primitives = { version = "3.9", default-features = false, features = ["indicators"] }
rust_decimal = "1"
```

```rust
use fin_primitives::signals::{indicators::Rsi, BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

fn last_rsi(closes: &[Decimal]) -> Result<Option<Decimal>, fin_primitives::FinError> {
    let mut rsi = Rsi::new("rsi14", 14)?;
    let mut last = None;
    for &close in closes {
        if let SignalValue::Scalar(v) = rsi.update(&BarInput::from_close(close))? {
            last = Some(v);
        }
    }
    Ok(last)
}
```

That build depends on `rust_decimal`, `chrono` (`std` and `clock` only), `thiserror` and
`libm`: four direct dependencies and 12 crates in all on Linux, counting the proc-macro
crates `thiserror` builds with. The crate is `#![forbid(unsafe_code)]`, has no build
script, and builds for `wasm32-unknown-unknown`.

| feature | default | adds |
|---|---|---|
| `serde` | yes | `Serialize`/`Deserialize` on the data types |
| `indicators` | yes | `signals::indicators` (700+ streaming indicators), `technical`, `regime`, `correlation`, `volatility` |
| `options` | yes | Black-Scholes, Greeks, implied volatility, option chains |
| `portfolio`, `risk`, `backtest` | yes | ledger, P&L, risk limits, backtester |
| `fixed-income`, `derivatives`, `crypto`, `credit`, `ml`, `tax`, `yield-curve`, `clustering`, `montecarlo` | yes | the other domain modules |
| `async` | no | `async_signals` on Tokio |
| `ta-compat` | no | the `ta` crate's API over these indicators |
| `ta`, `yata`, `wickra`, `arrow` | no | interop with those crates (`wickra` needs Rust 1.86, `arrow` 1.88) |
| `python` | no | the PyO3 bindings behind the Python wheel |

## Stability

* Semantic versioning. Breaking changes to public items come only in a major release, and
  the CHANGELOG lists every change.
* Minimum supported Rust version: 1.81, checked in CI with `--no-default-features` and with
  the default features. Raising it is a minor release and is noted in the CHANGELOG.
* The Python package and the crate share one version number. `fin_primitives.ta`,
  `fin_primitives.fast`, `df.fp`, the Polars `.fp` namespaces, `Indicator` and
  `catalogue()` are the public Python API; modules starting with `_` are not.
* CI runs `cargo clippy -D warnings`, the Rust tests, the MSRV build, and the Python test
  suite (including the comparison with pandas-ta-classic) on every push and pull request.
* The typed surface is checked: CI runs `mypy --strict` on a file that calls
  `fin_primitives.ta` (`python/tests/typing/ta_usage.py`) against the installed wheel.

## Maintenance and provenance

What a reviewer can check before taking the dependency:

* Release history: 28 crates.io releases since 0.1.0 on 2026-03-07, and the PyPI package
  since 3.3.0 on 2026-11-15, every one listed in the [CHANGELOG](../CHANGELOG.md). Each
  version has a `v<version>` tag (Rust) and a `py-v<version>` tag (Python) on GitHub.
* Builds: the PyPI wheels and sdist are built and uploaded by
  [`python-wheels.yml`](../.github/workflows/python-wheels.yml) with PyPI trusted publishing
  (no stored token), and carry PyPI provenance attestations that link each file to the
  workflow run that built it.
* Reference data for the tests: pandas-ta-classic 0.9.0 for `fin_primitives.ta`, `df.fp`
  and the Polars namespaces (`python/tests/test_ta.py`, `test_accessors.py`,
  `test_accessors_more.py`, `test_strategy.py`), and TA-Lib through the `ta-lib` crate
  for the Rust indicators in `tests/talib_parity.rs`. Both run in CI.
* Security reports: see [SECURITY.md](../SECURITY.md).
* Removing it: behind the optional extra above, removal is deleting the extra and the
  `try`/`except` import; the fallback path is the code the project already had.
