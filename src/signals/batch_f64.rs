//! Whole-series `f64` indicator kernels: one pass over a slice, one `Vec<f64>` out.
//!
//! ## Responsibility
//! The batch functions behind `fin_primitives.fast` in Python. Each takes a whole price
//! series as `&[f64]` and returns a vector of the same length, with `NaN` wherever the
//! indicator has no value. They follow pandas-ta-classic 0.9.0 (and so TA-Lib's seeding):
//! the same warm-up `NaN`s, the same seeds, and in most cases the same floating-point
//! operations in the same order, so the results agree with pandas-ta to the last bit or
//! within a few units in the last place.
//!
//! | function | pandas-ta-classic | first value at index |
//! |---|---|---|
//! | [`sma`] | `ta.sma(close, period)` | `period - 1` |
//! | [`ema`] | `ta.ema(close, period)` (SMA seed) | `period - 1` |
//! | [`rma`] | `ta.rma(close, period)` (Wilder, SMA seed) | `period - 1` |
//! | [`rsi`] | `ta.rsi(close, period)` | `period` |
//! | [`true_range`] | `ta.true_range(high, low, close)` | `1` |
//! | [`atr`] | `ta.atr(high, low, close, period)` (TA-Lib seed) | `period` |
//! | [`macd`] | `ta.macd(close, fast, slow, signal)` | `slow - 1`, signal `slow + signal - 2` |
//! | [`rolling_std`] | `ta.stdev(close, period, ddof)` | `period - 1` |
//! | [`bbands`] | `ta.bbands(close, period, std, ddof)` | `period - 1` |
//!
//! ## Guarantees
//! - O(n) time for every function, one output allocation per returned series
//! - Missing values (`NaN`) are handled the way pandas-ta handles them; see each function
//! - A `period` of 0, or an input shorter than the period, gives an all-`NaN` result
//!   (pandas-ta returns all `NaN` for short input too); nothing here panics
//!
//! ## NOT Responsible For
//! - Exact decimal arithmetic (see [`crate::signals::indicators`])
//! - Streaming, bar-at-a-time updates (see [`crate::signals::float`])

/// Sum the way numpy's `np.add.reduce` sums a contiguous float64 array (pairwise, with
/// eight accumulators per block of up to 128 values), so a seed mean matches numpy's bit
/// for bit.
pub fn numpy_sum(a: &[f64]) -> f64 {
    let n = a.len();
    if n < 8 {
        let mut res = -0.0;
        for &v in a {
            res += v;
        }
        res
    } else if n <= 128 {
        let mut r = [0.0f64; 8];
        r.copy_from_slice(&a[..8]);
        let full = n - n % 8;
        let mut i = 8;
        while i < full {
            for j in 0..8 {
                r[j] += a[i + j];
            }
            i += 8;
        }
        let mut res = ((r[0] + r[1]) + (r[2] + r[3])) + ((r[4] + r[5]) + (r[6] + r[7]));
        for &v in &a[full..] {
            res += v;
        }
        res
    } else {
        let mut n2 = n / 2;
        n2 -= n2 % 8;
        numpy_sum(&a[..n2]) + numpy_sum(&a[n2..])
    }
}

/// `numpy.mean` of a slice (`NaN` for an empty one).
fn numpy_mean(a: &[f64]) -> f64 {
    if a.is_empty() {
        return f64::NAN;
    }
    numpy_sum(a) / a.len() as f64
}

/// pandas `Series.mean()` of a slice: `NaN`s are skipped (summed as 0, not counted).
fn pandas_nanmean(a: &[f64]) -> f64 {
    let count = a.iter().filter(|v| !v.is_nan()).count();
    if count == 0 {
        return f64::NAN;
    }
    if count == a.len() {
        return numpy_sum(a) / count as f64;
    }
    let filled: Vec<f64> = a.iter().map(|&v| if v.is_nan() { 0.0 } else { v }).collect();
    numpy_sum(&filled) / count as f64
}

/// Simple moving average, as `close.rolling(period).mean()` in pandas (which pandas-ta's
/// `sma` calls).
///
/// The running sum is Kahan-compensated, with separate compensation terms for adding and
/// removing values, exactly as pandas does it, so drift does not build up over long
/// series and the results match pandas bit for bit. A window that holds a `NaN` reads
/// `NaN`.
pub fn sma(x: &[f64], period: usize) -> Vec<f64> {
    let n = x.len();
    let mut out = vec![f64::NAN; n];
    if period == 0 || n < period {
        return out;
    }
    let mut nobs: usize = 0;
    let mut neg_ct: usize = 0;
    let mut sum = 0.0f64;
    let mut comp_add = 0.0f64;
    let mut comp_rem = 0.0f64;
    let mut same: usize = 0;
    let mut prev = f64::NAN;
    for i in 0..n {
        if i >= period {
            let v = x[i - period];
            if !v.is_nan() {
                nobs -= 1;
                let y = -v - comp_rem;
                let t = sum + y;
                comp_rem = t - sum - y;
                sum = t;
                if v.is_sign_negative() {
                    neg_ct -= 1;
                }
            }
        }
        let v = x[i];
        if !v.is_nan() {
            nobs += 1;
            let y = v - comp_add;
            let t = sum + y;
            comp_add = t - sum - y;
            sum = t;
            if v.is_sign_negative() {
                neg_ct += 1;
            }
            if v == prev {
                same += 1;
            } else {
                same = 1;
            }
            prev = v;
        }
        if i + 1 >= period && nobs >= period {
            let mut r = sum / nobs as f64;
            if same >= nobs {
                r = prev;
            } else if (neg_ct == 0 && r < 0.0) || (neg_ct == nobs && r > 0.0) {
                r = 0.0;
            }
            out[i] = r;
        }
    }
    out
}

