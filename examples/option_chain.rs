//! Price a small European option chain with Black-Scholes, print the Greeks
//! side by side, then recover the input volatility from one price with the
//! implied-volatility solver.
//!
//! ```text
//! cargo run --example option_chain
//! ```
//!
//! Inputs are illustrative (spot 212.40, 30 days, 4.5% rate, 28% vol).

use fin_primitives::greeks::{BlackScholes, OptionSpec, OptionType};
use fin_primitives::FinError;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

mod support;
use support::{fixed, Paint};

fn main() -> Result<(), FinError> {
    let paint = Paint::detect();
    let spot = dec!(212.40);
    let spec = |strike: Decimal, option_type: OptionType| OptionSpec {
        strike,
        expiry_days: 30,
        spot,
        risk_free_rate: dec!(0.045),
        volatility: dec!(0.28),
        option_type,
    };

    println!();
    println!(
        "  {}  spot {}  30 days  r 4.50%  vol 28.00%",
        paint.bold("chain"),
        fixed(spot, 2)
    );
    println!();
    println!(
        "  {:>7} {:>7} {:>7}  {:^8}  {:>7} {:>7} {:>7}  {:>7} {:>6}",
        paint.dim("theta"),
        paint.dim("delta"),
        paint.green("call"),
        paint.bold("strike"),
        paint.red("put"),
        paint.dim("delta"),
        paint.dim("theta"),
        paint.dim("gamma"),
        paint.dim("vega")
    );

    let mut strike = dec!(195);
    while strike <= dec!(230) {
        let call = spec(strike, OptionType::Call);
        let put = spec(strike, OptionType::Put);
        let (cp, cg) = (BlackScholes::price(&call)?, BlackScholes::greeks(&call)?);
        let (pp, pg) = (BlackScholes::price(&put)?, BlackScholes::greeks(&put)?);
        // The strike nearest spot is the at-the-money row.
        let atm = (strike - spot).abs() < dec!(2.5);
        let k = fixed(strike, 2);
        println!(
            "  {:>7} {:>7} {:>7}  {:^8}  {:>7} {:>7} {:>7}  {:>7} {:>6}",
            fixed(cg.theta, 3),
            fixed(cg.delta, 3),
            paint.green(&fixed(cp, 2)),
            if atm {
                paint.bold(&format!(" {k}*"))
            } else {
                paint.plain(&format!(" {k} "))
            },
            paint.red(&fixed(pp, 2)),
            fixed(pg.delta, 3),
            fixed(pg.theta, 3),
            fixed(cg.gamma, 4),
            fixed(cg.vega, 3)
        );
        strike += dec!(5);
    }
    println!();
    println!(
        "  {}",
        paint.dim("* nearest the money. theta is per calendar day, vega per 1 vol point.")
    );

    // Round trip: price at 28% vol, then solve for the vol that reproduces that price.
    let atm_call = spec(dec!(210), OptionType::Call);
    let price = BlackScholes::price(&atm_call)?;
    let iv = BlackScholes::implied_vol(price, &atm_call)?;
    println!();
    println!(
        "  {}  210 call priced at {} recovers vol {}%",
        paint.dim("implied vol"),
        fixed(price, 4),
        paint.bold(&fixed(iv * dec!(100), 4))
    );
    Ok(())
}
