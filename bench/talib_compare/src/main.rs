//! Runs fin-primitives indicators over data/bars.csv, one bar at a time, and
//! writes every output to data/fp_values.csv and per-bar timings to
//! data/fp_timing.csv. compare.py reads both.
//!
//! Usage: cargo run --release -- [data_dir] [repeats]

use fin_primitives::signals::indicators::{
    Adx, Aroon, Atr, BollingerB, Cci, Dema, Ema, Kama, Macd, Mfi, Momentum, Natr, Obv, Roc, Rsi,
    Sma, StochasticD, StochasticK, Tema, Trix, WilliamsR, Wma,
};
use fin_primitives::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::fs;
use std::hint::black_box;
use std::io::Write;
use std::str::FromStr;
use std::time::Instant;

type Make = fn() -> Box<dyn Signal>;

/// (column name, constructor). Parameters match compare.py.
fn registry() -> Vec<(&'static str, Make)> {
    vec![
        ("SMA_20", || Box::new(Sma::new("sma", 20).unwrap())),
        ("EMA_20", || Box::new(Ema::new("ema", 20).unwrap())),
        ("WMA_20", || Box::new(Wma::new("wma", 20).unwrap())),
        ("DEMA_20", || Box::new(Dema::new("dema", 20).unwrap())),
        ("TEMA_20", || Box::new(Tema::new("tema", 20).unwrap())),
        ("TRIX_15", || Box::new(Trix::new("trix", 15).unwrap())),
        ("KAMA_10", || Box::new(Kama::new("kama", 10).unwrap())),
        ("RSI_14", || Box::new(Rsi::new("rsi", 14).unwrap())),
        ("MACDHIST_12_26_9", || Box::new(Macd::new("macd", 12, 26, 9).unwrap())),
        ("BBANDS_PCTB_20_2", || Box::new(BollingerB::new("bb", 20, 2).unwrap())),
        ("ATR_14", || Box::new(Atr::new("atr", 14).unwrap())),
        ("NATR_14", || Box::new(Natr::new("natr", 14).unwrap())),
        ("STOCHF_K_14", || Box::new(StochasticK::new("k", 14).unwrap())),
        ("STOCHF_D_14_3", || Box::new(StochasticD::new("d", 14, 3).unwrap())),
        ("WILLR_14", || Box::new(WilliamsR::new("wr", 14).unwrap())),
        ("CCI_20", || Box::new(Cci::new("cci", 20).unwrap())),
        ("ROC_10", || Box::new(Roc::new("roc", 10).unwrap())),
        ("MOM_10", || Box::new(Momentum::new("mom", 10).unwrap())),
        ("MFI_14", || Box::new(Mfi::new("mfi", 14).unwrap())),
        ("ADX_14", || Box::new(Adx::new("adx", 14).unwrap())),
        ("AROONOSC_14", || Box::new(Aroon::new("aroon", 14).unwrap())),
        ("OBV", || Box::new(Obv::new("obv"))),
    ]
}

fn load(path: &str) -> Vec<BarInput> {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    text.lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let f: Vec<Decimal> = l.split(',').map(|x| Decimal::from_str(x.trim()).unwrap()).collect();
            // BarInput::new(close, high, low, open, volume)
            BarInput::new(f[3], f[1], f[2], f[0], f[4])
        })
        .collect()
}

fn fmt(v: Option<Decimal>) -> String {
    v.map(|d| d.to_string()).unwrap_or_default()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = args.get(1).cloned().unwrap_or_else(|| "data".to_string());
    let repeats: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(5);
    let bars = load(&format!("{dir}/bars.csv"));
    let n = bars.len();
    eprintln!("loaded {n} bars");

    // ---- values ----------------------------------------------------------
    let reg = registry();
    let mut cols: Vec<(String, Vec<String>)> = Vec::new();
    for (name, make) in &reg {
        let mut ind = make();
        let v: Vec<String> = bars
            .iter()
            .map(|b| match ind.update(b).unwrap() {
                SignalValue::Scalar(d) => d.to_string(),
                SignalValue::Unavailable => String::new(),
            })
            .collect();
        cols.push(((*name).to_string(), v));
    }
    // Multi-output extras: Bollinger bands and Aroon up/down.
    {
        let mut bb = BollingerB::new("bb", 20, 2).unwrap();
        let (mut up, mut mid, mut lo) = (Vec::new(), Vec::new(), Vec::new());
        for b in &bars {
            bb.update(b).unwrap();
            let bands = bb.bands();
            up.push(fmt(bands.map(|x| x.0)));
            mid.push(fmt(bands.map(|x| x.1)));
            lo.push(fmt(bands.map(|x| x.2)));
        }
        cols.push(("BBANDS_UPPER_20_2".into(), up));
        cols.push(("BBANDS_MIDDLE_20_2".into(), mid));
        cols.push(("BBANDS_LOWER_20_2".into(), lo));

        let mut ar = Aroon::new("aroon", 14).unwrap();
        let (mut u, mut d) = (Vec::new(), Vec::new());
        for b in &bars {
            ar.update(b).unwrap();
            u.push(fmt(ar.up()));
            d.push(fmt(ar.down()));
        }
        cols.push(("AROON_UP_14".into(), u));
        cols.push(("AROON_DOWN_14".into(), d));
    }
    let mut out = fs::File::create(format!("{dir}/fp_values.csv")).unwrap();
    let header: Vec<&str> = cols.iter().map(|(c, _)| c.as_str()).collect();
    writeln!(out, "{}", header.join(",")).unwrap();
    for i in 0..n {
        let row: Vec<&str> = cols.iter().map(|(_, v)| v[i].as_str()).collect();
        writeln!(out, "{}", row.join(",")).unwrap();
    }

    // ---- timing: streaming, one update() per bar, best of `repeats` -------
    let mut t = fs::File::create(format!("{dir}/fp_timing.csv")).unwrap();
    writeln!(t, "indicator,bars,best_ns_per_bar").unwrap();
    for (name, make) in &reg {
        let mut best = f64::INFINITY;
        for _ in 0..repeats {
            let mut ind = make();
            let start = Instant::now();
            for b in &bars {
                black_box(ind.update(black_box(b)).unwrap());
            }
            let ns = start.elapsed().as_nanos() as f64 / n as f64;
            best = best.min(ns);
        }
        writeln!(t, "{name},{n},{best:.1}").unwrap();
        eprintln!("{name:<20} {best:>9.1} ns/bar");
    }
}
