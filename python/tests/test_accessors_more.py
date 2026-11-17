"""The 3.6.0 ``df.fp`` / Polars ``.fp`` indicators against pandas-ta-classic 0.9.0's ``df.ta``:
same method names, defaults, column names and column order, and the same values (within
1e-12 of the value, or of the price or volume level for oscillators that cross zero)."""
import numpy as np
import pytest

from test_fast import REL, real_like, walk

pd = pytest.importorskip("pandas")
import fin_primitives.pandas  # noqa: E402,F401  registers df.fp
from fin_primitives import _pta  # noqa: E402


def make_frames():
    out = {}
    for name, (h, l, c) in {"walk": walk(5000), "real_like": real_like()}.items():
        rng = np.random.default_rng(3)
        o = np.r_[c[0], c[:-1]] * (1 + rng.normal(0, 0.001, len(c)))
        o = np.clip(o, l, h)
        v = np.round(rng.lognormal(8, 1, len(c)))
        out[name] = pd.DataFrame({"open": o, "high": h, "low": l, "close": c, "volume": v})
    return out


FRAMES = make_frames()


@pytest.fixture(scope="module")
def ta():
    if not hasattr(np, "NaN"):
        np.NaN = np.nan
    return pytest.importorskip("pandas_ta_classic")


VOLUME_BASED = {"obv", "ad", "adosc", "efi", "pvt"}

CASES = [
    ("hl2", {}), ("hlc3", {}), ("ohlc4", {}),
    ("wma", {}), ("wma", {"length": 30}),
    ("hma", {}), ("hma", {"length": 21}),
    ("dema", {}), ("dema", {"length": 21}),
    ("tema", {}), ("tema", {"length": 21}),
    ("t3", {}), ("t3", {"length": 5, "a": 0.8}),
    ("trima", {}), ("trima", {"length": 15}),
    ("zlma", {}), ("zlma", {"length": 20}),
    ("vwma", {}), ("vwma", {"length": 20}),
    ("midpoint", {}), ("midpoint", {"length": 14}),
    ("midprice", {}), ("midprice", {"length": 14}),
    ("linreg", {}), ("linreg", {"length": 20}),
    ("supertrend", {}), ("supertrend", {"length": 10, "multiplier": 2.5}),
    ("ichimoku", {}), ("ichimoku", {"tenkan": 7, "kijun": 22, "senkou": 44}),
    ("mom", {}), ("mom", {"length": 5}),
    ("roc", {}), ("roc", {"length": 5}),
    ("cmo", {}), ("cmo", {"length": 9}),
    ("stoch", {}), ("stoch", {"k": 5, "d": 3, "smooth_k": 1}),
    ("stochrsi", {}), ("stochrsi", {"length": 10, "rsi_length": 7, "k": 2, "d": 4}),
    ("willr", {}), ("willr", {"length": 21}),
    ("cci", {}), ("cci", {"length": 20}),
    ("ao", {}), ("ao", {"fast": 3, "slow": 20}),
    ("apo", {}), ("apo", {"fast": 5, "slow": 15}),
    ("ppo", {}), ("ppo", {"fast": 5, "slow": 15, "signal": 4}),
    ("trix", {}), ("trix", {"length": 15, "signal": 5}),
    ("tsi", {}), ("tsi", {"fast": 7, "slow": 20, "signal": 5}),
    ("uo", {}), ("uo", {"fast": 5, "medium": 10, "slow": 20}),
    ("bop", {}),
    ("qqe", {}), ("qqe", {"length": 10, "smooth": 3, "factor": 3.0}),
    ("adx", {}), ("adx", {"length": 20, "lensig": 10}),
    ("aroon", {}), ("aroon", {"length": 25}),
    ("psar", {}), ("psar", {"af0": 0.01, "af": 0.01, "max_af": 0.1}),
    ("vortex", {}), ("vortex", {"length": 21}),
    ("chop", {}), ("chop", {"length": 20}),
    ("dpo", {}), ("dpo", {"length": 14}),
    ("natr", {}), ("natr", {"length": 21}),
    ("kc", {}), ("kc", {"length": 10, "scalar": 1.5}),
    ("donchian", {}), ("donchian", {"lower_length": 10, "upper_length": 30}),
    ("mad", {}), ("mad", {"length": 10}),
    ("zscore", {}), ("zscore", {"length": 20}),
    ("log_return", {}), ("log_return", {"length": 5}),
    ("percent_return", {}), ("percent_return", {"length": 5}),
    ("obv", {}),
    ("mfi", {}), ("mfi", {"length": 10}),
    ("ad", {}),
    ("adosc", {}), ("adosc", {"fast": 5, "slow": 20}),
    ("cmf", {}), ("cmf", {"length": 10}),
    ("efi", {}), ("efi", {"length": 20}),
    ("pvt", {}),
]


def test_every_new_indicator_has_a_case():
    assert {m for m, _ in CASES} == set(_pta.SPEC)
    assert len(_pta.SPEC) >= 50


def _columns(r):
    return list(r.columns) if isinstance(r, pd.DataFrame) else [r.name]


