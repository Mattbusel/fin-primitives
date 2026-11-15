"""Inputs, modes, the catalogue and error handling."""
import math
from decimal import Decimal

import pytest

import fin_primitives as fp
from fin_primitives import Indicator


# -- inputs --------------------------------------------------------------------------------

@pytest.mark.parametrize("value", ["101.25", 101, 101.25, Decimal("101.25"), Decimal("1.0125E+2")])
def test_prices_accept_str_int_float_decimal(value):
    ind = Indicator("sma", 1)
    out = ind.update(value)
    assert isinstance(out, Decimal)
    assert out == Decimal(str(value)) if not isinstance(value, Decimal) else out == value


def test_float_input_is_its_shortest_decimal():
    ind = Indicator("sma", 1)
    assert str(ind.update(0.1)) == "0.1"
    assert str(ind.update(0.30000000000000004)) == "0.30000000000000004"


def test_exact_mode_has_no_binary_rounding():
    # 0.1 + 0.2 averaged in binary floating point is 0.15000000000000002.
    assert fp.sma([0.1, 0.2], 2)[-1] == Decimal("0.15")
    assert fp.sma(["0.1", "0.2"], 2)[-1] == Decimal("0.15")


def test_float_mode_returns_floats():
    ind = Indicator("sma", 2, mode="float")
    assert ind.update(1.0) is None
    out = ind.update(2)
    assert out == 1.5 and isinstance(out, float)


@pytest.mark.parametrize("bad", [True, None, "abc", float("nan"), float("inf"), "NaN", [1]])
def test_bad_prices_raise(bad):
    with pytest.raises((TypeError, ValueError)):
        Indicator("sma", 2).update(bad)


def test_price_and_quantity_types():
    assert str(fp.Price("0.1")) == "0.1"
    assert fp.Price(0.1).decimal == Decimal("0.1")
    assert fp.Price(Decimal("64250.50")).decimal == Decimal("64250.50")
    assert fp.Price(7).value == 7.0
    assert fp.Quantity("0").decimal == 0
    with pytest.raises(ValueError):
        fp.Price("-1")
    with pytest.raises(ValueError):
        fp.Quantity(-0.5)


def test_order_book_accepts_decimal_strings():
    book = fp.OrderBook("BTC-USD")
    book.apply_delta("bid", "64250.00", "1.2", True, 1)
    book.apply_delta("ask", Decimal("64250.50"), 0.8, True, 2)
    assert book.bid_ask_spread() == 0.5
    with pytest.raises(ValueError):
        book.apply_delta("bid", "64251", "1", True, 3)  # would cross the book


# -- construction ----------------------------------------------------------------------------

def test_keyword_parameters_and_defaults():
    a = Indicator("macd", 12, 26, 9)
    b = Indicator("macd", fast_period=12, slow_period=26, signal_period=9)
    c = Indicator("macd", 12, slow_period=26, signal_period=9)
    d = Indicator("macd")
    assert a.params == b.params == c.params == d.params == (12, 26, 9)
    assert repr(a) == "Indicator('macd', 12, 26, 9)"
    assert a.label == "macd_12_26_9"


@pytest.mark.parametrize("key", sorted(fp.DEFAULTS))
def test_every_default_builds(key):
    ind = Indicator(key)
    assert ind.key == key


def test_aliases_and_key_normalisation():
    assert Indicator("bollinger", 20, 2).key == "bollinger_b"
    assert Indicator("PSAR", "0.02", "0.2").key == "parabolic_sar"
    assert Indicator("Bollinger-B", 20, 2).key == "bollinger_b"
    assert fp.resolve("stochastic") == "stochastic_k"
    assert fp.aliases()["hullma"] == "hull_ma"


def test_parameter_errors():
    with pytest.raises(KeyError, match="did you mean"):
        Indicator("rs", 14)
    with pytest.raises(TypeError, match="missing parameter"):
        Indicator("bollinger_b", 20)
    with pytest.raises(TypeError, match="unexpected keyword"):
        Indicator("rsi", 14, length=3)
    with pytest.raises(TypeError, match="takes 1 parameter"):
        Indicator("rsi", 14, 3)
    with pytest.raises(ValueError):
        Indicator("rsi", 0)
    with pytest.raises(ValueError):
        Indicator("macd", 26, 12, 9)  # fast must be below slow
    with pytest.raises(TypeError):
        Indicator("rsi", "14")
    with pytest.raises(ValueError, match="mode"):
        Indicator("rsi", 14, mode="fast")


def test_streaming_state_reset_and_chunks():
    closes = [100 + (i % 7) - (i % 3) for i in range(60)]
    whole = Indicator("rsi", 14).batch(closes)
    ind = Indicator("rsi", 14)
    chunked = ind.batch(closes[:25]) + ind.batch(closes[25:])
    assert chunked == whole
    assert ind.is_ready and ind.period == 14
    ind.reset()
    assert not ind.is_ready
    assert ind.batch(closes) == whole


