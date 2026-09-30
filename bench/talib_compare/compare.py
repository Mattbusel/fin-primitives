"""Compare fin-primitives indicator output and speed with TA-Lib.

Reads data/bars.csv (from gen_data.py), data/fp_values.csv and
data/fp_timing.csv (from the Rust harness), runs TA-Lib on the same bars and
prints a markdown report. Also writes data/results.json.

Numerical agreement is measured on every bar where both libraries produce a
value, and again after skipping the first SETTLE bars, because recursive
indicators (EMA family, RSI, ATR, ADX, ...) carry their seed forward and a
different seed only decays geometrically.
"""
import csv
import json
import os
import platform
import subprocess
import sys
import time

import numpy as np
import talib
from talib import stream

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(HERE, "data")
SETTLE = 1000  # bars skipped for the "after warmup" column
BATCH_REPEATS = 20
STREAM_BARS = 5000  # bars used for the per-bar Python stream timing


def load_bars():
    a = np.loadtxt(os.path.join(DATA, "bars.csv"), delimiter=",", skiprows=1)
    return {k: np.ascontiguousarray(a[:, i]) for i, k in enumerate(["open", "high", "low", "close", "volume"])}


def load_fp():
    cols = {}
    with open(os.path.join(DATA, "fp_values.csv")) as f:
        r = csv.reader(f)
        header = next(r)
        rows = list(r)
    for j, h in enumerate(header):
        cols[h] = np.array([float(row[j]) if row[j] != "" else np.nan for row in rows])
    timing = {}
    with open(os.path.join(DATA, "fp_timing.csv")) as f:
        for row in csv.DictReader(f):
            timing[row["indicator"]] = float(row["best_ns_per_bar"])
    return cols, timing


def talib_series(b):
    o, h, l, c, v = b["open"], b["high"], b["low"], b["close"], b["volume"]
    macd, macdsig, macdhist = talib.MACD(c, 12, 26, 9)
    up, mid, lo = talib.BBANDS(c, 20, 2.0, 2.0, 0)
    fk, fd = talib.STOCHF(h, l, c, 14, 3, 0)
    adown, aup = talib.AROON(h, l, 14)
    return {
        "SMA_20": talib.SMA(c, 20),
        "EMA_20": talib.EMA(c, 20),
        "WMA_20": talib.WMA(c, 20),
        "DEMA_20": talib.DEMA(c, 20),
        "TEMA_20": talib.TEMA(c, 20),
        "TRIX_15": talib.TRIX(c, 15),
        "KAMA_10": talib.KAMA(c, 10),
        "RSI_14": talib.RSI(c, 14),
        "MACDHIST_12_26_9": macdhist,
        "BBANDS_UPPER_20_2": up,
        "BBANDS_MIDDLE_20_2": mid,
        "BBANDS_LOWER_20_2": lo,
        "BBANDS_PCTB_20_2": (c - lo) / (up - lo),
        "ATR_14": talib.ATR(h, l, c, 14),
        "NATR_14": talib.NATR(h, l, c, 14),
        "STOCHF_K_14": fk,
        "STOCHF_D_14_3": fd,
        "WILLR_14": talib.WILLR(h, l, c, 14),
        "CCI_20": talib.CCI(h, l, c, 20),
        "ROC_10": talib.ROC(c, 10),
        "MOM_10": talib.MOM(c, 10),
        "MFI_14": talib.MFI(h, l, c, v, 14),
        "ADX_14": talib.ADX(h, l, c, 14),
        "AROONOSC_14": talib.AROONOSC(h, l, 14),
        "AROON_UP_14": aup,
        "AROON_DOWN_14": adown,
        "OBV": talib.OBV(c, v),
    }


# Alternative TA-Lib formulations that match a documented fin-primitives
# definition exactly, used to show *why* a column differs.
def talib_alternatives(b):
    h, l, c, v = b["high"], b["low"], b["close"], b["volume"]
    tr = talib.TRANGE(h, l, c)
    sma_tr = talib.SMA(tr, 14)
    return {
        "ATR_14": ("SMA(TRANGE, 14)", sma_tr),
        "NATR_14": ("SMA(TRANGE, 14) / close * 100", sma_tr / c * 100.0),
        "OBV": ("OBV - volume[0]", talib.OBV(c, v) - v[0]),
    }


def diff_stats(fp, ta):
    both = ~np.isnan(fp) & ~np.isnan(ta)
    idx = np.nonzero(both)[0]
    if len(idx) == 0:
        return None
    d = np.abs(fp[both] - ta[both])
    late = idx >= SETTLE
    scale = np.maximum(np.abs(ta[both]), 1e-12)
    return {
        "first_fp": int(np.argmax(~np.isnan(fp))),
        "first_ta": int(np.argmax(~np.isnan(ta))),
        "compared": int(both.sum()),
        "max_abs_all": float(d.max()),
        "max_abs_settled": float(d[late].max()) if late.any() else None,
        "max_rel_settled": float((d / scale)[late].max()) if late.any() else None,
        "bars_over_1e-9_settled": int((d[late] > 1e-9).sum()) if late.any() else None,
        "value_range": float(np.nanmax(np.abs(ta))),
    }


