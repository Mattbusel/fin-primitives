//! Aggregate a raw tick stream into one-minute OHLCV bars, draw them as a
//! candlestick chart, and run EMA(9) and RSI(14) over the bars. RSI shows the
//! warm-up contract: it reports `Unavailable` until it has seen 15 bars.
//!
//! ```text
//! cargo run --example candles
//! ```
//!
//! The ticks are a seeded random walk generated below, so the output is the
//! same on every run. Swap `synthetic_ticks` for your own feed.

use fin_primitives::ohlcv::{OhlcvAggregator, OhlcvBar, Timeframe};
use fin_primitives::signals::indicators::{Ema, Rsi};
use fin_primitives::signals::{BarInput, Signal, SignalValue};
use fin_primitives::tick::Tick;
use fin_primitives::types::{NanoTimestamp, Price, Quantity, Side, Symbol};
use fin_primitives::FinError;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::fmt::Write as _;

mod support;
use support::{fixed, money, Paint};

const BARS: usize = 32;
const CHART_ROWS: usize = 19;

fn main() -> Result<(), FinError> {
    let paint = Paint::detect();
    let symbol = Symbol::new("ETH-USD")?;
    let ticks = synthetic_ticks(&symbol, BARS)?;

    // Ticks in, completed bars out. `flush` closes the last partial bar.
    let mut agg = OhlcvAggregator::new(symbol.clone(), Timeframe::Minutes(1))?;
    let mut bars: Vec<OhlcvBar> = Vec::new();
    for tick in &ticks {
        bars.extend(agg.push_tick(tick)?);
    }
    bars.extend(agg.flush());

    // Indicators consume bars and return `SignalValue::Unavailable` while warming up.
    let mut ema = Ema::new("ema9", 9)?;
    let mut rsi = Rsi::new("rsi14", 14)?;
    let mut rows = Vec::with_capacity(bars.len());
    for bar in &bars {
        let input = BarInput::from(bar);
        rows.push((bar, ema.update(&input)?, rsi.update(&input)?));
    }

    println!();
    println!(
        "  {}  {} ticks aggregated into {} one-minute bars",
        paint.bold(symbol.as_str()),
        ticks.len(),
        bars.len()
    );
    println!();
    let ema_line: Vec<Option<Decimal>> = rows.iter().map(|(_, e, _)| e.as_decimal()).collect();
    draw_chart(&bars, &ema_line, &paint);
    println!();
    println!(
        "  {} {}   {} {}   {} {}",
        paint.green("┃"),
        paint.dim("up bar"),
        paint.red("┃"),
        paint.dim("down bar"),
        paint.yellow("·"),
        paint.dim("ema9")
    );

    println!();
    println!(
        "  {:<5} {:>9} {:>9} {:>9} {:>9} {:>7} {:>5} {:>9} {:>7}",
        paint.dim("time"),
        paint.dim("open"),
        paint.dim("high"),
        paint.dim("low"),
        paint.dim("close"),
        paint.dim("volume"),
        paint.dim("ticks"),
        paint.dim("ema9"),
        paint.dim("rsi14")
    );
    let shown = [0usize, 1, 2, 12, 13, 14, 15, bars.len() - 2, bars.len() - 1];
    let mut last = 0;
    for &i in &shown {
        if i > last + 1 {
            println!("  {}", paint.dim("  ..."));
        }
        last = i;
        let (bar, ema_v, rsi_v) = &rows[i];
        let up = bar.close.value() >= bar.open.value();
        let close = money(bar.close.value(), 2);
        println!(
            "  {:<5} {:>9} {:>9} {:>9} {:>9} {:>7} {:>5} {:>9} {:>7}",
            clock(bar.ts_open),
            money(bar.open.value(), 2),
            money(bar.high.value(), 2),
            money(bar.low.value(), 2),
            if up {
                paint.green(&close)
            } else {
                paint.red(&close)
            },
            fixed(bar.volume.value(), 2),
            bar.tick_count,
            cell(ema_v, &paint),
            cell(rsi_v, &paint)
        );
    }

    let first_rsi = rows
        .iter()
        .position(|(_, _, r)| matches!(r, SignalValue::Scalar(_)));
    println!();
    if let Some(i) = first_rsi {
        println!(
            "  {}",
            paint.dim(&format!(
                "rsi14 returned Unavailable for bars 1-{i}; first value on bar {} ({}). No NaN, no zero-fill.",
                i + 1,
                clock(bars[i].ts_open)
            ))
        );
    }
    Ok(())
}

