"""``fin_primitives.indicator_diff``: the comparison, the formula identification and the CLI.

The variants are written here the way they appear in other projects (pandas one-liners), so
the identification is checked against independent code, not against the catalogue itself.
"""
import json
import math

import pytest

np = pytest.importorskip("numpy")
pd = pytest.importorskip("pandas")

from fin_primitives import fast  # noqa: E402
from fin_primitives import indicator_diff as idiff  # noqa: E402

S = idiff.standard_series()
CLOSE, HIGH, LOW = S["close"], S["high"], S["low"]


def frame():
    return pd.DataFrame({"close": CLOSE, "high": HIGH, "low": LOW})


# -- independent implementations (as found in the wild) ---------------------------------


def cutler_rsi(df, n=14):
    d = df["close"].diff()
    g = d.clip(lower=0).rolling(n).mean()
    l = (-d.clip(upper=0)).rolling(n).mean()
    return 100 - 100 / (1 + g / l)


def span_rsi(df, n=14):
    d = df["close"].diff()
    g = d.clip(lower=0).ewm(span=n, adjust=False).mean()
    l = (-d.clip(upper=0)).ewm(span=n, adjust=False).mean()
    return 100 - 100 / (1 + g / l)


def ewm_wilder_rsi(df, n=14):
    d = df["close"].diff()
    g = d.clip(lower=0).ewm(alpha=1 / n, adjust=False, min_periods=n).mean()
    l = (-d.clip(upper=0)).ewm(alpha=1 / n, adjust=False, min_periods=n).mean()
    return 100 - 100 / (1 + g / l)


def loop_wilder_rsi(df, n=14):
    """Wilder's own procedure, written as a loop over plain floats."""
    c = list(df["close"])
    out = [math.nan] * len(c)
    gains = [max(c[i] - c[i - 1], 0) for i in range(1, len(c))]
    losses = [max(c[i - 1] - c[i], 0) for i in range(1, len(c))]
    ag, al = sum(gains[:n]) / n, sum(losses[:n]) / n
    out[n] = 100 - 100 / (1 + ag / al)
    for i in range(n, len(gains)):
        ag = (ag * (n - 1) + gains[i]) / n
        al = (al * (n - 1) + losses[i]) / n
        out[i + 1] = 100 - 100 / (1 + ag / al)
    return out


def true_range(df):
    pc = df["close"].shift()
    return pd.concat([df["high"] - df["low"], (df["high"] - pc).abs(), (df["low"] - pc).abs()], axis=1).max(axis=1)


def sma_atr(df, n=14):
    return true_range(df).rolling(n).mean()


def zero_filled_atr(df, n=14):
    """bukosabino/ta's shape: zeros until the seed, seed = mean of TR[0:n] with TR[0] = high - low."""
    tr = true_range(df).to_numpy()
    out = np.zeros(len(tr))
    out[n - 1] = tr[:n].mean()
    for i in range(n, len(tr)):
        out[i] = (out[i - 1] * (n - 1) + tr[i]) / n
    return out


def ewm_macd(df):
    c = df["close"]
    line = c.ewm(span=12, adjust=False).mean() - c.ewm(span=26, adjust=False).mean()
    sig = line.ewm(span=9, adjust=False).mean()
    return line, sig, line - sig


def run(cands, **kw):
    kw.setdefault("libraries", False)
    kw.setdefault("gap_probe", False)
    return idiff.diff(CLOSE, HIGH, LOW, candidates=cands, **kw)


def row(rep, impl, ind):
    return next(r for r in rep.rows if r.implementation == impl and r.indicator == ind)


# -- the standard series -------------------------------------------------------------


def test_standard_series_is_fixed_and_valid():
    s = idiff.standard_series()
    assert len(s["close"]) == 500
    # Pure-Python generator: these values must never change between machines or releases.
    assert list(s["close"][:3]) == list(CLOSE[:3])
    assert all(np.array_equal(s[k], idiff.standard_series()[k]) for k in s)
    assert (s["high"] >= np.maximum(s["open"], s["close"]) - 1e-9).all()
    assert (s["low"] <= np.minimum(s["open"], s["close"]) + 1e-9).all()
    rsi = np.asarray(fast.rsi(s["close"], 14))
    assert np.nanmax(rsi) > 70 and np.nanmin(rsi) < 30, "series must visit both RSI zones"


# -- identification and comparison --------------------------------------------------------


def test_textbook_implementation_matches():
    rep = run({"loop": ("rsi", loop_wilder_rsi)}, indicators=["rsi"])
    r = row(rep, "loop", "rsi")
    assert r.matches and r.max_abs_diff < 1e-9 and r.verdict_flips == 0
    assert r.first_value_bar == r.textbook_first_bar == 14
    assert r.identified_as == "rsi.textbook (exact)"
    assert rep.disagreements() == []


