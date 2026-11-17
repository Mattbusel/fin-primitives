"""``df.fp.strategy`` (3.7.0) against pandas-ta-classic 0.9.0's ``df.ta.strategy``: the same
columns, in the same order, with the same values, for a custom Strategy, CommonStrategy and
every category; ``"all"`` adds only columns pandas-ta's ``"all"`` adds, with its values."""
import numpy as np
import pytest

pd = pytest.importorskip("pandas")
from fin_primitives.pandas import CATEGORIES, AllStrategy, CommonStrategy, Strategy  # noqa: E402


def frame(n=800, hourly=True):
    rng = np.random.default_rng(7)
    close = 100 + np.cumsum(rng.normal(0, 1, n))
    high = close + rng.uniform(0, 2, n)
    low = close - rng.uniform(0, 2, n)
    open_ = np.clip(close + rng.normal(0, 0.5, n), low, high)
    volume = np.round(rng.uniform(1e3, 5e3, n))
    index = pd.date_range("2024-01-01", periods=n, freq="h") if hourly else pd.RangeIndex(n)
    return pd.DataFrame({"open": open_, "high": high, "low": low, "close": close, "volume": volume}, index=index)


@pytest.fixture(scope="module")
def ta():
    if not hasattr(np, "NaN"):
        np.NaN = np.nan
    return pytest.importorskip("pandas_ta_classic")


def added(df, base):
    return [c for c in df.columns if c not in base.columns]


def same_values(a, b, cols):
    for c in cols:
        x, y = a[c].to_numpy(float), b[c].to_numpy(float)
        assert np.array_equal(np.isnan(x), np.isnan(y)), c
        ok = ~np.isnan(x)
        scale = np.nanmax(np.abs(a["volume"].to_numpy(float)))
        assert np.allclose(x[ok], y[ok], rtol=1e-9, atol=1e-9 * scale), c


CUSTOM = [
    {"kind": "rsi"},
    {"kind": "macd", "fast": 8, "slow": 21},
    {"kind": "bbands", "length": 20, "col_names": ("L", "M", "U", "B", "P")},
    {"kind": "ema", "params": (50,), "suffix": "X"},
    {"kind": "sma", "close": "volume", "length": 20, "prefix": "VOLUME"},
    {"kind": "supertrend", "length": 10, "multiplier": 3.0},
]


@pytest.mark.parametrize("make", [lambda ta: (ta.Strategy(name="c", ta=CUSTOM), Strategy(name="c", ta=CUSTOM)), lambda ta: (ta.CommonStrategy, CommonStrategy)])
def test_custom_strategies_match_pandas_ta(ta, make):
    theirs, ours = make(ta)
    base = frame()
    a, b = base.copy(), base.copy()
    a.ta.strategy(theirs)
    b.fp.strategy(ours)
    assert list(b.columns) == list(a.columns)
    same_values(a, b, added(b, base))


@pytest.mark.parametrize("category", sorted(CATEGORIES))
def test_categories_add_pandas_ta_columns(ta, category):
    base = frame()
    a, b = base.copy(), base.copy()
    a.ta.strategy(category)
    b.fp.strategy(category)
    new = added(b, base)
    assert new and set(new) <= set(added(a, base))
    same_values(a, b, new)


def test_all(ta):
    base = frame()
    a, b = base.copy(), base.copy()
    a.ta.strategy("all")
    assert b.fp.strategy(AllStrategy, returns=True) is b
    new = added(b, base)
    assert len(new) > 100 and set(new) <= set(added(a, base))
    same_values(a, b, new)


def test_rules():
    base = frame(hourly=False)
    df = base.copy()
    df.fp.strategy("overlap")
    assert "VWAP_D" not in df.columns  # vwap needs a DatetimeIndex, as in pandas-ta
    no_volume = base.drop(columns="volume")
    no_volume.fp.strategy("volume")
    assert list(no_volume.columns) == list(base.columns[:-1])  # every volume indicator skipped
    df = base.copy()
    df.fp.strategy("momentum", exclude=["qqe", "macd"])
    assert not any(c.startswith(("QQE", "MACD")) for c in df.columns)
    df.fp.cores = 4  # accepted, as df.ta.cores
    df.fp.study(Strategy(name="s", ta=[{"kind": "rsi", "length": 7}]), cores=0)
    assert "RSI_7" in df.columns
    with pytest.raises(ValueError, match="neither 'all' nor a category"):
        df.fp.strategy("CommonStrategy")
    with pytest.raises(ValueError, match="unknown indicator"):
        df.fp.strategy("all", exclude=["nope"])
    with pytest.raises(ValueError, match="does not apply to the custom Strategy"):
        df.fp.strategy(CommonStrategy, exclude=["sma"])
    with pytest.raises(ValueError, match="not an indicator"):
        df.fp.strategy(Strategy(name="bad", ta=[{"kind": "nope"}]))
    with pytest.raises(TypeError, match="params must be a tuple"):
        df.fp.strategy(Strategy(name="bad", ta=[{"kind": "sma", "params": [5]}]))
    with pytest.raises(TypeError):
        Strategy(name="bad", ta=[{"length": 5}])
    assert df.fp.categories == sorted(CATEGORIES)
    assert "rsi" in df.fp.indicators() and "rsi" not in df.fp.indicators(exclude=["rsi"])
