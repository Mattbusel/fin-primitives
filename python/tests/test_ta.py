"""``fin_primitives.ta`` (3.8.0; kdj, fisher, bias, slope and ichimoku in 3.9.0), pandas-ta's function API: the same values and column
names as ``df.fp``, signatures that line up with pandas-ta-classic 0.9.0's functions, and
(when pandas-ta-classic is installed) the same results as calling those functions."""
import inspect
import pathlib
import subprocess
import sys

import numpy as np
import pytest

pd = pytest.importorskip("pandas")
from test_accessors_more import VOLUME_BASED, assert_close, make_frames  # noqa: E402

from fin_primitives import _pta, ta  # noqa: E402

FRAMES = make_frames()
DF = FRAMES["real_like"]
COLUMNS = {"open_": "open", "high": "high", "low": "low", "close": "close", "volume": "volume"}
SKIP_DEFAULT_VWAP = {"vwap"}  # needs a DatetimeIndex; tested on its own below


def inputs_of(fn):
    """The input series: required, or psar's optional close (adosc's and cmf's ``open_`` is a
    pandas-ta parameter accepted only as None)."""
    sig = inspect.signature(fn).parameters
    return [p for p, v in sig.items() if p in COLUMNS and (v.default is inspect.Parameter.empty or fn.__name__ == "psar")]


def params_of(fn, kind=None):
    inputs = inputs_of(fn)
    return [
        p.name
        for p in inspect.signature(fn).parameters.values()
        if p.name not in inputs and p.kind is not inspect.Parameter.VAR_KEYWORD and (kind is None or p.kind is kind)
    ]


def call(fn, df, **kw):
    return fn(*[df[COLUMNS[c]] for c in inputs_of(fn)], **kw)


def assert_equal(got, want):
    if isinstance(want, pd.DataFrame):
        pd.testing.assert_frame_equal(got, want, check_exact=True)
    else:
        pd.testing.assert_series_equal(got, want, check_exact=True)


@pytest.mark.parametrize("name", [n for n in ta.__all__ if n not in SKIP_DEFAULT_VWAP])
def test_matches_df_fp_exactly(name):
    fn = getattr(ta, name)
    for df in FRAMES.values():
        got = call(fn, df)
        want = getattr(df.fp, name)()
        assert_equal(got, want)


def test_covers_every_df_fp_indicator():
    assert set(ta.__all__) == set(_pta.SPEC) | {
        "sma", "ema", "rma", "rsi", "atr", "true_range", "macd", "bbands", "stdev", "variance", "vwap",
    }


@pytest.mark.parametrize("name", sorted(_pta.SPEC.keys()))
def test_signature_follows_pta(name):
    fn = getattr(ta, name)
    ref = inspect.signature(_pta.SPEC[name][1])
    params = [p for p in params_of(fn) if p not in ("open_", "ln")]
    assert inputs_of(fn) + params == list(ref.parameters)
    for p in params:
        assert inspect.signature(fn).parameters[p].default == ref.parameters[p].default


def test_parameters_by_position_and_name():
    a = ta.stoch(DF["high"], DF["low"], DF["close"], 5, 3, 1)
    b = ta.stoch(DF["high"], DF["low"], DF["close"], k=5, d=3, smooth_k=1)
    assert list(a.columns) == ["STOCHk_5_3_1", "STOCHd_5_3_1"]
    assert_equal(a, b)
    assert_equal(a, DF.fp.stoch(5, 3, 1))
    assert ta.macd(DF["close"], 8, 21, 5).columns[0] == "MACD_8_21_5"
    assert ta.rsi(DF["close"], 7).name == "RSI_7"
    assert ta.bbands(DF["close"], 20, 2.5).columns[0] == "BBL_20_2.5"


def test_numpy_and_list_input():
    close = DF["close"].to_numpy()
    got = ta.rsi(close, 14)
    assert isinstance(got, pd.Series) and isinstance(got.index, pd.RangeIndex)
    np.testing.assert_array_equal(got.to_numpy(), ta.rsi(DF["close"], 14).to_numpy())
    np.testing.assert_array_equal(ta.sma(list(close[:50]), 5).to_numpy(), ta.sma(close[:50], 5).to_numpy())


def test_index_is_kept():
    df = DF.iloc[:300].copy()
    df.index = pd.date_range("2024-01-01", periods=len(df), freq="h")
    out = ta.atr(df["high"], df["low"], df["close"])
    assert out.index.equals(df.index) and out.name == "ATRr_14"


def test_different_indexes_raise():
    high = DF["high"].iloc[1:]
    with pytest.raises(ValueError, match="different indexes"):
        ta.atr(high, DF["low"].iloc[:-1], DF["close"].iloc[:-1])
    shifted = DF["low"].copy()
    shifted.index = shifted.index + 1
    with pytest.raises(ValueError, match="different indexes"):
        ta.willr(DF["high"], shifted, DF["close"])


@pytest.mark.parametrize("kw", [{"talib": False}, {"offset": 1}, {"mamode": "ema"}, {"fillna": 0}, {"high": 1}])
def test_unsupported_arguments_raise(kw):
    with pytest.raises(TypeError):
        ta.rsi(DF["close"], 14, **kw)


def test_dataframe_instead_of_series_raises():
    with pytest.raises(TypeError, match="DataFrame"):
        ta.sma(DF[["close"]], 10)