fn cell(v: &SignalValue, paint: &Paint) -> support::Styled {
    match v {
        SignalValue::Scalar(d) => paint.plain(&money(*d, 2)),
        SignalValue::Unavailable => paint.dim("warmup"),
    }
}

fn clock(ts: NanoTimestamp) -> String {
    ts.to_datetime().format("%H:%M").to_string()
}

/// Draws bars as vertical candles (`│` wick, `┃` body) with the EMA dotted
/// in the gap to the right of each candle.
fn draw_chart(bars: &[OhlcvBar], ema: &[Option<Decimal>], paint: &Paint) {
    let hi = bars
        .iter()
        .map(|b| b.high.value())
        .max()
        .unwrap_or_default();
    let lo = bars.iter().map(|b| b.low.value()).min().unwrap_or_default();
    let span = (hi - lo).max(Decimal::ONE);
    let rows = CHART_ROWS;
    // Row 0 is the top of the chart.
    let row_of = |p: Decimal| -> usize {
        let frac = (hi - p) / span * Decimal::from(rows - 1);
        frac.round().to_usize().unwrap_or(0).min(rows - 1)
    };
    for r in 0..rows {
        let mut line = String::new();
        for (bar, e) in bars.iter().zip(ema) {
            let (o, c) = (bar.open.value(), bar.close.value());
            let (top, bottom) = (row_of(o.max(c)), row_of(o.min(c)));
            let (wick_top, wick_bottom) = (row_of(bar.high.value()), row_of(bar.low.value()));
            let glyph = if r >= top && r <= bottom {
                "┃"
            } else if r >= wick_top && r <= wick_bottom {
                "│"
            } else {
                " "
            };
            let styled = if c >= o {
                paint.green(glyph)
            } else {
                paint.red(glyph)
            };
            let dot = match e {
                Some(v) if row_of(*v) == r => paint.yellow("·"),
                _ => paint.plain(" "),
            };
            let _ = write!(line, "{styled}{dot}");
        }
        let label = if r % 3 == 0 || r == rows - 1 {
            let price = hi - span * Decimal::from(r) / Decimal::from(rows - 1);
            format!("{}", paint.dim(&money(price, 2)))
        } else {
            String::new()
        };
        println!("  {line} {label}");
    }
    // Time axis: a label every 8 bars.
    let mut axis = String::new();
    let mut col = 0;
    for (i, bar) in bars.iter().enumerate() {
        let target = i * 2;
        if i % 8 == 0 && target >= col {
            axis.push_str(&" ".repeat(target - col));
            let t = clock(bar.ts_open);
            col = target + t.len();
            axis.push_str(&t);
        }
    }
    println!("  {}", paint.dim(&axis));
}

/// A seeded random walk: 2 to 9 seconds between ticks, sizes 0.01 to 4 ETH,
/// with a slow drift that turns halfway through so the chart has some shape.
fn synthetic_ticks(symbol: &Symbol, minutes: usize) -> Result<Vec<Tick>, FinError> {
    let mut rng = XorShift(0x5EED_F1A7_2026);
    let start = NanoTimestamp::from_secs(1_767_625_200); // 2026-01-05 15:00:00 UTC
    let end = start.nanos() + i64::try_from(minutes).unwrap_or(0) * 60 * 1_000_000_000;
    let mut t = start.nanos();
    let mut price = Decimal::new(318_450, 2); // 3,184.50
    let mut out = Vec::new();
    while t < end {
        #[allow(clippy::cast_precision_loss)]
        let elapsed = (t - start.nanos()) as f64 / (end - start.nanos()) as f64;
        let drift = if elapsed < 0.55 { 0.22 } else { -0.3 };
        let step = (rng.unit() - 0.5) * 6.0 + drift;
        price += Decimal::from_f64_retain(step)
            .unwrap_or_default()
            .round_dp(2);
        let qty = Decimal::from_f64_retain(0.01 + rng.unit().powi(3) * 4.0)
            .unwrap_or_default()
            .round_dp(3);
        let side = if rng.unit() < 0.5 {
            Side::Bid
        } else {
            Side::Ask
        };
        out.push(Tick::new(
            symbol.clone(),
            Price::new(price)?,
            Quantity::new(qty)?,
            side,
            NanoTimestamp::new(t),
        ));
        t += (2 + (rng.next() % 8) as i64) * 1_000_000_000;
    }
    Ok(out)
}

struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    #[allow(clippy::cast_precision_loss)]
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}
