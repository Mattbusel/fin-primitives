"""Generate ``src/fin_primitives/ta.py``: pandas-ta's function API (``ta.rsi(close, length=14)``).

The functions are written out one by one, with typed signatures, so editors, mypy and
pyright see every name and parameter. They are read from ``src/fin_primitives/_pta.py``
(its ``SPEC`` table and function signatures) plus the indicators ``fin_primitives.pandas``
implements itself (``BUILTIN`` below). Run it from any directory after changing either:

    python python/tools/gen_ta.py

The generated file is committed. ``--check`` exits non-zero when it is stale. The script
only parses source files, so it runs without building the extension module.
"""
from __future__ import annotations

import ast
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
PTA = ROOT / "src" / "fin_primitives" / "_pta.py"
OUT = ROOT / "src" / "fin_primitives" / "ta.py"

COLUMNS = ("open_", "high", "low", "close", "volume")

# Indicators fin_primitives.pandas implements itself (not in _pta.SPEC):
# name -> (input columns in pandas-ta's order, [(parameter, default)], several columns?)
BUILTIN = {
    "sma": (("close",), [("length", None)], False),
    "ema": (("close",), [("length", None)], False),
    "rma": (("close",), [("length", None)], False),
    "rsi": (("close",), [("length", None)], False),
    "true_range": (("high", "low", "close"), [], False),
    "atr": (("high", "low", "close"), [("length", None)], False),
    "macd": (("close",), [("fast", None), ("slow", None), ("signal", None)], True),
    "bbands": (("close",), [("length", None), ("std", None), ("ddof", 0)], True),
    "stdev": (("close",), [("length", None), ("ddof", 0)], False),
    "variance": (("close",), [("length", None), ("ddof", 0)], False),
    "vwap": (("high", "low", "close", "volume"), [("anchor", None)], False),
}

# pandas-ta returns a tuple (frame, forward span frame) from ichimoku; df.fp.ichimoku covers it.
SKIP = {"ichimoku"}

INTS = {
    "length", "fast", "slow", "signal", "k", "d", "smooth_k", "lensig", "atr_length", "rsi_length",
    "lower_length", "upper_length", "medium", "smooth", "ddof",
}
BOOLS = {"include_chikou", "centered", "cumulative", "tr"}
STRS = {"anchor"}

# Where pandas-ta-classic 0.9.0's signatures differ from _pta's, so that positional calls
# bind the way they do in pandas-ta. NOT_SUPPORTED: parameters pandas-ta takes positionally
# that select another formula; they are accepted at their default only.
# name -> [(parameter, the parameter it comes before, annotation)]
NOT_SUPPORTED = {
    "adosc": [("open_", "fast", "Optional[SeriesLike]")],
    "cmf": [("open_", "length", "Optional[SeriesLike]")],
    "chop": [("ln", "scalar", "Optional[bool]")],
}
# Parameters pandas-ta takes only by name (from **kwargs, or after one this module lacks).
KEYWORD_ONLY = {"kc": {"tr"}, "zscore": {"ddof"}}


def ann(name: str, default: object) -> str:
    base = "int" if name in INTS else "bool" if name in BOOLS else "str" if name in STRS else "float"
    return base if default is not None else f"Optional[{base}]"


def read_pta() -> dict:
    tree = ast.parse(PTA.read_text(encoding="utf-8"))
    funcs = {n.name: n for n in tree.body if isinstance(n, ast.FunctionDef)}
    spec_keys: list = []
    multi: set = set()
    for node in tree.body:
        target = node.target if isinstance(node, ast.AnnAssign) else node.targets[0] if isinstance(node, ast.Assign) else None
        if isinstance(target, ast.Name) and target.id == "SPEC":
            spec_keys = [ast.literal_eval(k) for k in node.value.keys]  # type: ignore[union-attr]
        if isinstance(target, ast.Name) and target.id == "MULTI":
            multi = set(ast.literal_eval(node.value))  # type: ignore[arg-type]
    out = {}
    for key in spec_keys:
        if key in SKIP:
            continue
        fn = funcs[key]
        args = [a.arg for a in fn.args.args]
        defaults = [None] * (len(args) - len(fn.args.defaults)) + [ast.literal_eval(d) for d in fn.args.defaults]
        inputs = tuple(a for a in args if a in COLUMNS)
        params = [(a, d) for a, d in zip(args, defaults) if a not in COLUMNS]
        out[key] = (inputs, params, key in multi)
    return out