def test_pandas_ta_positional_parameters_that_change_the_formula():
    h, l, c, v = DF["high"], DF["low"], DF["close"], DF["volume"]
    assert_equal(ta.cmf(h, l, c, v, None, 10), DF.fp.cmf(10))
    assert_equal(ta.adosc(h, l, c, v, None, 5, 20), DF.fp.adosc(5, 20))
    assert_equal(ta.chop(h, l, c, 20, 10, False), DF.fp.chop(20, 10))
    with pytest.raises(TypeError, match="open_"):
        ta.cmf(h, l, c, v, DF["open"])
    with pytest.raises(TypeError, match="ln"):
        ta.chop(h, l, c, ln=True)
    with pytest.raises(TypeError):
        ta.kc(h, l, c, 20, 2.0, False)  # pandas-ta's third parameter is mamode; tr is by name
    assert_equal(ta.kc(h, l, c, 20, 2.0, tr=False), DF.fp.kc(20, 2.0, tr=False))
    assert_equal(ta.zscore(c, 20, ddof=1), DF.fp.zscore(20, ddof=1))


def test_short_input_is_nan_not_none():
    out = ta.rsi(DF["close"].iloc[:5], 14)
    assert len(out) == 5 and out.isna().all()


def test_vwap_needs_datetime_index():
    df = DF.iloc[:500].copy()
    df.index = pd.date_range("2024-01-01", periods=len(df), freq="h")
    got = ta.vwap(df["high"], df["low"], df["close"], df["volume"])
    assert_equal(got, df.fp.vwap())
    with pytest.raises(TypeError, match="DatetimeIndex"):
        ta.vwap(DF["high"], DF["low"], DF["close"], DF["volume"])


def test_psar_without_close():
    assert_equal(ta.psar(DF["high"], DF["low"]), DF[["high", "low"]].fp.psar())


def test_generated_file_is_current():
    tool = pathlib.Path(__file__).resolve().parents[1] / "tools" / "gen_ta.py"
    if not tool.exists() or sys.platform == "emscripten":
        pytest.skip("needs the source checkout and subprocesses")
    assert subprocess.run([sys.executable, str(tool), "--check"]).returncode == 0


# ---------------------------------------------------------------------------
# against pandas-ta-classic's own functions
# ---------------------------------------------------------------------------


@pytest.fixture(scope="module")
def pta():
    if not hasattr(np, "NaN"):
        np.NaN = np.nan
    return pytest.importorskip("pandas_ta_classic")


@pytest.mark.parametrize("name", [n for n in ta.__all__ if n not in SKIP_DEFAULT_VWAP])
def test_signature_is_a_prefix_of_pandas_ta(pta, name):
    fn = getattr(ta, name)
    ours = inputs_of(fn) + params_of(fn, inspect.Parameter.POSITIONAL_OR_KEYWORD)
    theirs = [p for p in inspect.signature(getattr(pta, name)).parameters if p != "kwargs"]
    assert theirs[: len(ours)] == ours
    for p in params_of(fn, inspect.Parameter.KEYWORD_ONLY):
        assert p in theirs or p == "tr"  # kc reads tr from **kwargs


@pytest.mark.parametrize("name", [n for n in ta.__all__ if n not in SKIP_DEFAULT_VWAP])
def test_same_result_as_pandas_ta_function(pta, name):
    fn = getattr(ta, name)
    for df in FRAMES.values():
        args = [df[COLUMNS[c]] for c in inputs_of(fn)]
        extra = {"talib": False} if "talib" in inspect.signature(getattr(pta, name)).parameters else {}
        want = getattr(pta, name)(*args, **extra)
        got = fn(*args)
        if isinstance(want, pd.DataFrame):
            assert list(got.columns) == list(want.columns)
        else:
            assert got.name == want.name
        price = np.maximum(np.abs(df["close"].to_numpy()), 100.0)
        scale = df["volume"].cumsum().to_numpy() * price if name in VOLUME_BASED else price
        if name == "pvt":
            scale = df["volume"].cumsum().to_numpy() * 100.0
        g = got if isinstance(got, pd.DataFrame) else got.to_frame()
        w = want if isinstance(want, pd.DataFrame) else want.to_frame()
        if name == "bbands":
            # A flat window: %B is 0/0 here (NaN) and 0 in pandas-ta (see test_accessors.py).
            flat = (g.iloc[:, 2] - g.iloc[:, 0]).to_numpy() == 0
            assert g.iloc[:, 4][flat].isna().all()
            g, w, scale = g[~flat], w[~flat], scale[~flat]
        for c in w.columns:
            assert_close(g[c], w[c], scale)


def test_new_in_3_9_by_position_match_pandas_ta_classic():
    pta = pytest.importorskip("pandas_ta_classic")
    h, l, c = DF["high"], DF["low"], DF["close"]
    pairs = [
        (ta.kdj(h, l, c, 14, 5), pta.kdj(h, l, c, 14, 5)),
        (ta.fisher(h, l, 20, 3), pta.fisher(h, l, 20, 3)),
        (ta.bias(c, 10, "ema"), pta.bias(c, 10, "ema")),
        (ta.slope(c, 3, True, True), pta.slope(c, 3, True, True)),
        (ta.ichimoku(h, l, c, 7, 22, 44, False), pta.ichimoku(h, l, c, 7, 22, 44, False)),
    ]
    for got, want in pairs:
        g = got if isinstance(got, pd.DataFrame) else got.to_frame()
        w = want if isinstance(want, pd.DataFrame) else want.to_frame()
        assert list(g.columns) == list(w.columns)
        for col in w:
            assert_close(g[col], w[col], np.maximum(np.abs(w[col].to_numpy(float)), 1.0))


def test_bias_rejects_an_unknown_mamode_and_ichimoku_append_span():
    with pytest.raises(ValueError, match="mamode"):
        ta.bias(DF["close"], mamode="jma")
    with pytest.raises(TypeError):
        ta.ichimoku(DF["high"], DF["low"], DF["close"], append_span=True)