def time_batch(fn):
    best = float("inf")
    for _ in range(BATCH_REPEATS):
        t0 = time.perf_counter_ns()
        fn()
        best = min(best, time.perf_counter_ns() - t0)
    return best


def batch_calls(b):
    o, h, l, c, v = b["open"], b["high"], b["low"], b["close"], b["volume"]
    return {
        "SMA_20": lambda: talib.SMA(c, 20),
        "EMA_20": lambda: talib.EMA(c, 20),
        "WMA_20": lambda: talib.WMA(c, 20),
        "DEMA_20": lambda: talib.DEMA(c, 20),
        "TEMA_20": lambda: talib.TEMA(c, 20),
        "TRIX_15": lambda: talib.TRIX(c, 15),
        "KAMA_10": lambda: talib.KAMA(c, 10),
        "RSI_14": lambda: talib.RSI(c, 14),
        "MACDHIST_12_26_9": lambda: talib.MACD(c, 12, 26, 9),
        "BBANDS_PCTB_20_2": lambda: talib.BBANDS(c, 20, 2.0, 2.0, 0),
        "ATR_14": lambda: talib.ATR(h, l, c, 14),
        "NATR_14": lambda: talib.NATR(h, l, c, 14),
        "STOCHF_K_14": lambda: talib.STOCHF(h, l, c, 14, 3, 0),
        "STOCHF_D_14_3": lambda: talib.STOCHF(h, l, c, 14, 3, 0),
        "WILLR_14": lambda: talib.WILLR(h, l, c, 14),
        "CCI_20": lambda: talib.CCI(h, l, c, 20),
        "ROC_10": lambda: talib.ROC(c, 10),
        "MOM_10": lambda: talib.MOM(c, 10),
        "MFI_14": lambda: talib.MFI(h, l, c, v, 14),
        "ADX_14": lambda: talib.ADX(h, l, c, 14),
        "AROONOSC_14": lambda: talib.AROONOSC(h, l, 14),
        "OBV": lambda: talib.OBV(c, v),
    }


# Per-bar use from Python: TA-Lib keeps no state between calls, so the
# talib.stream functions recompute the last value from the arrays they are
# given. We pass a trailing window of WINDOW bars ending at each bar.
STREAM_WINDOW = 200


def stream_calls():
    return {
        "SMA_20": lambda o, h, l, c, v: stream.SMA(c, 20),
        "EMA_20": lambda o, h, l, c, v: stream.EMA(c, 20),
        "RSI_14": lambda o, h, l, c, v: stream.RSI(c, 14),
        "MACDHIST_12_26_9": lambda o, h, l, c, v: stream.MACD(c, 12, 26, 9),
        "BBANDS_PCTB_20_2": lambda o, h, l, c, v: stream.BBANDS(c, 20, 2.0, 2.0, 0),
        "ATR_14": lambda o, h, l, c, v: stream.ATR(h, l, c, 14),
        "STOCHF_K_14": lambda o, h, l, c, v: stream.STOCHF(h, l, c, 14, 3, 0),
        "OBV": lambda o, h, l, c, v: stream.OBV(c, v),
    }


def time_stream(b, fn):
    o, h, l, c, v = b["open"], b["high"], b["low"], b["close"], b["volume"]
    start = STREAM_WINDOW
    t0 = time.perf_counter_ns()
    for i in range(start, start + STREAM_BARS):
        s = slice(i - STREAM_WINDOW + 1, i + 1)
        fn(o[s], h[s], l[s], c[s], v[s])
    return (time.perf_counter_ns() - t0) / STREAM_BARS


def cpu_name():
    try:
        out = subprocess.run(
            ["powershell", "-NoProfile", "-Command", "(Get-CimInstance Win32_Processor).Name"],
            capture_output=True, text=True, timeout=30,
        ).stdout.strip()
        if out:
            return out
    except Exception:
        pass
    return platform.processor() or "unknown CPU"


def g(x):
    if x is None:
        return "n/a"
    if x == 0:
        return "0"
    return f"{x:.1e}"


