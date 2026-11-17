"""``df.fp`` (fin_primitives.pandas) and ``pl.col(...).fp`` / ``df.fp`` (fin_primitives.polars)
against pandas-ta-classic 0.9.0's ``df.ta``: same method names, same defaults, same column
names, same numbers (within the tolerance test_fast.py uses), on fixed series."""
import numpy as np
import pytest

from test_fast import assert_same, real_like, walk

pd = pytest.importorskip("pandas")
import fin_primitives.pandas  # noqa: E402,F401  registers df.fp


def frames():
    for name, (h, l, c) in {"walk": walk(5000), "real_like": real_like()}.items():
        yield name, pd.DataFrame({"open": c, "high": h, "low": l, "close": c, "volume": np.full(len(c), 1000.0)})


@pytest.fixture(scope="module")
def ta():
    if not hasattr(np, "NaN"):
        np.NaN = np.nan
    return pytest.importorskip("pandas_ta_classic")


SINGLE = [
    ("sma", {}), ("sma", {"length": 50}),
    ("ema", {}), ("ema", {"length": 21}),
    ("rma", {}), ("rsi", {}), ("rsi", {"length": 7}),
    ("atr", {}), ("atr", {"length": 21}),
    ("true_range", {}),
    ("stdev", {}), ("stdev", {"length": 20}),
    ("variance", {}),
]


@pytest.mark.parametrize("method,kw", SINGLE)
def test_single_output_matches_pandas_ta(ta, method, kw):
    for _, df in frames():
        want = getattr(df.ta, method)(talib=False, **kw)
        got = getattr(df.fp, method)(**kw)
        assert got.name == want.name
        assert_same(got, want, scale=df["close"] if method in ("stdev", "variance") else None)


@pytest.mark.parametrize("kw", [{}, {"fast": 5, "slow": 35, "signal": 5}])
def test_macd_matches_pandas_ta(ta, kw):
    for _, df in frames():
        want = df.ta.macd(talib=False, **kw)
        got = df.fp.macd(**kw)
        assert list(got.columns) == list(want.columns)
        for c in want.columns:
            assert_same(got[c], want[c])


@pytest.mark.parametrize("kw", [{}, {"length": 20, "std": 2.5}])
def test_bbands_matches_pandas_ta(ta, kw):
    for _, df in frames():
        want = df.ta.bbands(talib=False, **kw)
        got = df.fp.bbands(**kw)
        assert list(got.columns) == list(want.columns)
        # On a flat window the band has zero width: fin_primitives' deviation is exactly 0 and
        # %B is 0/0 (NaN), while pandas-ta's rolling deviation keeps a ~1e-14 float residue and
        # gives an arbitrary %B there. Those rows are compared for being flat, not for %B.
        flat = (got.iloc[:, 2] - got.iloc[:, 0]).to_numpy() == 0
        for c in want.columns:
            if c.startswith("BBP"):
                assert np.isnan(got[c].to_numpy()[flat]).all()
                assert_same(got[c].to_numpy()[~flat], want[c].to_numpy()[~flat])
            else:
                assert_same(got[c], want[c], scale=df["close"])


def test_append_and_series_accessor():
    _, df = next(frames())
    df = df.copy()
    df.fp.rsi(append=True)
    df.fp.macd(append=True)
    assert {"RSI_14", "MACD_12_26_9", "MACDh_12_26_9", "MACDs_12_26_9"} <= set(df.columns)
    assert_same(df["close"].fp.ema(10), df.fp.ema(10))
    with pytest.raises(ValueError):
        df["close"].fp.atr()


def test_columns_found_in_any_case_or_by_name():
    _, df = next(frames())
    upper = df.rename(columns=str.title)
    assert_same(upper.fp.atr(), df.fp.atr())
    renamed = df.rename(columns={"close": "px"})
    assert_same(renamed.fp.rsi(close="px"), df.fp.rsi())
    with pytest.raises(KeyError):
        renamed.fp.rsi()


def test_formula_changing_arguments_are_refused():
    _, df = next(frames())
    for kw in ({"talib": True}, {"mamode": "ema"}, {"offset": 1}):
        with pytest.raises(TypeError):
            df.fp.atr(**kw)


def test_compute_runs_any_indicator():
    _, df = next(frames())
    out = df.fp.compute("sma", 10)
    assert out.name == "SMA_10"
    assert_same(out, df.fp.sma(10))


# ---------------------------------------------------------------------------
# polars
# ---------------------------------------------------------------------------


@pytest.fixture(scope="module")
def pl():
    mod = pytest.importorskip("polars")
    import fin_primitives.polars  # noqa: F401  registers the namespaces

    return mod


def test_polars_expressions_match_pandas(pl):
    for _, df in frames():
        p = pl.from_pandas(df)
        out = p.select(
            pl.col("close").fp.sma(20).alias("sma"),
            pl.col("close").fp.ema().alias("ema"),
            pl.col("close").fp.rma().alias("rma"),
            pl.col("close").fp.rsi(14).alias("rsi"),
            pl.col("close").fp.stdev(30).alias("stdev"),
            pl.col("close").fp.variance().alias("var"),
            pl.col("close").fp.macd_line().alias("macd"),
        )
        assert_same(out["sma"].to_numpy(), df.fp.sma(20))
        assert_same(out["ema"].to_numpy(), df.fp.ema())
        assert_same(out["rma"].to_numpy(), df.fp.rma())
        assert_same(out["rsi"].to_numpy(), df.fp.rsi(14))
        assert_same(out["stdev"].to_numpy(), df.fp.stdev(30))
        assert_same(out["var"].to_numpy(), df.fp.variance())
        assert_same(out["macd"].to_numpy(), df.fp.macd()["MACD_12_26_9"])


def test_polars_frame_namespace_matches_pandas(pl):
    for _, df in frames():
        p = pl.from_pandas(df)
        assert_same(p.fp.atr().to_numpy(), df.fp.atr())
        assert_same(p.fp.true_range().to_numpy(), df.fp.true_range())
        m = p.fp.macd()
        want = df.fp.macd()
        assert m.columns == list(want.columns)
        for c in m.columns:
            assert_same(m[c].to_numpy(), want[c])
        b = p.fp.bbands(20, 2.0)
        wb = df.fp.bbands(20, 2.0)
        assert b.columns == list(wb.columns)
        for c in b.columns:
            assert_same(b[c].to_numpy(), wb[c], scale=df["close"])
        both = p.fp.with_indicators(p.fp.atr(), m)
        assert "ATRr_14" in both.columns and "MACDs_12_26_9" in both.columns


def test_polars_over_groups(pl):
    _, df = next(frames())
    a, b = df.iloc[:2000], df.iloc[2000:4000]
    p = pl.concat([pl.from_pandas(a).with_columns(pl.lit("A").alias("sym")), pl.from_pandas(b).with_columns(pl.lit("B").alias("sym"))])
    out = p.with_columns(pl.col("close").fp.rsi(14).over("sym").alias("rsi"))
    assert_same(out.filter(pl.col("sym") == "A")["rsi"].to_numpy(), a.fp.rsi(14))
    assert_same(out.filter(pl.col("sym") == "B")["rsi"].to_numpy(), b.reset_index(drop=True).fp.rsi(14))
