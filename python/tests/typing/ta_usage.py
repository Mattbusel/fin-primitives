"""Checked by CI with ``mypy --strict`` against the installed wheel: what a project that
calls ``fin_primitives.ta`` sees. Not a pytest module."""
from __future__ import annotations

import numpy as np
import pandas as pd

from fin_primitives import ta

df = pd.DataFrame({"high": [2.0] * 40, "low": [1.0] * 40, "close": [1.5] * 40, "volume": [10.0] * 40})

rsi: pd.Series = ta.rsi(df["close"], length=14)
macd: pd.DataFrame = ta.macd(df["close"], 12, 26, 9)
bands: pd.DataFrame = ta.bbands(df["close"], length=20, std=2.0)
atr: pd.Series = ta.atr(df["high"], df["low"], df["close"])
kdj: pd.DataFrame = ta.kdj(df["high"], df["low"], df["close"], 9, 3)
fisher: pd.DataFrame = ta.fisher(df["high"], df["low"])
bias: pd.Series = ta.bias(df["close"], 26, "sma")
slope: pd.Series = ta.slope(np.asarray(df["close"]), 3, as_angle=True)
cloud: pd.DataFrame = ta.ichimoku(df["high"], df["low"], df["close"], include_chikou=False)
obv: pd.Series = ta.obv(df["close"].tolist(), df["volume"].tolist())