/// pandas' `ewm(alpha=..., adjust=False).mean()` recursion over `y`, written into `out`.
///
/// `alpha` is the effective smoothing factor pandas computes from its centre of mass.
/// A `NaN` input carries the average across the gap, giving the next value a larger
/// weight, as pandas does (`ignore_na=False`).
///
/// `first` is the first input value and `y[1..]` the rest (`y[0]` is not read), so a
/// caller can substitute a seed without copying the series.
fn pandas_ewm(first: f64, y: &[f64], alpha: f64, out: &mut [f64]) {
    let n = y.len();
    if n == 0 {
        return;
    }
    let factor = 1.0 - alpha;
    let new_wt = alpha;
    // With no gap the divisor is (1 - alpha) + alpha; dividing by an exact 1.0 is a no-op.
    let unit = factor + new_wt == 1.0;
    let mut weighted = first;
    out[0] = weighted;
    let mut old_wt = 1.0f64;
    for i in 1..n {
        let cur = y[i];
        let obs = !cur.is_nan();
        if !weighted.is_nan() {
            old_wt *= factor;
            if obs {
                if weighted != cur {
                    weighted = old_wt * weighted + new_wt * cur;
                    if !(unit && old_wt == factor) {
                        weighted /= old_wt + new_wt;
                    }
                }
                old_wt = 1.0;
            }
        } else if obs {
            weighted = cur;
        }
        out[i] = weighted;
    }
}

/// An SMA-seeded exponential average with pandas' `ewm` arithmetic: pandas-ta's `ema`
/// (`alpha` from the span) and `rma` (Wilder, `alpha` = 1 / period) share it.
fn seeded_ewm(x: &[f64], period: usize, alpha: f64) -> Vec<f64> {
    let n = x.len();
    let mut out = vec![f64::NAN; n];
    if period == 0 || n < period {
        return out;
    }
    let fv = match x.iter().position(|v| !v.is_nan()) {
        Some(p) => p,
        None => return out,
    };
    if fv + period > n {
        return out;
    }
    let seed_at = fv + period - 1;
    let seed = pandas_nanmean(&x[fv..fv + period]);
    // The recursion starts at the seed: everything before it reads NaN.
    pandas_ewm(seed, &x[seed_at..], alpha, &mut out[seed_at..]);
    // A missing bar has no value (the recursion still carries its state across it).
    for i in seed_at..n {
        if x[i].is_nan() {
            out[i] = f64::NAN;
        }
    }
    out
}

/// pandas' effective `alpha` for `ewm(span=period)`.
fn span_alpha(period: usize) -> f64 {
    let com = (period as f64 - 1.0) / 2.0;
    1.0 / (1.0 + com)
}

/// pandas' effective `alpha` for `ewm(alpha=1/period)`.
fn wilder_alpha(period: usize) -> f64 {
    let a = 1.0 / period as f64;
    let com = (1.0 - a) / a;
    1.0 / (1.0 + com)
}

/// Exponential moving average seeded with the SMA of the first `period` values, as
/// pandas-ta's `ema` (and TA-Lib's `EMA`): `alpha = 2 / (period + 1)`.
///
/// A leading `NaN` run is skipped (the seed is the mean of the first `period` values after
/// it); a `NaN` later in the series reads `NaN` and the average carries across it.
pub fn ema(x: &[f64], period: usize) -> Vec<f64> {
    if period == 0 {
        return vec![f64::NAN; x.len()];
    }
    seeded_ewm(x, period, span_alpha(period))
}

/// Wilder's moving average (RMA, SMMA) seeded with an SMA, as pandas-ta's `rma`:
/// `alpha = 1 / period`. `NaN` handling as in [`ema`].
pub fn rma(x: &[f64], period: usize) -> Vec<f64> {
    if period == 0 {
        return vec![f64::NAN; x.len()];
    }
    seeded_ewm(x, period, wilder_alpha(period))
}

/// Relative Strength Index with Wilder smoothing and TA-Lib's seeding, as pandas-ta's
/// `rsi`: the first value is at index `period`, from the mean gain and loss of the first
/// `period` changes. A window with no movement at all reads 0, as in TA-Lib.
pub fn rsi(x: &[f64], period: usize) -> Vec<f64> {
    let n = x.len();
    if period == 0 || n < period {
        return vec![f64::NAN; n];
    }
    let mut pos = vec![f64::NAN; n];
    let mut neg = vec![f64::NAN; n];
    for i in 1..n {
        let d = x[i] - x[i - 1];
        pos[i] = if d < 0.0 { 0.0 } else { d };
        neg[i] = if d > 0.0 { 0.0 } else { d };
    }
    let alpha = wilder_alpha(period);
    let p = seeded_ewm(&pos, period, alpha);
    let q = seeded_ewm(&neg, period, alpha);
    let mut out = p;
    for i in 0..n {
        let den = out[i] + q[i].abs();
        out[i] = if den == 0.0 { 0.0 } else { 100.0 * out[i] / den };
    }
    out
}

