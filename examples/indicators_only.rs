//! The smallest integration: indicators only, no other domain modules.
//!
//! ```toml
//! fin-primitives = { version = "3.8", default-features = false, features = ["indicators"] }
//! ```
//!
//! Run: `cargo run --example indicators_only --no-default-features --features indicators`

use fin_primitives::signals::{indicators::Rsi, BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

fn last_rsi(closes: &[Decimal]) -> Result<Option<Decimal>, fin_primitives::FinError> {
    let mut rsi = Rsi::new("rsi14", 14)?;
    let mut last = None;
    for &close in closes {
        if let SignalValue::Scalar(v) = rsi.update(&BarInput::from_close(close))? {
            last = Some(v);
        }
    }
    Ok(last)
}

fn main() -> Result<(), fin_primitives::FinError> {
    let cents = [
        4434, 4409, 4415, 4361, 4433, 4483, 4510, 4542, 4584, 4608, 4589, 4603, 4561, 4628, 4628,
        4600,
    ];
    let closes: Vec<Decimal> = cents.iter().map(|&c| Decimal::new(c, 2)).collect();
    match last_rsi(&closes)? {
        Some(v) => println!("RSI(14) = {}", v.round_dp(2)),
        None => println!("RSI(14) still warming up"),
    }
    Ok(())
}
