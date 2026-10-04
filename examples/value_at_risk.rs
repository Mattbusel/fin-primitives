//! One-day and ten-day Value at Risk for a $1,000,000 book, three ways: historical,
//! parametric (normal) and Monte Carlo, at 95%, 97.5% and 99%.
//!
//! ```text
//! cargo run --example value_at_risk
//! ```
//!
//! The daily returns are a seeded random series (normal days with 1.2% volatility,
//! plus a 4.8% loss every 60th day), not market data, so the output is the same
//! every run.

use fin_primitives::normal;
use fin_primitives::risk::var_engine::VarEngine;

/// 750 seeded daily returns (about three years).
fn daily_returns() -> Vec<f64> {
    let mut state: u64 = 0x5EED_2026_1003;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f64 / (1u64 << 53) as f64
    };
    (0..750)
        .map(|i| {
            // Normal draw by inverting the CDF, plus a 4-sigma shock on every 60th day.
            let z = normal::inv_cdf(next().clamp(1e-12, 1.0 - 1e-12));
            let shock = if i % 60 == 59 { -4.0 * 0.012 } else { 0.0 };
            0.0004 + 0.012 * z + shock
        })
        .collect()
}

fn main() {
    let value = 1_000_000.0;
    let r = daily_returns();
    let n = r.len() as f64;
    let mean = r.iter().sum::<f64>() / n;
    let sd = (r.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0)).sqrt();
    println!("{} daily returns: mean {:.4}%, volatility {:.3}%\n", r.len(), mean * 100.0, sd * 100.0);

    println!("{:>10} {:>8} {:>14} {:>14} {:>14} {:>16}", "confidence", "z", "historical", "parametric", "monte carlo", "param. 10-day");
    for conf in [0.95, 0.975, 0.99] {
        let hist = VarEngine::historical_var(&r, conf, value);
        let para = VarEngine::parametric_var(mean, sd, conf, value, 1);
        let mc = VarEngine::monte_carlo_var(mean, sd, conf, value, 20_000, 7);
        let para10 = VarEngine::parametric_var(mean, sd, conf, value, 10);
        println!(
            "{:>9.1}% {:>8.4} {:>14.0} {:>14.0} {:>14.0} {:>16.0}",
            conf * 100.0,
            normal::inv_cdf(conf),
            hist.var_abs,
            para.var_abs,
            mc.var_abs,
            para10.var_abs
        );
    }

    let es = VarEngine::parametric_var(mean, sd, 0.975, value, 1);
    println!("\nExpected shortfall at 97.5% (parametric): {:.0}", es.cvar_abs);
    println!("Historical VaR is higher than the normal one at 99% because of the shock days:");
    println!("that gap is the fat tail a normal model cannot see.");
}