/// True range, as pandas-ta's `true_range`: `max(high - low, |high - prev close|,
/// |prev close - low|)`, `NaN` up to and including the first bar where high, low and
/// close all have a value (TA-Lib's `TRANGE` has no value there either).
///
/// With `first_bar_range`, that first bar reads `high - low` instead (TradingView's
/// `ta.tr(true)`).
pub fn true_range(high: &[f64], low: &[f64], close: &[f64], first_bar_range: bool) -> Vec<f64> {
    let n = close.len().min(high.len()).min(low.len());
    let mut out = vec![f64::NAN; close.len()];
    for i in 0..n {
        let hl = (high[i] - low[i]).abs();
        out[i] = if i == 0 {
            hl
        } else {
            let pc = close[i - 1];
            // f64::max skips NaN like numpy's fmax.
            hl.max((high[i] - pc).abs()).max((pc - low[i]).abs())
        };
    }
    let start = (0..n).find(|&i| !high[i].is_nan() && !low[i].is_nan() && !close[i].is_nan()).unwrap_or(n);
    let blank = if first_bar_range { start } else { start + 1 };
    for v in out.iter_mut().take(blank.min(close.len())) {
        *v = f64::NAN;
    }
    if first_bar_range && start < n {
        out[start] = (high[start] - low[start]).abs();
    }
    out
}

/// Where an ATR's Wilder average is seeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtrSeed {
    /// TA-Lib and pandas-ta: the first true range needs a previous close, so the seed is
    /// the mean of bars 1..=period and the first ATR is at index `period`.
    TaLib,
    /// TradingView (`ta.atr`): bar 0's true range is `high - low`, the seed is the mean of
    /// bars 0..period and the first ATR is at index `period - 1`.
    TradingView,
}

/// Average True Range: Wilder's average of the true range, as pandas-ta's `atr`
/// ([`AtrSeed::TaLib`]) or TradingView's `ta.atr` ([`AtrSeed::TradingView`]).
pub fn atr(high: &[f64], low: &[f64], close: &[f64], period: usize, seed: AtrSeed) -> Vec<f64> {
    let n = close.len();
    if period == 0 || n < period || high.len() != n || low.len() != n {
        return vec![f64::NAN; n];
    }
    let tr = true_range(high, low, close, seed == AtrSeed::TradingView);
    seeded_ewm(&tr, period, wilder_alpha(period))
}

/// TA-Lib's `EMA` over a gap-free series, seeded at `seed_end` with the mean of the
/// `period` values ending there (pandas-ta's `_ema_aligned`).
fn ema_aligned(a: &[f64], period: usize, seed_end: usize, out: &mut [f64]) {
    let m = a.len();
    if period == 0 || seed_end + 1 < period || seed_end >= m {
        return;
    }
    let k = 2.0 / (period as f64 + 1.0);
    let k1 = 1.0 - k;
    let mut prev = numpy_mean(&a[seed_end + 1 - period..=seed_end]);
    out[seed_end] = prev;
    for i in seed_end + 1..m {
        prev = k * a[i] + k1 * prev;
        out[i] = prev;
    }
}

/// MACD line, signal line and histogram, as pandas-ta's `macd` (and TA-Lib's `MACD`):
/// the fast EMA is seeded at `fast - 1`, the slow one at `slow - 1`, and the signal EMA at
/// `slow + signal - 2`, each from an SMA. Periods are swapped if `slow < fast`.
///
/// Rows where `close` is not finite are skipped and read `NaN` in all three outputs;
/// the averages run over the remaining rows, as pandas-ta does.
pub fn macd(close: &[f64], fast: usize, slow: usize, signal: usize) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let n = close.len();
    let (fast, slow) = if slow < fast { (slow, fast) } else { (fast, slow) };
    let nan = || vec![f64::NAN; n];
    if fast == 0 || signal == 0 {
        return (nan(), nan(), nan());
    }
    let all_finite = close.iter().all(|v| v.is_finite());
    let rows: Option<Vec<usize>> = if all_finite {
        None
    } else {
        Some((0..n).filter(|&i| close[i].is_finite()).collect())
    };
    let compact: Vec<f64>;
    let c: &[f64] = match &rows {
        None => close,
        Some(r) => {
            compact = r.iter().map(|&i| close[i]).collect();
            &compact
        }
    };
    let m = c.len();
    if m < fast.max(slow).max(signal) {
        return (nan(), nan(), nan());
    }
    let mut f = vec![f64::NAN; m];
    let mut s = vec![f64::NAN; m];
    ema_aligned(c, fast, fast - 1, &mut f);
    ema_aligned(c, slow, slow - 1, &mut s);
    let line: Vec<f64> = f.iter().zip(&s).map(|(a, b)| a - b).collect();
    let mut sig = vec![f64::NAN; m];
    ema_aligned(&line, signal, slow - 1 + signal - 1, &mut sig);
    let hist: Vec<f64> = line.iter().zip(&sig).map(|(a, b)| a - b).collect();
    match rows {
        None => (line, sig, hist),
        Some(r) => {
            let (mut l2, mut s2, mut h2) = (nan(), nan(), nan());
            for (j, &i) in r.iter().enumerate() {
                l2[i] = line[j];
                s2[i] = sig[j];
                h2[i] = hist[j];
            }
            (l2, s2, h2)
        }
    }
}