def test_full_bar_indicators_use_high_low_volume():
    # ATR needs the range; with close only the true range is the close-to-close move.
    ind = Indicator("atr", 2)
    ind.update_bar(10, high=11, low=9)
    ind.update_bar(10, high=12, low=8)
    assert ind.update_bar(10, high=13, low=7) == Decimal(5)  # mean of TR 4 and 6
    obv = Indicator("obv")
    obv.update_bar(10, volume=100)
    assert obv.update_bar(11, volume=250) == Decimal(250)


# -- catalogue ------------------------------------------------------------------------------

def test_catalogue_is_the_whole_rust_library():
    cat = fp.catalogue()
    keys = [e["key"] for e in cat]
    assert len(cat) == 743 and len(set(keys)) == len(keys)
    by_key = {e["key"]: e for e in cat}
    for k in ["sma", "ema", "rsi", "macd", "bollinger_b", "atr", "adx", "vwap", "obv", "ichimoku", "super_trend"]:
        assert k in by_key
    assert by_key["macd"]["type"] == "Macd"
    assert by_key["macd"]["params"] == [("fast_period", "int"), ("slow_period", "int"), ("signal_period", "int")]
    assert by_key["rsi"]["doc"]


def _sample_args(params):
    ints = iter([3, 5, 8, 13, 21])
    out = []
    for name, ty in params:
        if ty == "int":
            out.append(next(ints))
        elif ty == "int[4]":
            out.append([2, 3, 4, 5])
        elif ty in ("decimal", "str"):
            out.append("0.5" if "alpha" in name or "factor" in name or "step" in name or "af" in name else "2")
        elif ty in ("float", "float|None"):
            out.append(0.5)
        elif ty == "bool":
            out.append(True)
        else:  # pragma: no cover - the generator only emits the tags above
            raise AssertionError(ty)
    return out


def test_every_catalogue_entry_runs():
    """Build each indicator with small sample parameters and feed it 80 bars.

    A few constructors reject the generic sample parameters (each has its own constraints);
    those must fail with ValueError, never crash. Everything that builds must run.
    """
    import random

    rng = random.Random(7)
    bars = []
    price = 100.0
    for _ in range(80):
        o = price
        price = max(1.0, price + rng.uniform(-1.5, 1.5))
        bars.append((round(price, 2), round(max(o, price) + rng.uniform(0, 1), 2),
                     round(min(o, price) - rng.uniform(0, 1), 2), round(o, 2), float(rng.randint(100, 900))))
    built = 0
    produced = 0
    rejected = []
    for e in fp.catalogue():
        try:
            ind = Indicator(e["key"], *_sample_args(e["params"]))
        except ValueError:
            rejected.append(e["key"])
            continue
        built += 1
        fast = Indicator(e["key"], *_sample_args(e["params"]), mode="float")
        got_value = False
        for c, h, lo, o, v in bars:
            try:
                x = ind.update_bar(c, high=h, low=lo, open=o, volume=v)
                y = fast.update_bar(c, high=h, low=lo, open=o, volume=v)
            except ValueError:
                break  # a checked arithmetic error is fine; a crash is not
            assert (x is None) == (y is None), e["key"]
            if x is not None:
                got_value = True
                assert isinstance(x, Decimal)
                assert y == float(x) or (math.isnan(y) and x.is_nan()), e["key"]
        produced += got_value
    assert built >= 700, (built, rejected)
    assert produced >= 650, produced


def test_catalogue_copies_are_independent():
    entry = next(e for e in fp.catalogue() if e["key"] == "sma")
    entry["params"].append(("bogus", "int"))
    entry["doc"] = "changed"
    again = next(e for e in fp.catalogue() if e["key"] == "sma")
    assert again["params"] == [("period", "int")]
    assert again["doc"] != "changed"
    assert Indicator("sma", 3).update(1) is None


@pytest.mark.parametrize("tiny", [1e-30, -1e-29, 5e-300, "1e-30", "0.00000000000000000000000000001"])
def test_values_that_would_round_to_zero_raise(tiny):
    with pytest.raises(ValueError):
        Indicator("sma", 1).update(tiny)
    with pytest.raises(ValueError):
        fp.Price(tiny)


def test_tiny_values_in_float_batches_raise():
    with pytest.raises(ValueError, match="out of the Decimal range"):
        Indicator("sma", 2, mode="float").batch([1.0, 1e-30, 2.0])


def test_small_but_representable_values_still_work():
    assert Indicator("sma", 1).update(0.0) == Decimal(0)
    assert Indicator("sma", 1).update("0.0000000000000000000000000001") == Decimal("1e-28")
    assert Indicator("sma", 1).update("0E-30") == Decimal(0)
    assert Indicator("sma", 1).update(1.5e-20) == Decimal("1.5e-20")


def test_indicator_cannot_be_pickled_or_copied():
    import copy
    import pickle

    ind = Indicator("rsi", 14)
    for attempt in (pickle.dumps, copy.deepcopy, copy.copy):
        with pytest.raises(TypeError):
            attempt(ind)
    twin = Indicator(ind.key, *ind.params, mode=ind.mode)
    assert twin.params == ind.params and twin.key == "rsi"
