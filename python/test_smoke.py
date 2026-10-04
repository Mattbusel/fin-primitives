"""Smoke test for the PyO3 bindings. Build and run:

    cd python && maturin develop --release && python test_smoke.py
"""
import _fin_primitives as fp

p = fp.PyPrice(0.1)
assert str(p) == "0.1", str(p)  # exact decimal, not 0.1000000000000000055...
try:
    fp.PyPrice(-1.0)
    raise AssertionError("negative price accepted")
except ValueError:
    pass

sma = fp.PySMA("sma3", 3)
assert [sma.update(x) for x in [1.0, 2.0, 3.0, 4.0]] == [None, None, 2.0, 3.0]

book = fp.PyOrderBook("BTC-USD")
book.apply_delta("bid", 64250.0, 1.2, True, 1)
book.apply_delta("ask", 64250.5, 0.8, True, 2)
assert book.bid_ask_spread() == 0.5
assert book.mid_price() == 64250.25
try:
    book.apply_delta("bid", 64251.0, 1.0, True, 3)  # would cross the book
    raise AssertionError("crossed book accepted")
except ValueError:
    pass
assert book.sequence() == 2

rsi = fp.PyRSI("rsi", 14)
vals = [rsi.update(100.0 + (i % 5)) for i in range(30)]
assert vals[13] is None and vals[14] is not None and 0.0 <= vals[-1] <= 100.0
print("python bindings ok")
