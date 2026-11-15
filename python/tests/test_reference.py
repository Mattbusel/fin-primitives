"""The Python bindings give the same numbers as the Rust crate.

``data/reference.json`` is written by the Rust example ``indicator_reference``
(``cargo run --example indicator_reference > python/tests/data/reference.json``): 120 seeded
OHLCV bars and the output of ``Rsi``, ``Ema``, ``Sma``, ``Macd``, ``BollingerB`` and ``Atr``
called directly from Rust. Exact mode must match those decimals digit for digit; float mode
must equal ``float()`` of them.
"""
import json
import math
import pathlib
from decimal import Decimal

import pytest

import fin_primitives as fp

DATA = json.loads((pathlib.Path(__file__).parent / "data" / "reference.json").read_text())
BARS = DATA["bars"]
CASES = DATA["cases"]
N = len(BARS["close"])


def expected(case):
    return [None if v is None else Decimal(v) for v in CASES[case]["values"]]


def build(case, mode="exact"):
    kind, *params = CASES[case]["indicator"]
    return fp.Indicator(kind, *params, mode=mode)


def feed_exact(ind, i):
    return ind.update_bar(
        BARS["close"][i], high=BARS["high"][i], low=BARS["low"][i], open=BARS["open"][i], volume=BARS["volume"][i]
    )


def test_cases_cover_the_six_indicators():
    assert sorted(CASES) == ["atr_14", "bollinger_b_20_2", "ema_10", "macd_12_26_9", "rsi_14", "sma_20"]
    assert N == 120


@pytest.mark.parametrize("case", sorted(CASES))
def test_exact_streaming_matches_rust(case):
    ind = build(case)
    got = [feed_exact(ind, i) for i in range(N)]
    assert got == expected(case)
    # Not just equal as numbers: the same decimal text, digit for digit.
    assert [None if v is None else str(v) for v in got] == CASES[case]["values"]
    assert all(v is None or isinstance(v, Decimal) for v in got)


@pytest.mark.parametrize("case", sorted(CASES))
def test_exact_batch_matches_rust(case):
    ind = build(case)
    got = ind.batch(BARS["close"], high=BARS["high"], low=BARS["low"], open=BARS["open"], volume=BARS["volume"])
    assert got == expected(case)


@pytest.mark.parametrize("case", sorted(CASES))
def test_float_mode_matches_rust(case):
    want = [None if v is None else float(v) for v in expected(case)]
    cols = {k: [float(x) for x in BARS[k]] for k in ("open", "high", "low", "close", "volume")}

    ind = build(case, mode="float")
    streamed = [
        ind.update_bar(cols["close"][i], cols["high"][i], cols["low"][i], cols["open"][i], cols["volume"][i])
        for i in range(N)
    ]
    assert streamed == want

    ind = build(case, mode="float")
    batched = ind.batch(cols["close"], high=cols["high"], low=cols["low"], open=cols["open"], volume=cols["volume"])
    assert batched == want


@pytest.mark.parametrize("case", ["rsi_14", "ema_10", "sma_20", "macd_12_26_9", "bollinger_b_20_2"])
def test_close_only_indicators_via_update(case):
    # These read only the close, so update(close) gives the same as update_bar(...).
    ind = build(case)
    assert [ind.update(c) for c in BARS["close"]] == expected(case)


def test_named_helpers_match_rust():
    close = [Decimal(c) for c in BARS["close"]]
    high = [Decimal(c) for c in BARS["high"]]
    low = [Decimal(c) for c in BARS["low"]]
    assert fp.rsi(close, 14) == expected("rsi_14")
    assert fp.ema(close, 10) == expected("ema_10")
    assert fp.sma(close, 20) == expected("sma_20")
    assert fp.macd(close, 12, 26, 9) == expected("macd_12_26_9")
    assert fp.bollinger_b(close, 20, 2) == expected("bollinger_b_20_2")
    assert fp.atr(high, low, close, 14) == expected("atr_14")


def test_sma_against_plain_python_decimal():
    # An independent check that needs no Rust output: SMA is a mean of Decimals.
    close = [Decimal(c) for c in BARS["close"]]
    got = fp.sma(close, 20)
    for i in range(19, N):
        assert got[i] == sum(close[i - 19 : i + 1]) / 20


def test_rsi_bounds_and_macd_sign_flip():
    r = [v for v in fp.rsi(BARS["close"], 14) if v is not None]
    assert all(Decimal(0) <= v <= Decimal(100) for v in r)
    m = [v for v in fp.macd(BARS["close"]) if v is not None]
    assert any(v > 0 for v in m) and any(v < 0 for v in m)
    assert not any(isinstance(v, float) and math.isnan(v) for v in m)
