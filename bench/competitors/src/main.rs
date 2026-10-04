//! Agreement check: run each library over the same closes and print the largest
//! difference from fin-primitives after a 1,000-bar warm-up.

use competitors::closes;
use fin_primitives::signals::indicators::{Ema, Rsi, Sma};
use fin_primitives::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;
use ta::Next;
use yata::core::Method;

fn fp_series(sig: &mut dyn Signal, xs: &[rust_decimal::Decimal]) -> Vec<Option<f64>> {
    xs.iter()
        .map(|&c| match sig.update(&BarInput::from_close(c)).unwrap() {
            SignalValue::Scalar(v) => v.to_f64(),
            SignalValue::Unavailable => None,
        })
        .collect()
}

fn max_diff(a: &[Option<f64>], b: &[f64], skip: usize) -> f64 {
    a.iter()
        .zip(b)
        .skip(skip)
        .filter_map(|(x, y)| x.map(|x| (x - y).abs()))
        .fold(0.0, f64::max)
}

fn main() {
    let n = 100_000;
    let (d, f) = closes(n);
    let skip = 1_000;

    let fp_sma = fp_series(&mut Sma::new("sma", 20).unwrap(), &d);
    let mut ta_sma = ta::indicators::SimpleMovingAverage::new(20).unwrap();
    let ta_sma: Vec<f64> = f.iter().map(|&x| ta_sma.next(x)).collect();
    let mut y_sma = yata::methods::SMA::new(20, &f[0]).unwrap();
    let y_sma: Vec<f64> = f.iter().map(|x| y_sma.next(x)).collect();
    println!("SMA(20)  max |fp - ta|   = {:.3e}", max_diff(&fp_sma, &ta_sma, skip));
    println!("SMA(20)  max |fp - yata| = {:.3e}", max_diff(&fp_sma, &y_sma, skip));

    let fp_ema = fp_series(&mut Ema::new("ema", 20).unwrap(), &d);
    let mut ta_ema = ta::indicators::ExponentialMovingAverage::new(20).unwrap();
    let ta_ema: Vec<f64> = f.iter().map(|&x| ta_ema.next(x)).collect();
    let mut y_ema = yata::methods::EMA::new(20, &f[0]).unwrap();
    let y_ema: Vec<f64> = f.iter().map(|x| y_ema.next(x)).collect();
    println!("EMA(20)  max |fp - ta|   = {:.3e}", max_diff(&fp_ema, &ta_ema, skip));
    println!("EMA(20)  max |fp - yata| = {:.3e}", max_diff(&fp_ema, &y_ema, skip));

    let fp_rsi = fp_series(&mut Rsi::new("rsi", 14).unwrap(), &d);
    let mut ta_rsi = ta::indicators::RelativeStrengthIndex::new(14).unwrap();
    let ta_rsi: Vec<f64> = f.iter().map(|&x| ta_rsi.next(x)).collect();
    println!("RSI(14)  max |fp - ta|   = {:.3e}", max_diff(&fp_rsi, &ta_rsi, skip));
    let last = n - 1;
    println!(
        "RSI(14)  last bar: fp {:.6}  ta {:.6}",
        fp_rsi[last].unwrap_or(f64::NAN),
        ta_rsi[last]
    );

    // Black-Scholes call, S=100 K=105 r=5% sigma=20% T=0.5
    use fin_primitives::options::greeks::{bsm_price_q, implied_vol, BSMInputs, OptionType};
    let inp = BSMInputs { S: 100.0, K: 105.0, r: 0.05, q: 0.0, sigma: 0.2, T: 0.5 };
    let fp_px = bsm_price_q(&inp, OptionType::Call);
    let bs_px = black_scholes::call(100.0, 105.0, 0.05, 0.2, 0.5);
    println!("BS call  fp {fp_px:.12}  black_scholes {bs_px:.12}  diff {:.3e}", (fp_px - bs_px).abs());
    let iv = implied_vol(fp_px, &BSMInputs { sigma: 0.5, ..inp }, OptionType::Call).unwrap();
    println!("IV round trip (fp): {iv:.12}");
}
