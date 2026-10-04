//! Already using the `ta` crate? Keep your ta indicators and add fin-primitives
//! where exact decimals matter: the same bars feed both.
//!
//! ```text
//! cargo run --example with_ta --features ta
//! ```
//!
//! Ticks are rolled into one-minute bars by fin-primitives; each finished bar goes
//! to ta's MACD and Bollinger Bands (through the `ta` feature's trait impls) and to
//! fin-primitives' Wilder RSI. Prices are a seeded walk, not market data.

use fin_primitives::ohlcv::{OhlcvAggregator, Timeframe};
use fin_primitives::signals::indicators::Rsi;
use fin_primitives::signals::{Signal, SignalValue};
use fin_primitives::tick::Tick;
use fin_primitives::types::{NanoTimestamp, Price, Quantity, Side, Symbol};
use fin_primitives::FinError;
use rust_decimal::Decimal;
use ta::indicators::{BollingerBands, MovingAverageConvergenceDivergence};
use ta::Next;

fn main() -> Result<(), FinError> {
    let sym = Symbol::new("BTC-USD")?;
    let mut bars = OhlcvAggregator::new(sym.clone(), Timeframe::Minutes(1))?;
    let mut macd = MovingAverageConvergenceDivergence::new(12, 26, 9).map_err(|e| FinError::InvalidInput(e.to_string()))?;
    let mut bb = BollingerBands::new(20, 2.0).map_err(|e| FinError::InvalidInput(e.to_string()))?;
    let mut rsi = Rsi::new("rsi14", 14)?;

    let mut cents: i64 = 6_425_050;
    let mut state: u64 = 42;
    println!("{:>5} {:>10} {:>9} {:>9} {:>10} {:>10}", "bar", "close", "rsi14", "macd", "bb upper", "bb lower");
    for i in 0..60 * 50 {
        state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        cents += ((state >> 33) % 2_001) as i64 - 1_000;
        let tick = Tick::new(
            sym.clone(),
            Price::new(Decimal::new(cents, 2))?,
            Quantity::new(Decimal::new(25, 3))?,
            Side::Bid,
            NanoTimestamp::from_secs(1_767_625_200 + i),
        );
        for bar in bars.push_tick(&tick)? {
            // ta reads the bar directly: `&OhlcvBar` implements ta::Close and friends.
            let m = macd.next(&bar);
            let b = bb.next(&bar);
            let r = rsi.update_bar(&bar)?;
            let n = bars.bar_count();
            if n % 5 == 0 {
                let r = match r {
                    SignalValue::Scalar(v) => format!("{:.2}", v),
                    SignalValue::Unavailable => "-".into(),
                };
                println!("{n:>5} {:>10} {r:>9} {:>9.2} {:>10.2} {:>10.2}", bar.close.value(), m.macd, b.upper, b.lower);
            }
        }
    }
    Ok(())
}
