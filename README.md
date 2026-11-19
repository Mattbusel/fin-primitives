# fin-primitives: your trading code probably can't agree with itself

English | [简体中文](README.zh-CN.md) | [日本語](README.ja.md) | [한국어](README.ko.md)

[![crates.io](https://img.shields.io/crates/v/fin-primitives.svg)](https://crates.io/crates/fin-primitives)
[![docs.rs](https://docs.rs/fin-primitives/badge.svg)](https://docs.rs/fin-primitives)
[![CI](https://github.com/Mattbusel/fin-primitives/actions/workflows/ci.yml/badge.svg)](https://github.com/Mattbusel/fin-primitives/actions/workflows/ci.yml)
[![MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Most trading code computes the same indicator more than one way and never notices. In a 73.5K-star stock tool, the screener, the alerts and the report each had their own RSI, and on the same 200-bar series one said oversold (26.36) while the others said neutral (50.19) ([the merged fix moved all three onto this crate](https://github.com/ZhuLinsen/daily_stock_analysis/pull/2537)). fin-primitives is one implementation of 700+ indicators, matched to TA-Lib to 2e-11 or better on 22 shared indicators, with exact decimal prices that refuse bad values. Python teams already run it without writing a line of Rust: that 73.5K-star Python project now ships it as a runtime dependency.

- Rejects bad prices, like a negative one.
- Rolls trades into candles.
- Runs an order book.
- Prices options with Greeks.
- Checks drawdown and equity limits.

## Install

```bash
cargo add fin-primitives rust_decimal rust_decimal_macros
```

## Use

```rust
use fin_primitives::signals::{indicators::Rsi, BarInput, Signal, SignalValue};
use fin_primitives::types::Price;
use rust_decimal_macros::dec;

fn main() -> Result<(), fin_primitives::FinError> {
    assert!(Price::new(dec!(-1)).is_err()); // bad values never get in

    let mut rsi = Rsi::new("rsi14", 14)?;
    let closes = [dec!(44.34), dec!(44.09), dec!(44.15), dec!(43.61), dec!(44.33), dec!(44.83), dec!(45.10), dec!(45.42),
                  dec!(45.84), dec!(46.08), dec!(45.89), dec!(46.03), dec!(45.61), dec!(46.28), dec!(46.28), dec!(46.00)];
    for close in closes {
        if let SignalValue::Scalar(v) = rsi.update(&BarInput::from_close(close))? {
            println!("RSI(14) = {}", v.round_dp(2)); // 70.46, then 66.25
        }
    }
    Ok(())
}
```

## Which RSI do you have?

Most indicator bugs are not arithmetic bugs. They are choices nobody wrote down: a simple average where Wilder used his smoothing, `ewm(span=14)` where Wilder's alpha is 1/14, a seed on the wrong bar, a warm-up filled with 50. `indicator-diff` runs one price series through every RSI, ATR and MACD installed (TA-Lib, pandas-ta, ta, the hand-rolled versions that get copied between projects, and yours) and names the formula each one actually computes, with the size of the gap and how many bars change verdict.

```bash
pip install fin-primitives pandas
indicator-diff --demo                       # the 500-bar standard series
indicator-diff prices.csv --load bot/ta.py --expr "rsi=compute_rsi(close, 14)"   # exits 1 if it is not Wilder's
```

We ran the indicator code of 20 popular open-source trading repos through it (plus two that inherit one library's numbers). All 20 differ from the textbook value somewhere, and 13 use a formula that never converges to Wilder's (three of them on purpose, following the Tongdaxin platform). The most common: Cutler's simple-average RSI, 10.2 points off Wilder's on the last bar of the standard series and on the other side of 30 or 70 on 92 bars, and a simple-average ATR, 5.4 % high and never converging. pandas-ta 0.4.71b0 gives different numbers depending on whether TA-Lib is installed. We had the same bug: until 4.0 our own `Atr::new` was a simple average. It is Wilder's now, everywhere in the crate.

[Docs](https://docs.rs/fin-primitives) · [crates.io](https://crates.io/crates/fin-primitives) · [Site](https://fin-primitives.vercel.app/)

More: [docs/DETAILS.md](docs/DETAILS.md) · Adding it to another project: [docs/INTEGRATION.md](docs/INTEGRATION.md)

MIT license. Does not place orders. Not financial advice.