/// Rolling variance over `period` values with `ddof` delta degrees of freedom (pandas-ta's
/// `variance`, which is numpy's `var` on each window). A window holding a non-finite value
/// reads `NaN`; so does every window when `ddof >= period`.
///
/// One pass: the sum and sum of squares of the deviations from a reference value (the
/// window mean at the last rebuild). Both sums are recomputed from the window itself every
/// `period` bars, and early when the variance drops far below the level they were built
/// at, which keeps the relative error near 1e-13 or better however long the series is,
/// while costing O(1) per bar amortised. A window of equal values reads
/// exactly 0 (numpy leaves a residue of about 1e-17 there).
pub fn rolling_var(x: &[f64], period: usize, ddof: usize) -> Vec<f64> {
    let n = x.len();
    let mut out = vec![f64::NAN; n];
    if period == 0 || n < period || ddof >= period {
        return out;
    }
    let p = period as f64;
    let denom = (period - ddof) as f64;
    let mut bad: usize = 0;
    let mut fresh = false;
    let mut since: usize = 0;
    // k: reference value (the window mean at the last rebuild); s1, s2: sum and sum of
    // squares of the deviations from k; peak: the largest s2 since the rebuild, which
    // bounds the rounding error the running sums have picked up.
    let (mut k, mut s1, mut s2, mut peak) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let rebuild = |w: &[f64]| -> (f64, f64, f64) {
        let m = w.iter().sum::<f64>() / p;
        let (mut a, mut b) = (0.0f64, 0.0f64);
        for &v in w {
            let d = v - m;
            a += d;
            b += d * d;
        }
        (m, a, b)
    };
    // Length of the run of equal values ending at i: a window inside one is exactly flat.
    let mut run: usize = 0;
    for i in 0..n {
        if i > 0 && x[i] == x[i - 1] {
            run += 1;
        } else {
            run = 1;
        }
        if !x[i].is_finite() {
            bad += 1;
        }
        if i >= period && !x[i - period].is_finite() {
            bad -= 1;
        }
        if i + 1 < period {
            continue;
        }
        if bad > 0 {
            fresh = false;
            continue;
        }
        if run >= period {
            out[i] = 0.0;
            fresh = false;
            continue;
        }
        let mut ss = f64::NAN;
        if fresh && since < period {
            let a = x[i] - k;
            let r = x[i - period] - k;
            s1 += a - r;
            s2 += a * a - r * r;
            peak = peak.max(s2);
            since += 1;
            ss = s2 - s1 * s1 / p;
        }
        // Rebuild every `period` bars, and early when the variance has fallen far below the
        // level the running sums were built at (their absolute error would then dominate).
        if ss.is_nan() || ss * 64.0 <= peak {
            (k, s1, s2) = rebuild(&x[i + 1 - period..=i]);
            peak = s2;
            fresh = true;
            since = 0;
            ss = s2 - s1 * s1 / p;
        }
        out[i] = if ss > 0.0 { ss / denom } else { 0.0 };
    }
    out
}

/// Rolling standard deviation, the square root of [`rolling_var`] (pandas-ta's `stdev`;
/// `ddof = 0` is the population deviation pandas-ta and TA-Lib default to, `ddof = 1` the
/// sample deviation pandas' `rolling().std()` defaults to).
pub fn rolling_std(x: &[f64], period: usize, ddof: usize) -> Vec<f64> {
    let mut v = rolling_var(x, period, ddof);
    for e in v.iter_mut() {
        *e = e.sqrt();
    }
    v
}

/// Bollinger Bands `(lower, mid, upper)`: an SMA middle band plus and minus `k` rolling
/// standard deviations with `ddof` (pandas-ta's `bbands`, whose default is `ddof = 0`).
pub fn bbands(x: &[f64], period: usize, k: f64, ddof: usize) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let mid = sma(x, period);
    let sd = rolling_std(x, period, ddof);
    let n = x.len();
    let mut lower = Vec::with_capacity(n);
    let mut upper = Vec::with_capacity(n);
    for i in 0..n {
        let dev = k * sd[i];
        lower.push(mid[i] - dev);
        upper.push(mid[i] + dev);
    }
    (lower, mid, upper)
}

// ---------------------------------------------------------------------------------------
// Kernels behind the pandas-ta accessors (fin_primitives.pandas / fin_primitives.polars).
// Each follows the pandas or pandas-ta-classic 0.9.0 code it replaces, operation for
// operation, so the accessors give pandas-ta's numbers.
// ---------------------------------------------------------------------------------------

/// Rolling sum, as `x.rolling(period).sum()` in pandas: the same compensated running sum
/// as [`sma`], and pandas' rule that a window of one repeated value reads that value times
/// the count. A window that holds a `NaN` reads `NaN`.
pub fn rolling_sum(x: &[f64], period: usize) -> Vec<f64> {
    let n = x.len();
    let mut out = vec![f64::NAN; n];
    if period == 0 || n < period {
        return out;
    }
    let mut nobs: usize = 0;
    let mut sum = 0.0f64;
    let mut comp_add = 0.0f64;
    let mut comp_rem = 0.0f64;
    let mut same: usize = 0;
    let mut prev = f64::NAN;
    for i in 0..n {
        if i >= period {
            let v = x[i - period];
            if !v.is_nan() {
                nobs -= 1;
                let y = -v - comp_rem;
                let t = sum + y;
                comp_rem = t - sum - y;
                sum = t;
            }
        }
        let v = x[i];
        if !v.is_nan() {
            nobs += 1;
            let y = v - comp_add;
            let t = sum + y;
            comp_add = t - sum - y;
            sum = t;
            if v == prev {
                same += 1;
            } else {
                same = 1;
            }
            prev = v;
        }
        if i + 1 >= period && nobs >= period {
            out[i] = if same >= nobs { prev * nobs as f64 } else { sum };
        }
    }
    out
}

