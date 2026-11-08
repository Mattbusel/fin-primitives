# fin-primitives

English | [简体中文](README.zh-CN.md) | [日本語](README.ja.md) | [한국어](README.ko.md)

[![crates.io](https://img.shields.io/crates/v/fin-primitives.svg)](https://crates.io/crates/fin-primitives)
[![docs.rs](https://docs.rs/fin-primitives/badge.svg)](https://docs.rs/fin-primitives)
[![CI](https://github.com/Mattbusel/fin-primitives/actions/workflows/ci.yml/badge.svg)](https://github.com/Mattbusel/fin-primitives/actions/workflows/ci.yml)
[![MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Trading basics for Rust, with exact prices.

700+ indicators. Matches TA-Lib to 2e-11 or better on 22 shared indicators.

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

[Docs](https://docs.rs/fin-primitives) · [crates.io](https://crates.io/crates/fin-primitives) · [Site](https://fin-primitives.vercel.app/)

More: [docs/DETAILS.md](docs/DETAILS.md)

MIT license. Does not place orders. Not financial advice.
