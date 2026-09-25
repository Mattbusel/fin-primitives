//! Run fills through a `PositionLedger`, mark it to market, and feed equity into
//! a `RiskMonitor` with a drawdown rule and an equity floor. Prints a session
//! log, the breaches as they fire, and the final book.
//!
//! ```text
//! cargo run --example position_risk
//! ```
//!
//! Prices below are made-up marks, not market data.
//!
//! Account value here is `net_liquidation_value` (cash plus the market value of
//! open positions). `PositionLedger::equity` is a different number, cash plus
//! unrealized P&L, and is not what you want to feed a drawdown monitor.

use fin_primitives::position::{Fill, PositionLedger};
use fin_primitives::risk::{MaxDrawdownRule, MinEquityRule, RiskMonitor};
use fin_primitives::types::{NanoTimestamp, Price, Quantity, Side, Symbol};
use fin_primitives::FinError;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;

mod support;
use support::{fixed, money, Paint};

enum Event {
    Fill(&'static str, Side, Decimal, Decimal),
    Mark(&'static str, Decimal),
}

fn main() -> Result<(), FinError> {
    let paint = Paint::detect();
    let start = dec!(100_000);
    let mut ledger = PositionLedger::new(start);
    let mut monitor = RiskMonitor::new(start)
        .add_rule(MaxDrawdownRule {
            threshold_pct: dec!(4),
        })
        .add_rule(MinEquityRule {
            floor: dec!(96_600),
        });
    let mut marks: HashMap<String, Price> = HashMap::new();

    use Event::{Fill as F, Mark as M};
    let session = [
        F("AAPL", Side::Bid, dec!(200), dec!(175.00)),
        F("MSFT", Side::Bid, dec!(80), dec!(410.00)),
        M("AAPL", dec!(178.40)),
        M("MSFT", dec!(415.25)),
        F("AAPL", Side::Ask, dec!(100), dec!(179.10)),
        M("MSFT", dec!(398.10)),
        M("AAPL", dec!(166.80)),
        M("MSFT", dec!(371.50)),
        F("MSFT", Side::Ask, dec!(80), dec!(372.00)),
        M("AAPL", dec!(171.35)),
    ];

    println!();
    println!(
        "  {}  start {}  rules: drawdown > 4%, equity < {}",
        paint.bold("session"),
        money(start, 2),
        money(dec!(96_600), 2)
    );
    println!();
    println!(
        "  {:<3} {:<22} {:>12} {:>9}  {}",
        paint.dim("#"),
        paint.dim("event"),
        paint.dim("equity"),
        paint.dim("drawdown"),
        paint.dim("risk")
    );

    for (i, event) in session.iter().enumerate() {
        let label = match *event {
            F(sym, side, qty, px) => {
                ledger.apply_fill(Fill {
                    symbol: Symbol::new(sym)?,
                    side,
                    quantity: Quantity::new(qty)?,
                    price: Price::new(px)?,
                    timestamp: NanoTimestamp::new(i64::try_from(i).unwrap_or(0)),
                    commission: dec!(1.00),
                })?;
                marks.insert(sym.to_owned(), Price::new(px)?);
                let verb = if side == Side::Bid { "buy " } else { "sell" };
                format!("{verb} {qty:>3} {sym} @ {}", fixed(px, 2))
            }
            M(sym, px) => {
                marks.insert(sym.to_owned(), Price::new(px)?);
                format!("mark {sym} {}", fixed(px, 2))
            }
        };
        let equity = ledger.net_liquidation_value(&marks)?;
        let breaches = monitor.update(equity);
        let dd = monitor.drawdown_pct();
        let dd_cell = format!("{}%", fixed(dd, 2));
        let status = if breaches.is_empty() {
            paint.green("ok")
        } else {
            let names: Vec<&str> = breaches.iter().map(|b| b.rule.as_str()).collect();
            paint.red(&format!("BREACH {}", names.join(", ")))
        };
        println!(
            "  {:<3} {:<22} {:>12} {:>9}  {}",
            i + 1,
            label,
            money(equity, 2),
            if dd > dec!(4) {
                paint.red(&dd_cell)
            } else {
                paint.plain(&dd_cell)
            },
            status
        );
        for b in &breaches {
            println!(
                "  {:<3} {}",
                "",
                paint.dim(&format!("  {}: {}", b.rule, b.detail))
            );
        }
    }

    // A buy the account cannot pay for is refused before it touches the book.
    let too_big = Fill {
        symbol: Symbol::new("AAPL")?,
        side: Side::Bid,
        quantity: Quantity::new(dec!(1000))?,
        price: Price::new(dec!(171.35))?,
        timestamp: NanoTimestamp::new(99),
        commission: dec!(1.00),
    };
    println!();
    if let Err(e) = ledger.apply_fill(too_big) {
        println!(
            "  {}  buy 1000 AAPL  {}",
            paint.red("rejected"),
            paint.dim(&e.to_string())
        );
    }

    println!();
    println!(
        "  {:<6} {:>6} {:>10} {:>10} {:>12} {:>12}",
        paint.dim("symbol"),
        paint.dim("qty"),
        paint.dim("avg cost"),
        paint.dim("mark"),
        paint.dim("unrealized"),
        paint.dim("realized")
    );
    for sym in ledger.symbols_sorted() {
        let Some(pos) = ledger.position(sym) else {
            continue;
        };
        let mark = marks.get(sym.as_str()).copied();
        let unreal = mark.map_or(Decimal::ZERO, |m| pos.unrealized_pnl(m));
        println!(
            "  {:<6} {:>6} {:>10} {:>10} {:>12} {:>12}",
            sym.as_str(),
            if pos.is_flat() {
                "flat".to_owned()
            } else {
                pos.quantity.to_string()
            },
            if pos.is_flat() {
                "-".to_owned()
            } else {
                fixed(pos.avg_cost, 2)
            },
            mark.map_or("-".to_owned(), |m| fixed(m.value(), 2)),
            signed(unreal, &paint),
            signed(pos.realized_pnl, &paint)
        );
    }
    let equity = ledger.net_liquidation_value(&marks)?;
    println!();
    println!(
        "  {:<14} {:>12}   {:<14} {:>9}",
        paint.dim("cash"),
        money(ledger.cash(), 2),
        paint.dim("peak equity"),
        money(monitor.peak_equity(), 2)
    );
    println!(
        "  {:<14} {:>12}   {:<14} {:>9}",
        paint.dim("equity"),
        money(equity, 2),
        paint.dim("drawdown"),
        format!("{}%", fixed(monitor.drawdown_pct(), 2))
    );
    Ok(())
}

fn signed(d: Decimal, paint: &Paint) -> support::Styled {
    match d.cmp(&Decimal::ZERO) {
        std::cmp::Ordering::Greater => paint.green(&format!("+{}", money(d, 2))),
        std::cmp::Ordering::Less => paint.red(&format!("-{}", money(-d, 2))),
        std::cmp::Ordering::Equal => paint.plain(&money(d, 2)),
    }
}