HEADER = '''"""pandas-ta's function API over fin-primitives' Rust kernels.

Code that calls pandas-ta's functions moves over by changing one import::

    import pandas_ta as ta                         # before
    from fin_primitives import ta                  # after

    df["RSI_14"] = ta.rsi(df["close"], length=14)
    macd = ta.macd(df["close"])                    # MACD_12_26_9, MACDh_12_26_9, MACDs_12_26_9
    atr = ta.atr(df["high"], df["low"], df["close"], length=14)   # ATRr_14

Every function takes pandas-ta's input series in pandas-ta's order, then pandas-ta's
parameters by position or name with pandas-ta's defaults, and returns a pandas Series or
DataFrame with pandas-ta's column names. The numbers are those of ``df.fp``
(:mod:`fin_primitives.pandas`), which the test suite checks against pandas-ta-classic 0.9.0.

Differences from pandas-ta:

* Input shorter than the period gives a result of ``NaN`` instead of ``None``.
* Arguments that select another formula or post-process the result (``talib``,
  ``mamode``, ``offset``, ``drift``, ``scalar`` where pandas-ta has it but this module
  does not, ``fillna``, ``fill_method``) raise ``TypeError`` rather than being ignored.
* Input series must share one index (take them from one DataFrame); otherwise
  ``ValueError`` is raised instead of aligning them. numpy arrays and lists are accepted
  and give a result with a ``RangeIndex``.
* ``ichimoku`` is not here (pandas-ta returns a tuple with a forward span frame); use
  ``df.fp.ichimoku()``.

Needs pandas and numpy (``pip install "fin-primitives[pandas]"``).

This file is generated by ``python/tools/gen_ta.py``; do not edit it by hand.
"""
from __future__ import annotations

from typing import Any, Dict, Optional, Sequence, Union, cast

import numpy as np
import pandas as pd

from .pandas import FinPrimitivesSeriesAccessor as _Accessor

SeriesLike = Union[pd.Series, np.ndarray, Sequence[float]]


def _series(value: Any, name: str) -> pd.Series:
    if isinstance(value, pd.Series):
        return value
    if isinstance(value, pd.DataFrame):
        raise TypeError(f"{name} must be a Series, got a DataFrame; pass df[{name!r}]")
    return pd.Series(np.asarray(value, dtype=np.float64), name=name)


def _unsupported(method: str, **given: Any) -> None:
    bad = sorted(k for k, v in given.items() if v is not None and v is not False)
    if bad:
        raise TypeError(f"{method}(): {', '.join(bad)} is not supported; fin_primitives.ta computes the default formula only")


def _call(method: str, inputs: Dict[str, Any], params: Dict[str, Any], kwargs: Dict[str, Any]) -> Any:
    columns = {k: _series(v, k) for k, v in inputs.items() if v is not None}
    ref_name, ref = next(iter(columns.items()))
    for k, s in columns.items():
        if len(s) != len(ref) or not s.index.equals(ref.index):
            raise ValueError(f"{method}(): {k} and {ref_name} have different indexes; take them from one DataFrame")
    clash = sorted(set(kwargs) & {"open", "high", "low", "close", "volume"})
    if clash:
        raise TypeError(f"{method}() got {', '.join(clash)} as a keyword; pass the series positionally")
    return getattr(_Accessor(ref), method)(**params, **columns, **kwargs)

'''


def render() -> str:
    entries = dict(BUILTIN)
    entries.update(read_pta())
    names = sorted(entries)
    lines = [HEADER, "\n__all__ = [\n" + "".join(f'    "{n}",\n' for n in names) + "]\n\n\n"]
    for name in names:
        inputs, params, multi = entries[name]
        sig, shown = [], []
        for c in inputs:
            optional = name == "psar" and c == "close"
            sig.append(f"{c}: Optional[SeriesLike] = None" if optional else f"{c}: SeriesLike")
            shown.append(f"{c}=None" if optional else c)
        extra = {before: (p, a) for p, before, a in NOT_SUPPORTED.get(name, [])}
        kwonly = KEYWORD_ONLY.get(name, set())
        star = False
        for p, d in params:
            if p in extra:
                sig.append(f"{extra[p][0]}: {extra[p][1]} = None")
                shown.append(f"{extra[p][0]}=None")
            if p in kwonly and not star:
                sig.append("*")
                shown.append("*")
                star = True
            sig.append(f"{p}: {ann(p, d)} = {d!r}")
            shown.append(f"{p}={d!r}")
        sig.append("**kwargs: Any")
        unsupported = [p for p, _, _ in NOT_SUPPORTED.get(name, [])]
        check = f"    _unsupported({name!r}, {', '.join(f'{u}={u}' for u in unsupported)})\n" if unsupported else ""
        ret = "pd.DataFrame" if multi else "pd.Series"
        cols = ", ".join(f'"{"open" if c == "open_" else c}": {c}' for c in inputs)
        args = ", ".join(f'"{p}": {p}' for p, _ in params)
        head = f"def {name}({', '.join(sig)}) -> {ret}:\n"
        if len(head) > 100:
            head = f"def {name}(\n" + "".join(f"    {s},\n" for s in sig) + f") -> {ret}:\n"
        lines.append(
            head
            + f'    """pandas-ta\'s ``{name}({", ".join(shown)})``.\n\n'
            + f'    Same defaults, column names and values as ``df.fp.{name}``.\n'
            + '    """\n'
            + check
            + f'    return cast("{ret}", _call({name!r}, {{{cols}}}, {{{args}}}, kwargs))\n\n\n'
        )
    return "".join(lines).rstrip("\n") + "\n"


def main() -> int:
    text = render()
    if "--check" in sys.argv:
        current = OUT.read_text(encoding="utf-8") if OUT.exists() else ""
        if current != text:
            print(f"{OUT} is stale; run python python/tools/gen_ta.py", file=sys.stderr)
            return 1
        return 0
    OUT.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