fn rolling_extreme(x: &[f64], period: usize, want_max: bool) -> Vec<f64> {
    let n = x.len();
    let mut out = vec![f64::NAN; n];
    if period == 0 || n < period {
        return out;
    }
    // Monotonic deque of indices; a window with a NaN in it has no value (pandas counts
    // only observed values against min_periods = period).
    let mut dq: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
    let mut nans = 0usize;
    for i in 0..n {
        if i >= period {
            let old = i - period;
            if x[old].is_nan() {
                nans -= 1;
            }
            if dq.front() == Some(&old) {
                dq.pop_front();
            }
        }
        let v = x[i];
        if v.is_nan() {
            nans += 1;
        } else {
            while let Some(&b) = dq.back() {
                let keep = if want_max { x[b] > v } else { x[b] < v };
                if keep {
                    break;
                }
                dq.pop_back();
            }
            dq.push_back(i);
        }
        if i + 1 >= period && nans == 0 {
            if let Some(&f) = dq.front() {
                out[i] = x[f];
            }
        }
    }
    out
}

/// Rolling maximum, as `x.rolling(period).max()` in pandas. O(n) with a monotonic deque.
pub fn rolling_max(x: &[f64], period: usize) -> Vec<f64> {
    rolling_extreme(x, period, true)
}

/// Rolling minimum, as `x.rolling(period).min()` in pandas.
pub fn rolling_min(x: &[f64], period: usize) -> Vec<f64> {
    rolling_extreme(x, period, false)
}

/// Linearly weighted moving average, weights `1..=period` with the newest bar heaviest,
/// as pandas-ta's `wma` (`np.dot` of each window with the weights, over their sum).
pub fn wma(x: &[f64], period: usize) -> Vec<f64> {
    let n = x.len();
    let mut out = vec![f64::NAN; n];
    if period == 0 || n < period {
        return out;
    }
    let total = 0.5 * period as f64 * (period as f64 + 1.0);
    for i in period - 1..n {
        let w = &x[i + 1 - period..=i];
        if w.iter().any(|v| v.is_nan()) {
            continue;
        }
        let mut dot = 0.0f64;
        for (j, v) in w.iter().enumerate() {
            dot += (j + 1) as f64 * v;
        }
        out[i] = dot / total;
    }
    out
}

/// Bars since the most recent highest (`want_max`) or lowest value in each window of
/// `length` bars, as `x.rolling(length).apply(lambda w: np.argmax(w[::-1]))` (pandas-ta's
/// `aroon`). numpy's rules carry over: ties go to the most recent bar, and a `NaN` in the
/// window counts as the extreme.
pub fn bars_since_extreme(x: &[f64], length: usize, want_max: bool) -> Vec<f64> {
    let n = x.len();
    let mut out = vec![f64::NAN; n];
    if length == 0 || n < length {
        return out;
    }
    for i in length - 1..n {
        let mut best = x[i];
        let mut best_k = 0usize;
        if !best.is_nan() {
            for k in 1..length {
                let v = x[i - k];
                if v.is_nan() {
                    best_k = k;
                    break;
                }
                if (want_max && v > best) || (!want_max && v < best) {
                    best = v;
                    best_k = k;
                }
            }
        }
        out[i] = best_k as f64;
    }
    out
}

/// Wilder's running sum (TA-Lib's smoothing of +DM, -DM and the true range), as
/// pandas-ta-classic's `wilder_smooth`: seeded with the sum of the first `length - 1`
/// values after bar 0, then `s = s - s / length + x`. A missing bar carries the sum and
/// reads `NaN`.
pub fn wilder_sum(x: &[f64], length: usize) -> Vec<f64> {
    let n = x.len();
    let mut out = vec![f64::NAN; n];
    if length < 2 {
        return out;
    }
    let first = match x.iter().position(|v| v.is_finite()) {
        Some(p) => p,
        None => return out,
    };
    let start = first.max(1);
    if start + length - 1 > n {
        return out;
    }
    let filled: Vec<f64> = x[start..start + length - 1].iter().map(|&v| if v.is_nan() { 0.0 } else { v }).collect();
    let mut value = numpy_sum(&filled);
    out[start + length - 2] = value;
    let len_f = length as f64;
    for i in start + length - 1..n {
        let raw = x[i];
        if !raw.is_nan() {
            value = value - value / len_f + raw;
        }
        out[i] = value;
    }
    for (i, v) in out.iter_mut().enumerate().skip(start) {
        if !x[i].is_finite() {
            *v = f64::NAN;
        }
    }
    out
}

/// Python's `max(a, b)`: `b` only when `b > a`, so a `NaN` first argument wins.
fn py_max(a: f64, b: f64) -> f64 {
    if b > a {
        b
    } else {
        a
    }
}

/// Python's `min(a, b)`: `b` only when `b < a`.
fn py_min(a: f64, b: f64) -> f64 {
    if b < a {
        b
    } else {
        a
    }
}

/// The four outputs of [`psar_loop`] and [`supertrend_loop`].
pub type Four = (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>);

