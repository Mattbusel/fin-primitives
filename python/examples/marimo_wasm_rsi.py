# /// script
# requires-python = ">=3.9"
# dependencies = [
#     "fin-primitives>=3.3.0",
#     "marimo",
#     "matplotlib",
#     "numpy",
# ]
# ///
"""RSI in exact decimal arithmetic, in a marimo notebook that also runs in the browser.

Run it locally:

    pip install fin-primitives marimo matplotlib numpy
    marimo edit python/examples/marimo_wasm_rsi.py

Run it in the browser (Pyodide / WebAssembly, nothing installed):

    marimo export html-wasm python/examples/marimo_wasm_rsi.py -o rsi_wasm --mode run

or paste the file into https://marimo.app or https://molab.marimo.io. In the browser,
`micropip` installs the `pyemscripten` wheel of fin-primitives from PyPI (PEP 783), so the
same Rust code that runs on a server computes the indicators in the page.
"""

import marimo

__generated_with = "0.17.0"
app = marimo.App(width="medium")


@app.cell
async def _():
    import sys

    if sys.platform == "emscripten":
        # Pyodide: install the WebAssembly wheel from PyPI before the import below.
        import micropip

        await micropip.install("fin-primitives")

    import marimo as mo
    import numpy as np
    import matplotlib.pyplot as plt
    import fin_primitives as fp

    return fp, mo, np, plt, sys


@app.cell
def _(mo, sys):
    where = "in your browser (Pyodide)" if sys.platform == "emscripten" else "in local Python"
    mo.md(
        f"""
        # RSI with fin-primitives

        Running {where}. The closes are a seeded random walk; move the sliders and every
        value is recomputed by the Rust indicator code in exact decimal arithmetic.
        """
    )
    return


@app.cell
def _(mo):
    period = mo.ui.slider(2, 50, value=14, label="RSI period", show_value=True)
    seed = mo.ui.slider(0, 99, value=7, label="random seed", show_value=True)
    mo.hstack([period, seed])
    return period, seed


@app.cell
def _(np, seed):
    rng = np.random.default_rng(seed.value)
    # A random walk of daily closes starting at 100, rounded to cents like real quotes.
    closes = np.round(100 * np.exp(np.cumsum(rng.normal(0, 0.015, 250))), 2)
    return (closes,)


@app.cell
def _(closes, fp, period):
    # Float mode: numpy array in, float64 array out (NaN while warming up).
    rsi = fp.rsi(closes, period.value)
    # Exact mode: the same indicator on strings returns decimal.Decimal values.
    exact = fp.rsi([f"{c:.2f}" for c in closes], period.value)
    return exact, rsi


@app.cell
def _(closes, plt, rsi):
    fig, (top, bottom) = plt.subplots(
        2, 1, figsize=(9, 5), sharex=True, gridspec_kw={"height_ratios": [2, 1]}
    )
    top.plot(closes, color="#1f2937", linewidth=1)
    top.set_ylabel("close")
    bottom.plot(rsi, color="#2563eb", linewidth=1)
    bottom.axhline(70, color="#9ca3af", linestyle="--", linewidth=0.8)
    bottom.axhline(30, color="#9ca3af", linestyle="--", linewidth=0.8)
    bottom.set_ylim(0, 100)
    bottom.set_ylabel("RSI")
    bottom.set_xlabel("bar")
    fig.tight_layout()
    fig
    return


@app.cell
def _(closes, exact, mo, rsi):
    last = [
        {"bar": i, "close": f"{closes[i]:.2f}", "rsi (float)": f"{rsi[i]:.6f}", "rsi (exact)": str(exact[i])}
        for i in range(len(closes) - 5, len(closes))
    ]
    mo.vstack([mo.md("Last five bars, float mode next to exact decimal mode:"), mo.ui.table(last, selection=None)])
    return


if __name__ == "__main__":
    app.run()
