//! Print reference values for RSI, EMA, SMA, MACD, Bollinger %B and ATR as JSON.
//!
//! The Python package's tests (`python/tests/test_reference.py`) compare the bindings
//! against this output, so the wheel and the Rust crate are checked to give the same
//! numbers, digit for digit. Regenerate the fixture after changing an indicator:
//!
//! ```text
//! cargo run --example indicator_reference > python/tests/data/reference.json
//! ```
//!
//! The bars are a seeded random walk generated below, so the output is the same on every run.

use fin_primitives::signals::indicators::{Atr, BollingerB, Ema, Macd, Rsi, Sma};
use fin_primitives::signals::{BarInput, Signal, SignalValue};
use fin_primitives::FinError;
use rust_decimal::Decimal;

const BARS: usize = 120;

/// One OHLCV bar with prices on a one-cent grid.
fn synthetic_bars(n: usize) -> Vec<BarInput> {
    let mut state: u64 = 0x5EED_F1A5_C0DE_0001;
    let mut next = move || {
        // xorshift64*: small, deterministic, good enough for test data.
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    };
    let cents = |v: i64| Decimal::new(v, 2);
    let mut close: i64 = 10_000; // 100.00
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let open = close;
        let step = (next() % 301) as i64 - 150; // -1.50 ..= +1.50
        close = (close + step).max(1_000);
        let up = (next() % 80) as i64;
        let down = (next() % 80) as i64;
        let high = open.max(close) + up;
        let low = (open.min(close) - down).max(1);
        let volume = 1_000 + (next() % 9_000) as i64;
        out.push(BarInput::new(
            cents(close),
            cents(high),
            cents(low),
            cents(open),
            Decimal::from(volume),
        ));
    }
    out
}

fn json_column(values: impl Iterator<Item = Decimal>) -> String {
    let items: Vec<String> = values.map(|d| format!("\"{d}\"")).collect();
    format!("[{}]", items.join(", "))
}

fn run(signal: &mut dyn Signal, bars: &[BarInput]) -> Result<String, FinError> {
    let mut items = Vec::with_capacity(bars.len());
    for bar in bars {
        items.push(match signal.update(bar)? {
            SignalValue::Scalar(d) => format!("\"{d}\""),
            SignalValue::Unavailable => String::from("null"),
        });
    }
    Ok(format!("[{}]", items.join(", ")))
}

fn main() -> Result<(), FinError> {
    let bars = synthetic_bars(BARS);
    let cases: Vec<(&str, &str, Box<dyn Signal>)> = vec![
        ("rsi_14", "[\"rsi\", 14]", Box::new(Rsi::new("rsi", 14)?)),
        ("ema_10", "[\"ema\", 10]", Box::new(Ema::new("ema", 10)?)),
        ("sma_20", "[\"sma\", 20]", Box::new(Sma::new("sma", 20)?)),
        (
            "macd_12_26_9",
            "[\"macd\", 12, 26, 9]",
            Box::new(Macd::new("macd", 12, 26, 9)?),
        ),
        (
            "bollinger_b_20_2",
            "[\"bollinger_b\", 20, 2]",
            Box::new(BollingerB::new("bb", 20, 2)?),
        ),
        ("atr_14", "[\"atr\", 14]", Box::new(Atr::new("atr", 14)?)),
    ];

    println!("{{");
    println!("  \"generator\": \"cargo run --example indicator_reference\",");
    println!("  \"bars\": {{");
    println!(
        "    \"open\": {},",
        json_column(bars.iter().map(|b| b.open))
    );
    println!(
        "    \"high\": {},",
        json_column(bars.iter().map(|b| b.high))
    );
    println!("    \"low\": {},", json_column(bars.iter().map(|b| b.low)));
    println!(
        "    \"close\": {},",
        json_column(bars.iter().map(|b| b.close))
    );
    println!(
        "    \"volume\": {}",
        json_column(bars.iter().map(|b| b.volume))
    );
    println!("  }},");
    println!("  \"cases\": {{");
    let last = cases.len() - 1;
    for (i, (key, spec, mut signal)) in cases.into_iter().enumerate() {
        let values = run(signal.as_mut(), &bars)?;
        let comma = if i == last { "" } else { "," };
        println!("    \"{key}\": {{\"indicator\": {spec}, \"values\": {values}}}{comma}");
    }
    println!("  }}");
    println!("}}");
    Ok(())
}