/// Parabolic SAR recursion of pandas-ta-classic's `psar`, from its initial state:
/// `(long, short, af, reversal)`.
pub fn psar_loop(high: &[f64], low: &[f64], falling: bool, sar: f64, ep: f64, af0: f64, max_af: f64) -> Four {
    let (mut falling, mut sar, mut ep) = (falling, sar, ep);
    let m = high.len().min(low.len());
    let mut long = vec![f64::NAN; m];
    let mut short = vec![f64::NAN; m];
    let mut af_arr = vec![f64::NAN; m];
    let mut rev = vec![0.0f64; m];
    if m == 0 {
        return (long, short, af_arr, rev);
    }
    af_arr[0] = af0;
    let mut af = af0;
    for row in 1..m {
        let h = high[row];
        let l = low[row];
        let back2 = row.saturating_sub(2);
        let mut s = sar + af * (ep - sar);
        let reverse;
        if falling {
            if l < ep {
                ep = l;
                af = py_min(af + af0, max_af);
            }
            s = py_max(py_max(high[row - 1], high[back2]), s);
            reverse = h > s;
        } else {
            if h > ep {
                ep = h;
                af = py_min(af + af0, max_af);
            }
            s = py_min(py_min(low[row - 1], low[back2]), s);
            reverse = l < s;
        }
        if reverse {
            s = ep;
            af = af0;
            falling = !falling;
            ep = if falling { l } else { h };
        }
        sar = s;
        if falling {
            short[row] = sar;
        } else {
            long[row] = sar;
        }
        af_arr[row] = af;
        rev[row] = if reverse { 1.0 } else { 0.0 };
    }
    (long, short, af_arr, rev)
}

/// Supertrend band recursion of pandas-ta-classic's `supertrend`, from the basic bands
/// `hl2 -+ multiplier * ATR`: `(direction, trend, long, short)`.
pub fn supertrend_loop(close: &[f64], upper: &[f64], lower: &[f64]) -> Four {
    let m = close.len().min(upper.len()).min(lower.len());
    let mut dir = vec![f64::NAN; m];
    let mut trend = vec![f64::NAN; m];
    let mut long = vec![f64::NAN; m];
    let mut short = vec![f64::NAN; m];
    let mut fub = vec![f64::NAN; m];
    let mut flb = vec![f64::NAN; m];
    for i in 1..m {
        let direction;
        if fub[i - 1].is_nan() || flb[i - 1].is_nan() {
            fub[i] = upper[i];
            flb[i] = lower[i];
            direction = 1.0;
        } else {
            fub[i] = if upper[i] < fub[i - 1] || close[i - 1] > fub[i - 1] { upper[i] } else { fub[i - 1] };
            flb[i] = if lower[i] > flb[i - 1] || close[i - 1] < flb[i - 1] { lower[i] } else { flb[i - 1] };
            direction = if close[i] > fub[i - 1] {
                1.0
            } else if close[i] < flb[i - 1] {
                -1.0
            } else {
                dir[i - 1]
            };
        }
        if fub[i].is_nan() || flb[i].is_nan() {
            continue;
        }
        dir[i] = direction;
        if direction > 0.0 {
            trend[i] = flb[i];
            long[i] = flb[i];
        } else {
            trend[i] = fub[i];
            short[i] = fub[i];
        }
    }
    (dir, trend, long, short)
}

/// Output of [`qqe_loop`].
pub struct Qqe {
    /// Trailing long band.
    pub long: Vec<f64>,
    /// Trailing short band.
    pub short: Vec<f64>,
    /// Trend: 1 long, -1 short.
    pub trend: Vec<f64>,
    /// The QQE line.
    pub qqe: Vec<f64>,
    /// The QQE line while long.
    pub qqe_long: Vec<f64>,
    /// The QQE line while short.
    pub qqe_short: Vec<f64>,
}

/// Trailing-band recursion of pandas-ta-classic's `qqe` over the smoothed RSI and its
/// upper and lower bands.
pub fn qqe_loop(rsi_ma: &[f64], upper: &[f64], lower: &[f64]) -> Qqe {
    let m = rsi_ma.len().min(upper.len()).min(lower.len());
    let mut long = vec![f64::NAN; m];
    let mut short = vec![f64::NAN; m];
    let mut trend = vec![1.0f64; m];
    let mut qqe = vec![f64::NAN; m];
    let mut ql = vec![f64::NAN; m];
    let mut qs = vec![f64::NAN; m];
    if m > 0 {
        qqe[0] = rsi_ma[0];
    }
    for i in 1..m {
        let c_rsi = rsi_ma[i];
        let p_rsi = rsi_ma[i - 1];
        let c_long = long[i - 1];
        let c_short = short[i - 1];
        let p_long = if i >= 2 { long[i - 2] } else { 0.0 };
        let p_short = if i >= 2 { short[i - 2] } else { 0.0 };
        long[i] = if p_rsi > c_long && c_rsi > c_long { py_max(c_long, lower[i]) } else { lower[i] };
        short[i] = if p_rsi < c_short && c_rsi < c_short { py_min(c_short, upper[i]) } else { upper[i] };
        if (c_rsi > c_short && p_rsi < p_short) || (c_rsi <= c_short && p_rsi >= p_short) {
            trend[i] = 1.0;
            qqe[i] = long[i];
            ql[i] = long[i];
        } else if (c_rsi > c_long && p_rsi < p_long) || (c_rsi <= c_long && p_rsi >= p_long) {
            trend[i] = -1.0;
            qqe[i] = short[i];
            qs[i] = short[i];
        } else {
            trend[i] = trend[i - 1];
            if trend[i] == 1.0 {
                qqe[i] = long[i];
                ql[i] = long[i];
            } else {
                qqe[i] = short[i];
                qs[i] = short[i];
            }
        }
    }
    Qqe { long, short, trend, qqe, qqe_long: ql, qqe_short: qs }
}