def assert_close(got, want, scale):
    got = np.asarray(got, dtype=float)
    want = np.asarray(want, dtype=float)
    assert got.shape == want.shape
    np.testing.assert_array_equal(np.isnan(got), np.isnan(want), err_msg="NaN positions differ")
    m = ~np.isnan(want)
    if not m.any():
        return
    np.testing.assert_array_equal(np.isinf(got[m]), np.isinf(want[m]))
    m &= np.isfinite(want)
    tol = REL * np.maximum(np.abs(want[m]), scale[m])
    diff = np.abs(got[m] - want[m])
    worst = int(np.argmax(diff - tol))
    assert np.all(diff <= tol), f"index {np.flatnonzero(m)[worst]}: {got[m][worst]!r} vs {want[m][worst]!r}"


@pytest.mark.parametrize("method,kw", CASES, ids=[f"{m}-{i}" for i, (m, _) in enumerate(CASES)])
def test_matches_pandas_ta(ta, method, kw):
    for name, df in FRAMES.items():
        want = getattr(df.ta, method)(talib=False, **kw)
        got = getattr(df.fp, method)(**kw)
        assert _columns(got) == _columns(want), name
        assert isinstance(got, pd.DataFrame) == isinstance(want, pd.DataFrame)
        price = np.maximum(np.abs(df["close"].to_numpy()), 100.0)
        scale = df["volume"].cumsum().to_numpy() * price if method in VOLUME_BASED else price
        if method == "pvt":
            scale = df["volume"].cumsum().to_numpy() * 100.0
        g = got if isinstance(got, pd.DataFrame) else got.to_frame()
        w = want if isinstance(want, pd.DataFrame) else want.to_frame()
        for c in w.columns:
            assert_close(g[c], w[c], scale)


def test_vwap_matches_pandas_ta(ta):
    df = FRAMES["walk"].iloc[:2000].copy()
    df.index = pd.date_range("2026-01-05 09:30", periods=len(df), freq="5min")
    for anchor in (None, "W"):
        want = df.ta.vwap(anchor=anchor)
        got = df.fp.vwap(anchor=anchor)
        assert got.name == want.name
        assert_close(got, want, np.full(len(df), 100.0))
    with pytest.raises(TypeError):
        FRAMES["walk"].fp.vwap()


def test_positional_append_and_errors():
    df = FRAMES["walk"].copy()
    assert df.fp.wma(20).name == "WMA_20"
    assert list(df.fp.stoch(5, 3, 3).columns) == ["STOCHk_5_3_3", "STOCHd_5_3_3"]
    df.fp.supertrend(append=True)
    df.fp.cci(append=True)
    assert {"SUPERT_7_3.0", "SUPERTd_7_3.0", "CCI_14_0.015"} <= set(df.columns)
    assert_close(df["close"].fp.hma(10), df.fp.hma(10), np.full(len(df), 100.0))
    with pytest.raises(TypeError):
        df.fp.adx(mamode="ema")
    with pytest.raises(TypeError):
        df.fp.wma(10, length=10)
    with pytest.raises(ValueError):
        df.fp.wma(0)
    with pytest.raises(ValueError):
        df["close"].fp.adx()


def test_leading_nan_is_skipped_like_pandas_ta(ta):
    df = FRAMES["walk"].iloc[:800].copy()
    df.iloc[:25] = np.nan
    for method in ("supertrend", "psar", "qqe", "aroon", "obv", "mfi", "adosc", "wma", "stoch"):
        want = getattr(df.ta, method)(talib=False)
        got = getattr(df.fp, method)()
        g = got if isinstance(got, pd.DataFrame) else got.to_frame()
        w = want if isinstance(want, pd.DataFrame) else want.to_frame()
        assert list(g.columns) == list(w.columns)
        scale = df["volume"].cumsum().fillna(0).to_numpy() * 1000 + 1000
        for c in w.columns:
            assert_close(g[c], w[c], scale)


# ---------------------------------------------------------------------------
# polars
# ---------------------------------------------------------------------------


@pytest.fixture(scope="module")
def pl():
    mod = pytest.importorskip("polars")
    import fin_primitives.polars  # noqa: F401

    return mod


@pytest.mark.parametrize("method", sorted(_pta.SPEC))
def test_polars_frame_namespace_matches_pandas(pl, method):
    df = FRAMES["real_like"]
    p = pl.from_pandas(df)
    got = getattr(p.fp, method)()
    want = getattr(df.fp, method)()
    w = want if isinstance(want, pd.DataFrame) else want.to_frame()
    cols = got.columns if isinstance(got, pl.DataFrame) else [got.name]
    assert cols == list(w.columns)
    scale = np.full(len(df), 1e12)
    for c in cols:
        values = (got[c] if isinstance(got, pl.DataFrame) else got).to_numpy()
        assert_close(values, w[c], scale)


def test_polars_expressions_match_pandas(pl):
    df = FRAMES["walk"]
    p = pl.from_pandas(df)
    exprs = {m: getattr(pl.col("close").fp, m)().alias(m) for m in fin_primitives.polars.EXPR_METHODS}
    out = p.select(list(exprs.values()))
    for m in exprs:
        want = getattr(df.fp, m)()
        assert_close(out[m].to_numpy(), want, np.full(len(df), 1e12))
    over = p.with_columns(pl.lit("A").alias("sym")).with_columns(pl.col("close").fp.wma(20).over("sym").alias("w"))
    assert_close(over["w"].to_numpy(), df.fp.wma(20), np.full(len(df), 100.0))