def main():
    b = load_bars()
    n = len(b["close"])
    fp, fp_t = load_fp()
    ta = talib_series(b)
    alt = talib_alternatives(b)

    results = {"bars": n, "settle": SETTLE, "cpu": cpu_name(),
               "talib": talib.__ta_version__.decode() if isinstance(talib.__ta_version__, bytes) else str(talib.__ta_version__),
               "talib_python": talib.__version__, "numpy": np.__version__,
               "python": sys.version.split()[0], "indicators": {}}

    for name in ta:
        if name not in fp:
            continue
        st = diff_stats(fp[name], ta[name])
        entry = {"vs_talib": st}
        if name in alt:
            label, series = alt[name]
            entry["vs_alternative"] = {"formula": label, **diff_stats(fp[name], series)}
        results["indicators"][name] = entry

    batch = batch_calls(b)
    for name, fn in batch.items():
        ns = time_batch(fn) / n
        results["indicators"].setdefault(name, {})["talib_batch_ns_per_bar"] = ns
        results["indicators"][name]["fp_stream_ns_per_bar"] = fp_t.get(name)
    for name, fn in stream_calls().items():
        results["indicators"][name]["talib_py_stream_ns_per_bar"] = time_stream(b, fn)

    # ADX: directional-movement ties. In exact decimals up_move == down_move
    # gives +DM = -DM = 0; in float64 the two differences can differ by an ulp
    # and TA-Lib then assigns one side.
    h, l = b["high"], b["low"]
    up = h[1:] - h[:-1]
    dn = l[:-1] - l[1:]
    hc = np.round(h * 100).astype(np.int64)
    lc = np.round(l * 100).astype(np.int64)
    exact_tie = ((hc[1:] - hc[:-1]) == (lc[:-1] - lc[1:])) & ((hc[1:] - hc[:-1]) > 0)
    float_tie = np.zeros(len(h), bool)
    float_tie[1:] = exact_tie & (up != dn)
    far = np.ones(len(h), bool)
    for t_idx in np.nonzero(float_tie)[0]:
        far[t_idx:t_idx + 600] = False
    far[:SETTLE] = False
    adx_d = np.abs(fp["ADX_14"] - ta["ADX_14"])
    results["adx_dm_ties"] = {"exact_ties": int(exact_tie.sum()),
                              "float_unequal_among_them": int(float_tie.sum()),
                              "max_abs_diff_600_bars_clear_of_float_ties": float(np.nanmax(adx_d[far]))}

    # Which side carries the ~1e-11 Bollinger difference? Recompute the upper band
    # with Python's decimal module at 40 digits on every 37th bar.
    from decimal import Decimal, getcontext
    getcontext().prec = 40
    with open(os.path.join(DATA, "bars.csv")) as f:
        cdec = [Decimal(line.split(",")[3]) for line in f.read().splitlines()[1:]]
    err_ta, err_fp = 0.0, 0.0
    for i in range(19, n, 37):
        w = cdec[i - 19:i + 1]
        m = sum(w) / 20
        ref = float(m + 2 * (sum((x - m) ** 2 for x in w) / 20).sqrt())
        err_ta = max(err_ta, abs(ref - ta["BBANDS_UPPER_20_2"][i]))
        err_fp = max(err_fp, abs(ref - fp["BBANDS_UPPER_20_2"][i]))
    results["bbands_upper_vs_40_digit_reference"] = {"talib_max_err": err_ta, "fp_max_err": err_fp}

    with open(os.path.join(DATA, "results.json"), "w") as f:
        json.dump(results, f, indent=2)

    print(f"# fin-primitives vs TA-Lib: {n} bars, CPU {results['cpu']}")
    print(f"TA-Lib {results['talib']} (Python TA-Lib {results['talib_python']}, numpy {results['numpy']}, Python {results['python']})\n")
    print("## Agreement (max absolute difference)\n")
    print(f"| indicator | first value (fp / TA-Lib) | all common bars | after bar {SETTLE} | bars > 1e-9 after {SETTLE} | vs matching formula |")
    print("|---|---|---|---|---|---|")
    for name, e in results["indicators"].items():
        st = e.get("vs_talib")
        if not st:
            continue
        altc = ""
        if "vs_alternative" in e:
            a = e["vs_alternative"]
            altc = f"{a['formula']}: {g(a['max_abs_all'])}"
        print(f"| {name} | {st['first_fp']} / {st['first_ta']} | {g(st['max_abs_all'])} | {g(st['max_abs_settled'])} | {st['bars_over_1e-9_settled']} | {altc} |")
    t = results["adx_dm_ties"]
    print(f"\nADX: {t['exact_ties']} bars have up_move == down_move > 0 exactly; "
          f"in float64 {t['float_unequal_among_them']} of them compare unequal. Max ADX difference on "
          f"bars after {SETTLE} that are more than 600 bars past such a tie: {g(t['max_abs_diff_600_bars_clear_of_float_ties'])}.")
    r = results["bbands_upper_vs_40_digit_reference"]
    print(f"\nBollinger upper band vs a 40-digit decimal reference (every 37th bar): "
          f"TA-Lib max error {g(r['talib_max_err'])}, fin-primitives max error {g(r['fp_max_err'])}.")
    print("\n## Throughput (ns per bar, lower is better)\n")
    print("| indicator | fin-primitives streaming update() | TA-Lib batch (whole array) | TA-Lib per bar from Python (talib.stream, 200-bar window) |")
    print("|---|---|---|---|")
    for name, e in results["indicators"].items():
        if "talib_batch_ns_per_bar" not in e:
            continue
        ps = e.get("talib_py_stream_ns_per_bar")
        print(f"| {name} | {e['fp_stream_ns_per_bar']:.0f} | {e['talib_batch_ns_per_bar']:.1f} | {'' if ps is None else f'{ps:.0f}'} |")


if __name__ == "__main__":
    main()
