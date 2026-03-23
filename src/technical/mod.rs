//! Technical analysis indicators for OHLCV price series.
//!
//! All functions return `Vec<f64>` of the same length as the input.
//! Values before the warm-up period are filled with `0.0`.

// ─────────────────────────────────────────────────────────────────────────────
// OHLCV bar
// ─────────────────────────────────────────────────────────────────────────────

/// A single OHLCV bar.
#[derive(Debug, Clone, PartialEq)]
pub struct Ohlcv {
    /// Opening price.
    pub open: f64,
    /// High price.
    pub high: f64,
    /// Low price.
    pub low: f64,
    /// Closing price.
    pub close: f64,
    /// Traded volume.
    pub volume: f64,
    /// Bar open time in milliseconds since the Unix epoch.
    pub timestamp_ms: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Simple Moving Average
// ─────────────────────────────────────────────────────────────────────────────

/// Simple moving average over `period` bars.
///
/// The first `period - 1` values are 0.0.
pub fn sma(prices: &[f64], period: usize) -> Vec<f64> {
    let n = prices.len();
    if period == 0 || n == 0 {
        return vec![0.0; n];
    }
    let mut out = vec![0.0; n];
    let mut window_sum = 0.0;
    for (i, &p) in prices.iter().enumerate() {
        window_sum += p;
        if i >= period {
            window_sum -= prices[i - period];
        }
        if i + 1 >= period {
            out[i] = window_sum / period as f64;
        }
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// Exponential Moving Average
// ─────────────────────────────────────────────────────────────────────────────

/// Exponential moving average with smoothing factor α = 2 / (period + 1).
///
/// The first `period - 1` values are 0.0; the seed value at index `period - 1`
/// is the simple average of the first `period` prices.
pub fn ema(prices: &[f64], period: usize) -> Vec<f64> {
    let n = prices.len();
    if period == 0 || n == 0 {
        return vec![0.0; n];
    }
    let mut out = vec![0.0; n];
    if period > n {
        return out;
    }
    let alpha = 2.0 / (period as f64 + 1.0);
    // Seed: SMA of first `period` prices.
    let seed: f64 = prices[..period].iter().sum::<f64>() / period as f64;
    out[period - 1] = seed;
    let mut prev = seed;
    for i in period..n {
        let e = alpha * prices[i] + (1.0 - alpha) * prev;
        out[i] = e;
        prev = e;
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// Relative Strength Index (Wilder)
// ─────────────────────────────────────────────────────────────────────────────

/// Wilder's RSI over `period` bars.
///
/// Uses Wilder's smoothed averages (equivalent to EMA with α = 1/period).
/// The first `period` values are 0.0.
pub fn rsi(prices: &[f64], period: usize) -> Vec<f64> {
    let n = prices.len();
    if period == 0 || n < 2 {
        return vec![0.0; n];
    }
    let mut out = vec![0.0; n];

    // Compute first-order differences.
    let mut gains = vec![0.0; n];
    let mut losses = vec![0.0; n];
    for i in 1..n {
        let diff = prices[i] - prices[i - 1];
        if diff > 0.0 {
            gains[i] = diff;
        } else {
            losses[i] = -diff;
        }
    }

    if n <= period {
        return out;
    }

    // Seed: simple average over first `period` differences (indices 1..=period).
    let avg_gain_seed: f64 = gains[1..=period].iter().sum::<f64>() / period as f64;
    let avg_loss_seed: f64 = losses[1..=period].iter().sum::<f64>() / period as f64;

    let mut avg_gain = avg_gain_seed;
    let mut avg_loss = avg_loss_seed;

    let rs = if avg_loss == 0.0 { f64::INFINITY } else { avg_gain / avg_loss };
    out[period] = 100.0 - 100.0 / (1.0 + rs);

    for i in (period + 1)..n {
        avg_gain = (avg_gain * (period as f64 - 1.0) + gains[i]) / period as f64;
        avg_loss = (avg_loss * (period as f64 - 1.0) + losses[i]) / period as f64;
        let rs = if avg_loss == 0.0 { f64::INFINITY } else { avg_gain / avg_loss };
        out[i] = 100.0 - 100.0 / (1.0 + rs);
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// MACD
// ─────────────────────────────────────────────────────────────────────────────

/// MACD indicator: (macd_line, signal_line, histogram).
///
/// `fast` / `slow` / `signal` are EMA periods.
/// Values before warm-up are 0.0.
pub fn macd(
    prices: &[f64],
    fast: usize,
    slow: usize,
    signal: usize,
) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let n = prices.len();
    let ema_fast = ema(prices, fast);
    let ema_slow = ema(prices, slow);

    // MACD line = fast EMA − slow EMA (non-zero only where both have warmed up).
    let mut macd_line = vec![0.0; n];
    for i in (slow - 1).min(n - 1)..n {
        macd_line[i] = ema_fast[i] - ema_slow[i];
    }

    // Signal line = EMA of MACD line (treat 0.0 prefix as if the series starts at slow-1).
    let signal_line = ema(&macd_line, signal);

    // Histogram = MACD − signal.
    let histogram: Vec<f64> = macd_line
        .iter()
        .zip(signal_line.iter())
        .map(|(&m, &s)| m - s)
        .collect();

    (macd_line, signal_line, histogram)
}

// ─────────────────────────────────────────────────────────────────────────────
// Bollinger Bands
// ─────────────────────────────────────────────────────────────────────────────

/// Bollinger Bands: (upper, middle, lower).
///
/// Middle = SMA; upper/lower = middle ± `std_dev_mult` × rolling std dev.
/// First `period - 1` values are 0.0 in all three bands.
pub fn bollinger_bands(
    prices: &[f64],
    period: usize,
    std_dev_mult: f64,
) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let n = prices.len();
    if period == 0 || n == 0 {
        return (vec![0.0; n], vec![0.0; n], vec![0.0; n]);
    }
    let middle = sma(prices, period);
    let mut upper = vec![0.0; n];
    let mut lower = vec![0.0; n];

    for i in (period - 1)..n {
        let window = &prices[(i + 1 - period)..=i];
        let mean = middle[i];
        let variance = window.iter().map(|&p| (p - mean).powi(2)).sum::<f64>() / period as f64;
        let std_dev = variance.sqrt();
        upper[i] = mean + std_dev_mult * std_dev;
        lower[i] = mean - std_dev_mult * std_dev;
    }
    (upper, middle, lower)
}

// ─────────────────────────────────────────────────────────────────────────────
// Average True Range
// ─────────────────────────────────────────────────────────────────────────────

/// Average True Range (Wilder's smoothing) over `period` bars.
///
/// True Range = max(H−L, |H−prev_C|, |L−prev_C|).
/// The first `period` values are 0.0.
pub fn atr(ohlcv: &[Ohlcv], period: usize) -> Vec<f64> {
    let n = ohlcv.len();
    if period == 0 || n < 2 {
        return vec![0.0; n];
    }
    let mut out = vec![0.0; n];
    let mut tr_vals = vec![0.0; n];
    for i in 1..n {
        let h = ohlcv[i].high;
        let l = ohlcv[i].low;
        let pc = ohlcv[i - 1].close;
        tr_vals[i] = (h - l).max((h - pc).abs()).max((l - pc).abs());
    }

    if period > n {
        return out;
    }

    // Seed: average of first `period` TR values (indices 1..=period).
    let seed: f64 = tr_vals[1..=period.min(n - 1)].iter().sum::<f64>() / period as f64;
    if period < n {
        out[period] = seed;
    }
    let mut prev = seed;
    for i in (period + 1)..n {
        let a = (prev * (period as f64 - 1.0) + tr_vals[i]) / period as f64;
        out[i] = a;
        prev = a;
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// On-Balance Volume
// ─────────────────────────────────────────────────────────────────────────────

/// On-Balance Volume.
///
/// Accumulates volume: +V when close > prev close, −V when close < prev close,
/// unchanged otherwise.  The first bar carries its own volume as the seed.
pub fn obv(ohlcv: &[Ohlcv]) -> Vec<f64> {
    let n = ohlcv.len();
    if n == 0 {
        return vec![];
    }
    let mut out = vec![0.0; n];
    out[0] = ohlcv[0].volume;
    for i in 1..n {
        let delta = if ohlcv[i].close > ohlcv[i - 1].close {
            ohlcv[i].volume
        } else if ohlcv[i].close < ohlcv[i - 1].close {
            -ohlcv[i].volume
        } else {
            0.0
        };
        out[i] = out[i - 1] + delta;
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// Stochastic Oscillator
// ─────────────────────────────────────────────────────────────────────────────

/// Stochastic oscillator: (%K, %D).
///
/// %K = (close − lowest_low) / (highest_high − lowest_low) × 100
/// %D = SMA(`d_period`) of %K.
///
/// Values before warm-up are 0.0.
pub fn stochastic(ohlcv: &[Ohlcv], k_period: usize, d_period: usize) -> (Vec<f64>, Vec<f64>) {
    let n = ohlcv.len();
    if k_period == 0 || n == 0 {
        return (vec![0.0; n], vec![0.0; n]);
    }
    let mut k_vals = vec![0.0; n];
    for i in (k_period - 1)..n {
        let window = &ohlcv[(i + 1 - k_period)..=i];
        let lowest_low = window.iter().map(|b| b.low).fold(f64::INFINITY, f64::min);
        let highest_high = window.iter().map(|b| b.high).fold(f64::NEG_INFINITY, f64::max);
        let range = highest_high - lowest_low;
        k_vals[i] = if range == 0.0 {
            50.0
        } else {
            (ohlcv[i].close - lowest_low) / range * 100.0
        };
    }
    let d_vals = sma(&k_vals, d_period);
    (k_vals, d_vals)
}

// ─────────────────────────────────────────────────────────────────────────────
// Candlestick Patterns
// ─────────────────────────────────────────────────────────────────────────────

/// Named candlestick patterns.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CandlePattern {
    /// Open ≈ close; tiny body relative to range.
    Doji,
    /// Long lower shadow, small upper shadow, small body near top.
    Hammer,
    /// Long upper shadow, small lower shadow, small body near bottom.
    InvertedHammer,
    /// Bullish bar fully engulfs the prior bearish bar.
    BullishEngulfing,
    /// Bearish bar fully engulfs the prior bullish bar.
    BearishEngulfing,
    /// Three-bar bottom reversal: bearish, small doji/star, bullish.
    MorningStar,
    /// Three-bar top reversal: bullish, small doji/star, bearish.
    EveningStar,
    /// Three consecutive bullish bars with higher closes.
    ThreeWhiteSoldiers,
    /// Three consecutive bearish bars with lower closes.
    ThreeBlackCrows,
}

/// Detect candlestick patterns in an OHLCV series.
///
/// Returns a list of `(bar_index, pattern)` pairs for every detected occurrence.
/// A single bar may appear more than once if multiple patterns apply.
pub fn detect_patterns(ohlcv: &[Ohlcv]) -> Vec<(usize, CandlePattern)> {
    let n = ohlcv.len();
    let mut results = Vec::new();

    for i in 0..n {
        let bar = &ohlcv[i];
        let body = (bar.close - bar.open).abs();
        let range = bar.high - bar.low;

        // ── Doji ────────────────────────────────────────────────────────────
        if range > 0.0 && body / range < 0.1 {
            results.push((i, CandlePattern::Doji));
        }

        // ── Hammer ──────────────────────────────────────────────────────────
        if body > 0.0 && range > 0.0 {
            let upper_wick = bar.high - bar.close.max(bar.open);
            let lower_wick = bar.close.min(bar.open) - bar.low;
            if lower_wick >= 2.0 * body && upper_wick <= 0.5 * body {
                results.push((i, CandlePattern::Hammer));
            }
            // ── InvertedHammer ──────────────────────────────────────────────
            if upper_wick >= 2.0 * body && lower_wick <= 0.5 * body {
                results.push((i, CandlePattern::InvertedHammer));
            }
        }

        // ── Two-bar patterns ────────────────────────────────────────────────
        if i >= 1 {
            let prev = &ohlcv[i - 1];
            let prev_bearish = prev.close < prev.open;
            let prev_bullish = prev.close > prev.open;
            let curr_bullish = bar.close > bar.open;
            let curr_bearish = bar.close < bar.open;

            // BullishEngulfing
            if prev_bearish
                && curr_bullish
                && bar.open <= prev.close
                && bar.close >= prev.open
            {
                results.push((i, CandlePattern::BullishEngulfing));
            }

            // BearishEngulfing
            if prev_bullish
                && curr_bearish
                && bar.open >= prev.close
                && bar.close <= prev.open
            {
                results.push((i, CandlePattern::BearishEngulfing));
            }
        }

        // ── Three-bar patterns ───────────────────────────────────────────────
        if i >= 2 {
            let b0 = &ohlcv[i - 2];
            let b1 = &ohlcv[i - 1];

            let b0_bearish = b0.close < b0.open;
            let b0_bullish = b0.close > b0.open;
            let b1_range = b1.high - b1.low;
            let b1_body = (b1.close - b1.open).abs();
            let b1_small = b1_range > 0.0 && b1_body / b1_range < 0.3;
            let curr_bullish = bar.close > bar.open;
            let curr_bearish = bar.close < bar.open;

            // MorningStar: bearish, small star, bullish
            if b0_bearish && b1_small && curr_bullish && bar.close > (b0.open + b0.close) / 2.0 {
                results.push((i, CandlePattern::MorningStar));
            }

            // EveningStar: bullish, small star, bearish
            if b0_bullish && b1_small && curr_bearish && bar.close < (b0.open + b0.close) / 2.0 {
                results.push((i, CandlePattern::EveningStar));
            }

            // ThreeWhiteSoldiers
            if b0.close > b0.open
                && b1.close > b1.open
                && bar.close > bar.open
                && b1.close > b0.close
                && bar.close > b1.close
            {
                results.push((i, CandlePattern::ThreeWhiteSoldiers));
            }

            // ThreeBlackCrows
            if b0.close < b0.open
                && b1.close < b1.open
                && bar.close < bar.open
                && b1.close < b0.close
                && bar.close < b1.close
            {
                results.push((i, CandlePattern::ThreeBlackCrows));
            }
        }
    }

    results
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(o: f64, h: f64, l: f64, c: f64) -> Ohlcv {
        Ohlcv { open: o, high: h, low: l, close: c, volume: 1000.0, timestamp_ms: 0 }
    }

    #[test]
    fn sma_known_series() {
        let prices = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let out = sma(&prices, 3);
        assert_eq!(out[0], 0.0);
        assert_eq!(out[1], 0.0);
        assert!((out[2] - 2.0).abs() < 1e-10);
        assert!((out[3] - 3.0).abs() < 1e-10);
        assert!((out[4] - 4.0).abs() < 1e-10);
    }

    #[test]
    fn ema_seed_equals_sma() {
        let prices = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let out_ema = ema(&prices, 3);
        let out_sma = sma(&prices, 3);
        // At the seed index (2) EMA should equal SMA.
        assert!((out_ema[2] - out_sma[2]).abs() < 1e-10);
    }

    #[test]
    fn rsi_in_range() {
        let prices: Vec<f64> = (0..30).map(|i| 100.0 + (i as f64).sin() * 5.0).collect();
        let out = rsi(&prices, 14);
        for &v in out.iter().skip(14) {
            assert!(v >= 0.0 && v <= 100.0, "RSI out of range: {v}");
        }
    }

    #[test]
    fn macd_signal_crossover_exists() {
        // Steadily rising then falling prices should produce at least one crossover.
        let mut prices: Vec<f64> = (0..50).map(|i| i as f64).collect();
        prices.extend((0..50).map(|i| 49.0 - i as f64));
        let (macd_line, signal_line, _hist) = macd(&prices, 12, 26, 9);
        let crossovers = macd_line
            .iter()
            .zip(signal_line.iter())
            .zip(macd_line.iter().skip(1).zip(signal_line.iter().skip(1)))
            .filter(|((m0, s0), (m1, s1))| (m0 > s0) != (m1 > s1))
            .count();
        assert!(crossovers > 0, "expected at least one MACD/signal crossover");
    }

    #[test]
    fn bollinger_width_positive() {
        let prices: Vec<f64> = (0..30).map(|i| 100.0 + (i % 5) as f64).collect();
        let (upper, middle, lower) = bollinger_bands(&prices, 10, 2.0);
        for i in 9..30 {
            assert!(upper[i] > lower[i], "upper <= lower at index {i}");
            assert!((upper[i] + lower[i]) / 2.0 - middle[i] < 1e-9);
        }
    }

    #[test]
    fn doji_detection() {
        // Perfect doji: open == close, has range.
        let bars = vec![bar(10.0, 12.0, 8.0, 10.0)];
        let patterns = detect_patterns(&bars);
        let has_doji = patterns.iter().any(|(_, p)| *p == CandlePattern::Doji);
        assert!(has_doji, "should detect doji");
    }

    #[test]
    fn bullish_engulfing_detection() {
        let bars = vec![
            bar(12.0, 13.0, 10.0, 10.5), // bearish
            bar(9.5, 13.5, 9.0, 13.0),   // bullish engulfing
        ];
        let patterns = detect_patterns(&bars);
        let found = patterns.iter().any(|(_, p)| *p == CandlePattern::BullishEngulfing);
        assert!(found, "should detect BullishEngulfing");
    }

    #[test]
    fn atr_positive() {
        let bars: Vec<Ohlcv> = (0..20)
            .map(|i| Ohlcv {
                open: 100.0,
                high: 102.0 + i as f64 * 0.1,
                low: 98.0 - i as f64 * 0.1,
                close: 100.5,
                volume: 500.0,
                timestamp_ms: i * 1000,
            })
            .collect();
        let out = atr(&bars, 14);
        assert!(out[14] > 0.0, "ATR should be positive");
    }

    #[test]
    fn obv_accumulates_correctly() {
        let bars = vec![
            bar(10.0, 11.0, 9.0, 10.0),
            bar(10.0, 12.0, 9.5, 11.0), // close higher → +volume
            bar(11.0, 11.5, 9.0, 9.5),  // close lower → -volume
        ];
        let out = obv(&bars);
        assert_eq!(out[0], 1000.0);
        assert_eq!(out[1], 2000.0);
        assert_eq!(out[2], 1000.0);
    }

    #[test]
    fn stochastic_in_range() {
        let bars: Vec<Ohlcv> = (0..20)
            .map(|i| Ohlcv {
                open: 100.0,
                high: 100.0 + (i % 5) as f64,
                low: 99.0 - (i % 3) as f64,
                close: 100.0 + (i % 4) as f64 * 0.5,
                volume: 1000.0,
                timestamp_ms: i as u64 * 1000,
            })
            .collect();
        let (k, d) = stochastic(&bars, 5, 3);
        for i in 4..20 {
            assert!(k[i] >= 0.0 && k[i] <= 100.0, "%K out of range at {i}: {}", k[i]);
        }
        for i in 6..20 {
            assert!(d[i] >= 0.0 && d[i] <= 100.0, "%D out of range at {i}: {}", d[i]);
        }
    }
}
