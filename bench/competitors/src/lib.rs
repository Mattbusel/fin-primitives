//! Shared inputs for the benchmarks and the agreement check.

use rust_decimal::Decimal;

/// `n` closing prices from a seeded random walk, rounded to cents, as both
/// `Decimal` and `f64` holding the same values.
pub fn closes(n: usize) -> (Vec<Decimal>, Vec<f64>) {
    let mut state: u64 = 20_261_003;
    let mut cents: i64 = 10_000; // 100.00
    let mut dec = Vec::with_capacity(n);
    let mut flt = Vec::with_capacity(n);
    for _ in 0..n {
        // xorshift64*
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        let r = state.wrapping_mul(0x2545_F491_4F6C_DD1D);
        let step = (r % 201) as i64 - 100; // -1.00 .. +1.00
        cents = (cents + step).max(100);
        dec.push(Decimal::new(cents, 2));
        flt.push(cents as f64 / 100.0);
    }
    (dec, flt)
}
