"""numpy arrays, pandas Series and polars Series in, the same type out."""
import json
import math
import pathlib
from decimal import Decimal

import pytest

import fin_primitives as fp

DATA = json.loads((pathlib.Path(__file__).parent / "data" / "reference.json").read_text())
CLOSE = [float(c) for c in DATA["bars"]["close"]]
HIGH = [float(c) for c in DATA["bars"]["high"]]
LOW = [float(c) for c in DATA["bars"]["low"]]
RSI = [None if v is None else Decimal(v) for v in DATA["cases"]["rsi_14"]["values"]]
ATR = [None if v is None else Decimal(v) for v in DATA["cases"]["atr_14"]["values"]]


def as_float(values):
    return [math.nan if v is None else float(v) for v in values]


def same_floats(got, want):
    assert len(got) == len(want)
    for g, w in zip(got, want):
        assert (math.isnan(g) and math.isnan(w)) or g == w


def test_numpy_float_mode():
    np = pytest.importorskip("numpy")
    close = np.array(CLOSE)
    out = fp.rsi(close, 14)
    assert isinstance(out, np.ndarray) and out.dtype == np.float64 and out.shape == close.shape
    same_floats(out.tolist(), as_float(RSI))
    # Non-contiguous and integer input work too.
    out2 = fp.sma(np.arange(10, 30)[::2], 3)
    assert out2.dtype == np.float64 and out2[2] == 12.0


def test_numpy_exact_mode():
    np = pytest.importorskip("numpy")
    out = fp.Indicator("rsi", 14).batch(np.array(CLOSE))
    assert isinstance(out, np.ndarray) and out.dtype == object
    assert out.tolist() == RSI


def test_numpy_nan_input_is_rejected():
    np = pytest.importorskip("numpy")
    with pytest.raises(ValueError, match=r"close\[1\]"):
        fp.sma(np.array([1.0, np.nan, 3.0]), 2)


def test_pandas_float_mode_keeps_index():
    pd = pytest.importorskip("pandas")
    idx = pd.date_range("2026-01-01", periods=len(CLOSE), freq="min")
    df = pd.DataFrame({"close": CLOSE, "high": HIGH, "low": LOW}, index=idx)
    out = fp.atr(df["high"], df["low"], df["close"], 14)
    assert isinstance(out, pd.Series) and out.dtype == "float64"
    assert out.index.equals(df.index) and out.name == "atr_14"
    same_floats(out.tolist(), as_float(ATR))


def test_pandas_exact_mode():
    pd = pytest.importorskip("pandas")
    s = pd.Series([Decimal(c) for c in DATA["bars"]["close"]], dtype=object)
    out = fp.rsi(s, 14)  # object dtype of Decimal -> auto picks exact mode
    assert isinstance(out, pd.Series) and out.dtype == object
    assert out.tolist() == RSI


def test_polars_float_mode():
    pl = pytest.importorskip("polars")
    s = pl.Series("close", CLOSE)
    out = fp.rsi(s, 14)
    assert isinstance(out, pl.Series) and out.dtype == pl.Float64 and out.name == "rsi_14"
    # Warm-up values are nulls in polars.
    assert out.null_count() == 14
    got = [math.nan if v is None else v for v in out.to_list()]
    same_floats(got, as_float(RSI))


def test_polars_exact_mode():
    pl = pytest.importorskip("polars")
    out = fp.Indicator("sma", 20).batch(pl.Series("close", DATA["bars"]["close"]))
    assert isinstance(out, pl.Series)
    want = [None if v is None else Decimal(v) for v in DATA["cases"]["sma_20"]["values"]]
    assert [None if v is None else Decimal(v) for v in out.to_list()] == want


def test_plain_sequences():
    assert isinstance(fp.sma(tuple(CLOSE), 3), tuple)
    out = fp.Indicator("sma", 3, mode="float").batch(iter(CLOSE))
    assert isinstance(out, list) and out[:2] == [None, None]


def test_length_mismatch_is_rejected():
    with pytest.raises(ValueError, match="high has"):
        fp.Indicator("atr", 3).batch(CLOSE, high=HIGH[:-1], low=LOW)
    with pytest.raises(ValueError, match="high has"):
        fp.Indicator("atr", 3, mode="float").batch(CLOSE, high=HIGH[:-1], low=LOW)


def test_polars_exact_mode_never_drops_values():
    """polars infers one Decimal(38, scale) for a column; values that do not fit it must not
    become null. Here a 28-decimal-place value comes before 11-digit ones."""
    pl = pytest.importorskip("polars")
    closes = ["1", "1", "2", "90000000000", "90000000000", "90000000001"]
    want = fp.Indicator("sma", 3).batch(closes)
    assert want[2] == Decimal("1.3333333333333333333333333333")
    assert all(v is not None for v in want[2:])
    out = fp.Indicator("sma", 3).batch(pl.Series("c", closes))
    assert isinstance(out, pl.Series) and len(out) == len(closes)
    assert out.null_count() == 2
    assert out.to_list() == want
    # When everything fits, the result stays a Decimal column.
    small = fp.Indicator("sma", 3).batch(pl.Series("c", ["1", "2", "4", "8"]))
    assert str(small.dtype).startswith("Decimal")
    assert small.to_list() == fp.Indicator("sma", 3).batch(["1", "2", "4", "8"])
    assert small[2] == Decimal("2.3333333333333333333333333333")


def test_polars_exact_mode_mixed_scales_from_compute():
    pl = pytest.importorskip("polars")
    closes = pl.Series("c", ["0.0000000001", "0.0000000003", "12345678901234567", "12345678901234569"])
    out = fp.compute("sma", closes, 2, mode="exact")
    assert out.to_list() == fp.compute("sma", closes.to_list(), 2)
    assert out.null_count() == 1


def test_numpy_2d_input_is_rejected():
    np = pytest.importorskip("numpy")
    with pytest.raises(ValueError, match="1-D"):
        fp.sma(np.ones((3, 2)), 2)
    with pytest.raises(ValueError, match="1-D"):
        fp.sma(np.ones((3, 2)), 2, mode="exact")
    with pytest.raises(ValueError, match="high"):
        fp.atr(np.ones((4, 2)), np.ones(4), np.ones(4), 2)
    with pytest.raises(ValueError, match="1-D"):
        fp.sma(np.array(5.0), 2)


def test_pandas_columns_must_share_an_index():
    pd = pytest.importorskip("pandas")
    close = pd.Series(CLOSE[:20], index=range(20))
    high = pd.Series(HIGH[:20], index=range(100, 120))
    low = pd.Series(LOW[:20], index=range(20))
    for mode in ("float", "exact"):
        with pytest.raises(ValueError, match="different pandas indexes"):
            fp.atr(high, low, close, 5, mode=mode)
    # Same index: fine, and the result keeps it.
    out = fp.atr(high.set_axis(close.index), low, close, 5)
    assert out.index.equals(close.index)
    # A numpy column next to a pandas close is paired by position, as documented.
    np = pytest.importorskip("numpy")
    out2 = fp.atr(np.array(HIGH[:20]), low, close, 5)
    assert out2.index.equals(close.index)