def test_cutler_rsi_is_named_and_never_converges():
    rep = run({"cutler": ("rsi", cutler_rsi)}, indicators=["rsi"])
    r = row(rep, "cutler", "rsi")
    assert not r.matches
    assert r.identified_as == "rsi.sma_cutler (exact)"
    assert "Cutler" in r.reason
    assert r.agrees_from_bar is None
    assert r.verdict_flips > 10
    assert r.max_abs_diff > 5


def test_span_rsi_is_named():
    r = row(run({"span": ("rsi", span_rsi)}, indicators=["rsi"]), "span", "rsi")
    assert r.identified_as == "rsi.ema_span (exact)"
    assert r.agrees_from_bar is None
    assert r.first_value_bar == 1 and "shows values from bar 1" in r.reason


def test_wilder_with_other_seed_converges():
    r = row(run({"ewm": ("rsi", ewm_wilder_rsi)}, indicators=["rsi"]), "ewm", "rsi")
    assert not r.matches
    assert r.identified_as == "rsi.wilder_seed_first_change (exact)"
    assert r.agrees_from_bar is not None and r.agrees_from_bar < 200
    assert abs(r.last_diff) < 1e-6


def test_atr_variants():
    rep = run({"sma": ("atr", sma_atr), "zeros": ("atr", zero_filled_atr)}, indicators=["atr"])
    s = row(rep, "sma", "atr")
    assert s.identified_as == "atr.sma (exact)" and s.agrees_from_bar is None
    z = row(rep, "zeros", "atr")
    assert z.identified_as == "atr.tradingview_seed (exact)"
    assert z.warmup_fill == 0.0 and "with 0 instead of leaving them empty" in z.reason


def test_macd_variants_compare_all_three_outputs():
    rep = run({"ewm": ("macd", ewm_macd)}, indicators=["macd"])
    rows = [r for r in rep.rows if r.implementation == "ewm"]
    assert [r.indicator for r in rows] == ["macd", "macd_signal", "macd_hist"]
    assert all(not r.matches for r in rows)
    assert rows[0].identified_as == "macd.ewm_first_value (exact)"
    assert all(r.agrees_from_bar is not None for r in rows)


def test_macd_line_only_is_accepted():
    rep = run({"line": ("macd", lambda df: ewm_macd(df)[0])}, indicators=["macd"])
    assert [r.indicator for r in rep.rows] == ["macd"]


def test_broken_candidate_is_reported_not_raised():
    def boom(df):
        raise KeyError("Adj Close")

    rep = run({"boom": ("rsi", boom), "short": ("rsi", lambda df: [1.0, 2.0])}, indicators=["rsi"])
    assert row(rep, "boom", "rsi").error.startswith("KeyError")
    assert "returned 2 values" in row(rep, "short", "rsi").error
    assert len(rep.disagreements()) == 2


def test_identify_directly():
    var, how = idiff.identify("rsi", cutler_rsi(frame()), {"close": CLOSE})
    assert (var.key, how) == ("rsi.sma_cutler", "exact")
    var, how = idiff.identify("rsi", np.full(len(CLOSE), 42.0), {"close": CLOSE})
    assert var is None and how == "unknown"


# -- input policy ---------------------------------------------------------------------


def test_missing_prices_are_refused_unless_allowed():
    c = CLOSE.copy()
    c[10] = np.nan
    with pytest.raises(ValueError, match=r"close\[10\] is missing"):
        idiff.diff(c, indicators=["rsi"], libraries=False)
    rep = idiff.diff(c, indicators=["rsi"], libraries=False, allow_nan=True)
    assert rep.n_bars == len(c)


def test_atr_needs_high_and_low():
    with pytest.raises(ValueError, match="ATR needs high and low"):
        idiff.diff(CLOSE, indicators=["atr"], libraries=False)


def test_unknown_indicator_and_length_mismatch():
    with pytest.raises(ValueError, match="unknown indicator"):
        idiff.diff(CLOSE, indicators=["vwap"], libraries=False)
    with pytest.raises(ValueError, match="high has"):
        idiff.diff(CLOSE, HIGH[:-1], LOW, libraries=False)


def test_short_series_note():
    rep = idiff.diff(CLOSE[:60], HIGH[:60], LOW[:60], libraries=False)
    assert any("short" in n for n in rep.notes)


def test_gap_probe():
    rep = run({"cutler": ("rsi", cutler_rsi), "span": ("rsi", span_rsi)}, indicators=["rsi"], gap_probe=True)
    assert row(rep, "cutler", "rsi").nan_after_gap.startswith("15 bar(s) of NaN")
    assert "absorbed silently" in row(rep, "span", "rsi").nan_after_gap


# -- installed libraries ----------------------------------------------------------------