/// Chaikin A/D oscillator recursion of pandas-ta-classic's `adosc`: two EMAs of the A/D
/// line started at its first value, their difference from bar `slow - 1` on.
pub fn adosc_loop(ad: &[f64], fast: usize, slow: usize) -> Vec<f64> {
    let m = ad.len();
    let mut out = vec![f64::NAN; m];
    if m == 0 || fast == 0 || slow == 0 {
        return out;
    }
    let fk = 2.0 / (fast as f64 + 1.0);
    let sk = 2.0 / (slow as f64 + 1.0);
    let mut fe = ad[0];
    let mut se = ad[0];
    for i in 1..m {
        fe = fk * ad[i] + (1.0 - fk) * fe;
        se = sk * ad[i] + (1.0 - sk) * se;
        if i + 1 >= slow {
            out[i] = fe - se;
        }
    }
    out
}

/// KAMA recursion of pandas-ta-classic's `kama`: seeded with the close at bar `length - 1`,
/// then `k[i] = sc[i] * close[i] + (1 - sc[i]) * k[i - 1]`, where `sc` is the squared
/// smoothing constant from the efficiency ratio.
pub fn kama_loop(sc: &[f64], close: &[f64], length: usize) -> Vec<f64> {
    let m = close.len().min(sc.len());
    let mut out = vec![f64::NAN; close.len()];
    if length == 0 || length > m {
        return out;
    }
    out[length - 1] = close[length - 1];
    for i in length..m {
        out[i] = sc[i] * close[i] + (1.0 - sc[i]) * out[i - 1];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kama_loop_seeds_at_length_and_follows_the_recursion() {
        let close = [1.0, 2.0, 3.0, 4.0, 5.0];
        let sc = [0.5; 5];
        let k = kama_loop(&sc, &close, 3);
        assert!(k[0].is_nan() && k[1].is_nan());
        assert_eq!(k[2], 3.0);
        assert_eq!(k[3], 0.5 * 4.0 + 0.5 * 3.0);
        assert_eq!(k[4], 0.5 * 5.0 + 0.5 * k[3]);
        assert!(kama_loop(&sc, &close, 6).iter().all(|v| v.is_nan()));
        assert!(kama_loop(&sc, &close, 0).iter().all(|v| v.is_nan()));
    }

    fn close_enough(a: f64, b: f64) -> bool {
        (a.is_nan() && b.is_nan()) || (a - b).abs() <= 1e-12 * b.abs().max(1.0)
    }

    fn walk(n: usize) -> Vec<f64> {
        // Deterministic pseudo-random walk (xorshift), no extra dev-dependency.
        let mut s: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut p = 100.0;
        (0..n)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                let u = (s >> 11) as f64 / (1u64 << 53) as f64 - 0.5;
                p *= 1.0 + 0.02 * u;
                p
            })
            .collect()
    }

    #[test]
    fn numpy_sum_matches_naive_on_small_inputs() {
        assert_eq!(numpy_sum(&[]), 0.0);
        assert_eq!(numpy_sum(&[1.0, 2.0, 3.0]), 6.0);
        let v: Vec<f64> = (1..=300).map(|i| i as f64).collect();
        assert_eq!(numpy_sum(&v), 45150.0);
    }

    #[test]
    fn sma_matches_naive() {
        let x = walk(500);
        let out = sma(&x, 20);
        for i in 0..19 {
            assert!(out[i].is_nan());
        }
        for i in 19..x.len() {
            let naive: f64 = x[i - 19..=i].iter().sum::<f64>() / 20.0;
            assert!(close_enough(out[i], naive), "{i}: {} vs {naive}", out[i]);
        }
    }

    #[test]
    fn sma_nan_window_is_nan() {
        let mut x = walk(50);
        x[30] = f64::NAN;
        let out = sma(&x, 5);
        for i in 30..35 {
            assert!(out[i].is_nan());
        }
        assert!(!out[35].is_nan());
        assert!(!out[29].is_nan());
    }

    #[test]
    fn ema_matches_textbook_recursion() {
        let x = walk(300);
        let out = ema(&x, 10);
        let k = 2.0 / 11.0;
        let mut prev = x[..10].iter().sum::<f64>() / 10.0;
        assert!(out[8].is_nan());
        assert!(close_enough(out[9], prev));
        for i in 10..x.len() {
            prev = k * x[i] + (1.0 - k) * prev;
            assert!(close_enough(out[i], prev));
        }
    }

    #[test]
    fn rsi_matches_wilder_textbook() {
        let x = walk(400);
        let out = rsi(&x, 14);
        let (mut g, mut l) = (0.0, 0.0);
        for i in 1..=14 {
            let d = x[i] - x[i - 1];
            if d > 0.0 { g += d } else { l -= d }
        }
        g /= 14.0;
        l /= 14.0;
        assert!(out[13].is_nan());
        assert!(close_enough(out[14], 100.0 * g / (g + l)));
        for i in 15..x.len() {
            let d = x[i] - x[i - 1];
            g = (g * 13.0 + d.max(0.0)) / 14.0;
            l = (l * 13.0 + (-d).max(0.0)) / 14.0;
            assert!((out[i] - 100.0 * g / (g + l)).abs() < 1e-9);
        }
    }

    #[test]
    fn flat_series() {
        let x = vec![5.0; 40];
        assert!(rsi(&x, 14)[20] == 0.0);
        assert!(rolling_std(&x, 10, 0)[20] == 0.0);
        assert!(sma(&x, 10)[20] == 5.0);
    }

    #[test]
    fn atr_seeds() {
        let c = walk(200);
        let h: Vec<f64> = c.iter().map(|v| v * 1.01).collect();
        let l: Vec<f64> = c.iter().map(|v| v * 0.99).collect();
        let a = atr(&h, &l, &c, 14, AtrSeed::TaLib);
        assert!(a[13].is_nan() && !a[14].is_nan());
        let b = atr(&h, &l, &c, 14, AtrSeed::TradingView);
        assert!(b[12].is_nan() && !b[13].is_nan());
        let tr0 = h[0] - l[0];
        let mut seed = tr0;
        for i in 1..14 {
            seed += (h[i] - l[i]).max((h[i] - c[i - 1]).abs()).max((c[i - 1] - l[i]).abs());
        }
        assert!(close_enough(b[13], seed / 14.0));
    }

    #[test]
    fn macd_warmup_and_identity() {
        let x = walk(300);
        let (line, sig, hist) = macd(&x, 12, 26, 9);
        assert!(line[24].is_nan() && !line[25].is_nan());
        assert!(sig[32].is_nan() && !sig[33].is_nan());
        for i in 33..x.len() {
            assert_eq!(hist[i], line[i] - sig[i]);
        }
    }

    #[test]
    fn rolling_std_matches_two_pass() {
        let x = walk(1000);
        for ddof in [0usize, 1] {
            let out = rolling_std(&x, 20, ddof);
            for i in 19..x.len() {
                let w = &x[i - 19..=i];
                let m = w.iter().sum::<f64>() / 20.0;
                let v = w.iter().map(|a| (a - m) * (a - m)).sum::<f64>() / (20 - ddof) as f64;
                assert!((out[i] - v.sqrt()).abs() <= 1e-12 * v.sqrt(), "{i}");
            }
        }
    }

    #[test]
    fn short_and_degenerate_inputs() {
        let x = walk(5);
        assert!(sma(&x, 10).iter().all(|v| v.is_nan()));
        assert!(ema(&x, 10).iter().all(|v| v.is_nan()));
        assert!(rsi(&x, 5).iter().all(|v| v.is_nan()));
        assert!(sma(&x, 0).iter().all(|v| v.is_nan()));
        assert!(rolling_std(&x, 3, 3).iter().all(|v| v.is_nan()));
        assert!(sma(&[], 3).is_empty());
        let (a, b, c) = macd(&x, 12, 26, 9);
        assert!(a.iter().chain(&b).chain(&c).all(|v| v.is_nan()));
    }
    #[test]
    fn rolling_kernels_match_naive_windows() {
        let mut x = walk(500);
        x[200] = f64::NAN;
        for p in [1usize, 3, 14] {
            let (sum, mx, mn, w) = (rolling_sum(&x, p), rolling_max(&x, p), rolling_min(&x, p), wma(&x, p));
            for i in 0..x.len() {
                if i + 1 < p || (i >= 200 && i < 200 + p) {
                    assert!(sum[i].is_nan() && mx[i].is_nan() && mn[i].is_nan() && w[i].is_nan(), "{p} {i}");
                    continue;
                }
                let win = &x[i + 1 - p..=i];
                let s: f64 = win.iter().sum();
                let total = (p * (p + 1)) as f64 / 2.0;
                let ws: f64 = win.iter().enumerate().map(|(j, v)| (j + 1) as f64 * v).sum::<f64>() / total;
                assert!(close_enough(sum[i], s), "{p} {i}");
                assert_eq!(mx[i], win.iter().cloned().fold(f64::MIN, f64::max));
                assert_eq!(mn[i], win.iter().cloned().fold(f64::MAX, f64::min));
                assert!(close_enough(w[i], ws), "{p} {i}");
            }
        }
    }

    #[test]
    fn bars_since_extreme_prefers_the_latest_tie() {
        let x = [1.0, 3.0, 2.0, 3.0, 0.0];
        let hi = bars_since_extreme(&x, 3, true);
        assert!(hi[1].is_nan());
        assert_eq!(&hi[2..], &[1.0, 0.0, 1.0]);
        let lo = bars_since_extreme(&x, 3, false);
        assert_eq!(&lo[2..], &[2.0, 1.0, 0.0]);
    }

    #[test]
    fn recursions_have_the_input_length() {
        let c = walk(300);
        let h: Vec<f64> = c.iter().map(|v| v + 0.5).collect();
        let l: Vec<f64> = c.iter().map(|v| v - 0.5).collect();
        let (a, b, af, r) = psar_loop(&h, &l, false, l[0], h[1], 0.02, 0.2);
        assert!(a.len() == 300 && b.len() == 300 && af.len() == 300 && r.len() == 300);
        assert!(a.iter().zip(&b).skip(1).all(|(x, y)| x.is_nan() != y.is_nan()));
        assert!(af.iter().all(|v| (0.02..=0.2 + 1e-12).contains(v)));
        let (d, t, _, _) = supertrend_loop(&c, &h, &l);
        assert!(d[0].is_nan() && t.len() == 300 && d[1..].iter().all(|v| *v == 1.0 || *v == -1.0));
        let q = qqe_loop(&c, &h, &l);
        assert_eq!(q.qqe.len(), 300);
        let ws = wilder_sum(&c, 14);
        assert!(ws[12].is_nan() && !ws[13].is_nan());
        let ad = adosc_loop(&c, 3, 10);
        assert!(ad[8].is_nan() && !ad[9].is_nan());
    }
}