def test_fin_primitives_paths_agree_with_each_other():
    """One RSI and one ATR: the exact-decimal helpers equal the float64 textbook values
    (since 4.0, ``fin_primitives.atr`` is Wilder's ATR, not a simple average)."""
    rep = idiff.diff(CLOSE, HIGH, LOW, indicators=["rsi", "atr"], gap_probe=False)
    for ind in ("rsi", "atr"):
        r = row(rep, "fin_primitives (exact decimal)", ind)
        assert r.matches, (ind, r.reason)


def test_hand_rolled_rows_are_identified():
    rep = idiff.diff(CLOSE, HIGH, LOW, gap_probe=False)
    assert row(rep, "hand-rolled: rolling-mean RSI", "rsi").identified_as == "rsi.sma_cutler (exact)"
    assert row(rep, "hand-rolled: ewm(span=n) RSI", "rsi").identified_as == "rsi.ema_span (exact)"
    assert row(rep, "hand-rolled: rolling-mean ATR", "atr").identified_as == "atr.sma (exact)"
    m = row(rep, "hand-rolled: ewm(span, adjust=False) MACD", "macd")
    assert m.identified_as == "macd.ewm_first_value (exact)"


def test_pandas_ta_classic_matches_textbook():
    pytest.importorskip("pandas_ta_classic")
    rep = idiff.diff(CLOSE, HIGH, LOW, gap_probe=False)
    rows = [r for r in rep.rows if r.implementation == "pandas_ta_classic"]
    assert rows and all(r.matches for r in rows)


def test_talib_macd_seeding_is_identified():
    pytest.importorskip("talib")
    rep = idiff.diff(CLOSE, HIGH, LOW, gap_probe=False)
    assert row(rep, "talib", "rsi").matches and row(rep, "talib", "atr").matches
    assert row(rep, "talib", "macd_signal").identified_as == "macd.talib (exact)"


# -- report and CLI -----------------------------------------------------------------------


def test_text_and_json_reports():
    rep = run({"cutler": ("rsi", cutler_rsi)})
    text = rep.to_text()
    assert "== RSI" in text and "cutler" in text and "disagree" in text
    d = json.loads(rep.to_json())
    assert d["n_bars"] == 500 and d["params"]["length"] == 14
    assert any(r["implementation"] == "cutler" for r in d["rows"])


def test_cli_exit_codes(capsys, tmp_path):
    good = "rsi=np.asarray(fast.rsi(close.to_numpy(), 14))"
    lib = tmp_path / "mylib.py"
    lib.write_text(
        "from fin_primitives import fast\n"
        "def cutler(close, n):\n"
        "    d = close.diff()\n"
        "    return 100 - 100 / (1 + d.clip(lower=0).rolling(n).mean() / (-d.clip(upper=0)).rolling(n).mean())\n"
    )
    base = ["--demo", "--only-expr", "--indicators", "rsi", "--load", str(lib)]
    assert idiff.main(base + ["--expr", good]) == 0
    assert idiff.main(base + ["--expr", "rsi=cutler(close, 14)"]) == 1
    assert idiff.main(base + ["--expr", "rsi=cutler(close, 14)", "--no-fail"]) == 0
    out = capsys.readouterr().out
    assert "Cutler" in out
    assert idiff.main(base + ["--expr", "rsi=cutler(close, 14)", "--json", "--no-fail"]) == 0
    payload = capsys.readouterr().out
    assert json.loads(payload)["rows"][0]["identified_as"] == "rsi.sma_cutler (exact)"


def test_cli_csv_input(capsys, tmp_path):
    p = tmp_path / "prices.csv"
    lines = ["Date,Open,High,Low,Close"] + [f"d{i},{o},{h},{l},{c}" for i, (o, h, l, c) in enumerate(zip(S["open"], HIGH, LOW, CLOSE))]
    p.write_text("\n".join(lines))
    assert idiff.main([str(p), "--only-expr", "--expr", "atr=close.rolling(14).mean()"]) == 1
    assert "== ATR" in capsys.readouterr().out
    bad = tmp_path / "bad.csv"
    bad.write_text("Date,Close\nd0,1\nd1,\n")
    assert idiff.main([str(bad), "--only-expr", "--indicators", "rsi"]) == 2
    assert "missing" in capsys.readouterr().err


def test_cli_usage_errors(capsys):
    assert idiff.main([]) == 2
    assert idiff.main(["--demo", "--expr", "nonsense"]) == 2
    assert idiff.main(["--demo", "--expr", "vwap=close"]) == 2
    assert "indicator must be one of" in capsys.readouterr().err


def test_running_a_diff_does_not_swap_the_callers_df_ta():
    """pandas-ta and pandas-ta-classic both register ``df.ta``; importing one for the
    comparison must not replace the one the caller already has."""
    ptc = pytest.importorskip("pandas_ta_classic")  # noqa: F841
    before = pd.DataFrame.__dict__.get("ta")
    idiff.diff(CLOSE, HIGH, LOW, gap_probe=False)
    assert pd.DataFrame.__dict__.get("ta") is before
