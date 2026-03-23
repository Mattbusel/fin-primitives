//! # Module: ml_features
//!
//! ## Responsibility
//! ML feature engineering for financial time-series: price-based features,
//! microstructure features, feature vector abstraction, normalization,
//! cross-sectional ranking, and lagged feature construction.
//!
//! ## Guarantees
//! - Zero panics; all fallible operations return `Result<_, FinError>`
//! - Normalizer must be `fit` before `transform` — enforced at runtime
//! - `LaggedFeatures` pads short series with `f64::NAN` (flagged, not silent)

use crate::error::FinError;

// ─────────────────────────────────────────
//  PriceFeatures
// ─────────────────────────────────────────

/// Price-derived features computed from a close-price series.
///
/// # Example
/// ```rust
/// use fin_primitives::ml_features::PriceFeatures;
///
/// let closes = vec![100.0, 101.0, 102.0, 101.5, 103.0];
/// let pf = PriceFeatures::compute(&closes, 3).unwrap();
/// assert!(pf.log_returns.len() == 4);
/// ```
#[derive(Debug, Clone)]
pub struct PriceFeatures {
    /// Log returns: `ln(close_t / close_{t-1})`.
    pub log_returns: Vec<f64>,
    /// Realized volatility: rolling std-dev of log returns over the last `window` periods.
    pub realized_volatility: f64,
    /// n-period momentum: `close[-1] / close[-n-1] - 1`.
    pub momentum: f64,
    /// RSI over the last `window` periods (0–100).
    pub rsi: f64,
    /// MACD signal line: EMA(12) − EMA(26) of closes.
    pub macd_signal: f64,
    /// Bollinger Band position: `(close - lower) / (upper - lower)`, in `[0, 1]`.
    pub bollinger_position: f64,
}

impl PriceFeatures {
    /// Compute price features from a close-price series.
    ///
    /// Requires at least `window + 1` observations for all features.
    ///
    /// # Errors
    /// - [`FinError::InsufficientData`] if fewer than `window + 2` closes.
    /// - [`FinError::InvalidPeriod`] if `window == 0`.
    pub fn compute(closes: &[f64], window: usize) -> Result<Self, FinError> {
        if window == 0 {
            return Err(FinError::InvalidPeriod(0));
        }
        if closes.len() < window + 2 {
            return Err(FinError::InvalidInput("insufficient close prices for window".into()));
        }
        let n = closes.len();
        // Log returns
        let log_returns: Vec<f64> = (1..n)
            .map(|i| (closes[i] / closes[i - 1]).ln())
            .collect();

        // Realized volatility (std of log returns over last `window` returns)
        let rv_slice = &log_returns[log_returns.len().saturating_sub(window)..];
        let realized_volatility = std_dev(rv_slice);

        // Momentum: current vs n periods ago
        let momentum = if n > window {
            closes[n - 1] / closes[n - 1 - window] - 1.0
        } else {
            0.0
        };

        // RSI
        let rsi = compute_rsi(&log_returns, window);

        // MACD signal (EMA12 - EMA26 applied to closes)
        let macd_signal = compute_macd_signal(closes);

        // Bollinger band position
        let bollinger_position = compute_bollinger_position(closes, window);

        Ok(Self {
            log_returns,
            realized_volatility,
            momentum,
            rsi,
            macd_signal,
            bollinger_position,
        })
    }
}

fn mean(data: &[f64]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    data.iter().sum::<f64>() / data.len() as f64
}

fn std_dev(data: &[f64]) -> f64 {
    if data.len() < 2 {
        return 0.0;
    }
    let m = mean(data);
    let var = data.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (data.len() - 1) as f64;
    var.sqrt()
}

fn ema(data: &[f64], period: usize) -> f64 {
    if data.is_empty() || period == 0 {
        return 0.0;
    }
    let k = 2.0 / (period as f64 + 1.0);
    let mut ema_val = data[0];
    for &v in &data[1..] {
        ema_val = v * k + ema_val * (1.0 - k);
    }
    ema_val
}

fn compute_rsi(log_returns: &[f64], window: usize) -> f64 {
    let slice = &log_returns[log_returns.len().saturating_sub(window)..];
    if slice.is_empty() {
        return 50.0;
    }
    let gains: Vec<f64> = slice.iter().map(|r| r.max(0.0)).collect();
    let losses: Vec<f64> = slice.iter().map(|r| (-r).max(0.0)).collect();
    let avg_gain = mean(&gains);
    let avg_loss = mean(&losses);
    if avg_loss == 0.0 {
        return 100.0;
    }
    let rs = avg_gain / avg_loss;
    100.0 - 100.0 / (1.0 + rs)
}

fn compute_macd_signal(closes: &[f64]) -> f64 {
    if closes.len() < 26 {
        return 0.0;
    }
    let ema12 = ema(closes, 12);
    let ema26 = ema(closes, 26);
    ema12 - ema26
}

fn compute_bollinger_position(closes: &[f64], window: usize) -> f64 {
    let n = closes.len();
    let slice = &closes[n.saturating_sub(window)..];
    if slice.len() < 2 {
        return 0.5;
    }
    let m = mean(slice);
    let sd = std_dev(slice);
    if sd == 0.0 {
        return 0.5;
    }
    let upper = m + 2.0 * sd;
    let lower = m - 2.0 * sd;
    let range = upper - lower;
    if range == 0.0 {
        return 0.5;
    }
    ((closes[n - 1] - lower) / range).clamp(0.0, 1.0)
}

// ─────────────────────────────────────────
//  MicrostructureFeatures
// ─────────────────────────────────────────

/// Market microstructure-derived features.
///
/// # Example
/// ```rust
/// use fin_primitives::ml_features::MicrostructureFeatures;
///
/// let f = MicrostructureFeatures::compute(1000.0, 800.0, 50, 0.01, 100_000.0);
/// assert!(f.order_imbalance.abs() <= 1.0);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct MicrostructureFeatures {
    /// Order imbalance: `(bid_vol - ask_vol) / (bid_vol + ask_vol)` in `[-1, 1]`.
    pub order_imbalance: f64,
    /// Trade intensity: number of trades per unit time (trades/minute equivalent).
    pub trade_intensity: f64,
    /// Price impact coefficient: `|Δprice| / dollar_volume` (Kyle's λ proxy).
    pub price_impact_coefficient: f64,
}

impl MicrostructureFeatures {
    /// Compute microstructure features.
    ///
    /// - `bid_volume`, `ask_volume`: cumulative bid/ask volumes at top of book.
    /// - `trade_count`: number of trades in the observation window.
    /// - `price_move`: absolute price change over the window.
    /// - `dollar_volume`: total dollar volume traded over the window.
    #[must_use]
    pub fn compute(
        bid_volume: f64,
        ask_volume: f64,
        trade_count: u64,
        price_move: f64,
        dollar_volume: f64,
    ) -> Self {
        let total = bid_volume + ask_volume;
        let order_imbalance = if total == 0.0 {
            0.0
        } else {
            (bid_volume - ask_volume) / total
        };
        let trade_intensity = trade_count as f64;
        let price_impact_coefficient = if dollar_volume == 0.0 {
            0.0
        } else {
            price_move.abs() / dollar_volume
        };
        Self {
            order_imbalance,
            trade_intensity,
            price_impact_coefficient,
        }
    }
}

// ─────────────────────────────────────────
//  FeatureVector
// ─────────────────────────────────────────

/// A named feature vector for ML pipelines.
///
/// # Example
/// ```rust
/// use fin_primitives::ml_features::FeatureVector;
///
/// let fv = FeatureVector::new(
///     vec!["momentum".into(), "rsi".into()],
///     vec![0.02, 65.0],
/// ).unwrap();
/// assert_eq!(fv.get("rsi"), Some(65.0));
/// ```
#[derive(Debug, Clone)]
pub struct FeatureVector {
    /// Feature names in the same order as `values`.
    pub names: Vec<String>,
    /// Feature values.
    pub values: Vec<f64>,
}

impl FeatureVector {
    /// Constructs a [`FeatureVector`].
    ///
    /// # Errors
    /// [`FinError::InvalidInput`] if `names` and `values` have different lengths.
    pub fn new(names: Vec<String>, values: Vec<f64>) -> Result<Self, FinError> {
        if names.len() != values.len() {
            return Err(FinError::InvalidInput(
                "names and values must have the same length".into(),
            ));
        }
        Ok(Self { names, values })
    }

    /// Returns the value for the given feature name, or `None`.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<f64> {
        self.names
            .iter()
            .position(|n| n == name)
            .map(|i| self.values[i])
    }

    /// Append a feature to the vector.
    pub fn push(&mut self, name: impl Into<String>, value: f64) {
        self.names.push(name.into());
        self.values.push(value);
    }

    /// Number of features.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Returns `true` if no features are present.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

// ─────────────────────────────────────────
//  FeatureNormalizer
// ─────────────────────────────────────────

/// Z-score (standardization) normalizer: `(x - mean) / std`.
///
/// Must be `fit` on training data before calling `transform`.
///
/// # Example
/// ```rust
/// use fin_primitives::ml_features::FeatureNormalizer;
///
/// let mut norm = FeatureNormalizer::new();
/// norm.fit(&[1.0, 2.0, 3.0, 4.0, 5.0]).unwrap();
/// let z = norm.transform(3.0).unwrap();
/// assert!((z - 0.0).abs() < 1e-10);
/// ```
#[derive(Debug, Clone)]
pub struct FeatureNormalizer {
    mean: Option<f64>,
    std: Option<f64>,
}

impl FeatureNormalizer {
    /// Constructs an unfitted normalizer.
    #[must_use]
    pub fn new() -> Self {
        Self {
            mean: None,
            std: None,
        }
    }

    /// Fit the normalizer to a dataset.
    ///
    /// # Errors
    /// [`FinError::InsufficientData`] if fewer than 2 observations.
    /// [`FinError::InvalidInput`] if std dev is zero (constant series).
    pub fn fit(&mut self, data: &[f64]) -> Result<(), FinError> {
        if data.len() < 2 {
            return Err(FinError::InvalidInput("need at least 2 data points to normalize".into()));
        }
        let m = mean(data);
        let s = std_dev(data);
        if s == 0.0 {
            return Err(FinError::InvalidInput(
                "cannot normalize a constant series".into(),
            ));
        }
        self.mean = Some(m);
        self.std = Some(s);
        Ok(())
    }

    /// Transform a single value to z-score.
    ///
    /// # Errors
    /// [`FinError::InvalidInput`] if the normalizer has not been fit.
    pub fn transform(&self, value: f64) -> Result<f64, FinError> {
        match (self.mean, self.std) {
            (Some(m), Some(s)) => Ok((value - m) / s),
            _ => Err(FinError::InvalidInput(
                "normalizer must be fit before transform".into(),
            )),
        }
    }

    /// Inverse transform a z-score back to original scale.
    ///
    /// # Errors
    /// [`FinError::InvalidInput`] if the normalizer has not been fit.
    pub fn inverse_transform(&self, z: f64) -> Result<f64, FinError> {
        match (self.mean, self.std) {
            (Some(m), Some(s)) => Ok(z * s + m),
            _ => Err(FinError::InvalidInput(
                "normalizer must be fit before inverse_transform".into(),
            )),
        }
    }

    /// Transform a slice of values in place.
    ///
    /// # Errors
    /// [`FinError::InvalidInput`] if the normalizer has not been fit.
    pub fn transform_slice(&self, values: &[f64]) -> Result<Vec<f64>, FinError> {
        values.iter().map(|v| self.transform(*v)).collect()
    }
}

impl Default for FeatureNormalizer {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────
//  CrossSectionalRanker
// ─────────────────────────────────────────

/// Ranks a cross-section of feature values as `rank / n` ∈ `(0, 1]`.
///
/// Ties are broken by stable order (first occurrence gets lower rank).
///
/// # Example
/// ```rust
/// use fin_primitives::ml_features::CrossSectionalRanker;
///
/// let ranked = CrossSectionalRanker::rank(&[30.0, 10.0, 20.0]);
/// // 10 → rank 1, 20 → rank 2, 30 → rank 3; n = 3
/// assert!((ranked[0] - 1.0).abs() < 1e-10); // 30 → 3/3
/// assert!((ranked[1] - 1.0/3.0).abs() < 1e-10); // 10 → 1/3
/// assert!((ranked[2] - 2.0/3.0).abs() < 1e-10); // 20 → 2/3
/// ```
pub struct CrossSectionalRanker;

impl CrossSectionalRanker {
    /// Rank values cross-sectionally.
    ///
    /// Returns a vector of `rank / n` values in the same order as input.
    /// Empty input returns an empty vector.
    #[must_use]
    pub fn rank(values: &[f64]) -> Vec<f64> {
        let n = values.len();
        if n == 0 {
            return vec![];
        }
        // Create (value, original_index) sorted by value ascending
        let mut indexed: Vec<(f64, usize)> = values
            .iter()
            .enumerate()
            .map(|(i, &v)| (v, i))
            .collect();
        indexed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut ranks = vec![0.0_f64; n];
        for (rank_idx, (_, orig_idx)) in indexed.iter().enumerate() {
            ranks[*orig_idx] = (rank_idx + 1) as f64 / n as f64;
        }
        ranks
    }
}

// ─────────────────────────────────────────
//  LaggedFeatures
// ─────────────────────────────────────────

/// Creates lagged versions of a feature series at standard lags (1, 5, 10, 21).
///
/// Missing values at the beginning of a lag series are filled with `f64::NAN`.
///
/// # Example
/// ```rust
/// use fin_primitives::ml_features::LaggedFeatures;
///
/// let series = (1..=25).map(|i| i as f64).collect::<Vec<_>>();
/// let lagged = LaggedFeatures::compute(&series);
/// // lag-1 of index 1 = series[0] = 1.0
/// assert!((lagged.lag1[1] - 1.0).abs() < 1e-10);
/// // lag-5 of index 0 is NaN
/// assert!(lagged.lag5[0].is_nan());
/// ```
#[derive(Debug, Clone)]
pub struct LaggedFeatures {
    /// Original series lagged by 1 period.
    pub lag1: Vec<f64>,
    /// Original series lagged by 5 periods.
    pub lag5: Vec<f64>,
    /// Original series lagged by 10 periods.
    pub lag10: Vec<f64>,
    /// Original series lagged by 21 periods.
    pub lag21: Vec<f64>,
}

impl LaggedFeatures {
    /// Compute lagged feature series from a price/return series.
    ///
    /// The output vectors have the same length as the input.
    /// Positions before the lag are `f64::NAN`.
    #[must_use]
    pub fn compute(series: &[f64]) -> Self {
        Self {
            lag1: Self::lag(series, 1),
            lag5: Self::lag(series, 5),
            lag10: Self::lag(series, 10),
            lag21: Self::lag(series, 21),
        }
    }

    fn lag(series: &[f64], n: usize) -> Vec<f64> {
        let len = series.len();
        let mut out = vec![f64::NAN; len];
        for i in n..len {
            out[i] = series[i - n];
        }
        out
    }

    /// Returns all four lag series as a vec of (lag_name, values) pairs.
    #[must_use]
    pub fn as_named_pairs(&self) -> Vec<(&'static str, &[f64])> {
        vec![
            ("lag1", &self.lag1),
            ("lag5", &self.lag5),
            ("lag10", &self.lag10),
            ("lag21", &self.lag21),
        ]
    }
}

// ─────────────────────────────────────────
//  Unit Tests
// ─────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // PriceFeatures
    #[test]
    fn price_features_basic() {
        let closes: Vec<f64> = (1..=30).map(|i| 100.0 + i as f64).collect();
        let pf = PriceFeatures::compute(&closes, 10).unwrap();
        assert!(!pf.log_returns.is_empty());
        assert!(pf.realized_volatility >= 0.0);
        assert!(pf.rsi >= 0.0 && pf.rsi <= 100.0);
        assert!(pf.bollinger_position >= 0.0 && pf.bollinger_position <= 1.0);
    }

    #[test]
    fn price_features_too_short() {
        let closes = vec![100.0, 101.0];
        assert!(PriceFeatures::compute(&closes, 5).is_err());
    }

    #[test]
    fn price_features_window_zero() {
        let closes = vec![100.0; 20];
        assert!(PriceFeatures::compute(&closes, 0).is_err());
    }

    // MicrostructureFeatures
    #[test]
    fn microstructure_order_imbalance_balanced() {
        let f = MicrostructureFeatures::compute(500.0, 500.0, 100, 0.01, 1_000_000.0);
        assert!((f.order_imbalance - 0.0).abs() < 1e-10);
    }

    #[test]
    fn microstructure_order_imbalance_extreme() {
        let f = MicrostructureFeatures::compute(1000.0, 0.0, 50, 0.01, 500_000.0);
        assert!((f.order_imbalance - 1.0).abs() < 1e-10);
    }

    #[test]
    fn microstructure_zero_volume() {
        let f = MicrostructureFeatures::compute(0.0, 0.0, 0, 0.0, 0.0);
        assert_eq!(f.order_imbalance, 0.0);
        assert_eq!(f.price_impact_coefficient, 0.0);
    }

    // FeatureVector
    #[test]
    fn feature_vector_get() {
        let fv = FeatureVector::new(
            vec!["rsi".into(), "macd".into()],
            vec![65.0, 0.5],
        )
        .unwrap();
        assert_eq!(fv.get("rsi"), Some(65.0));
        assert_eq!(fv.get("missing"), None);
    }

    #[test]
    fn feature_vector_length_mismatch() {
        assert!(FeatureVector::new(vec!["a".into()], vec![1.0, 2.0]).is_err());
    }

    #[test]
    fn feature_vector_push() {
        let mut fv = FeatureVector::new(vec![], vec![]).unwrap();
        fv.push("vol", 0.02);
        assert_eq!(fv.len(), 1);
        assert_eq!(fv.get("vol"), Some(0.02));
    }

    // FeatureNormalizer
    #[test]
    fn normalizer_fit_transform() {
        let mut norm = FeatureNormalizer::new();
        norm.fit(&[1.0, 2.0, 3.0, 4.0, 5.0]).unwrap();
        let z = norm.transform(3.0).unwrap();
        assert!((z - 0.0).abs() < 1e-10);
    }

    #[test]
    fn normalizer_inverse_transform() {
        let mut norm = FeatureNormalizer::new();
        norm.fit(&[10.0, 20.0, 30.0]).unwrap();
        let z = norm.transform(20.0).unwrap();
        let back = norm.inverse_transform(z).unwrap();
        assert!((back - 20.0).abs() < 1e-10);
    }

    #[test]
    fn normalizer_not_fit_errors() {
        let norm = FeatureNormalizer::new();
        assert!(norm.transform(1.0).is_err());
        assert!(norm.inverse_transform(0.0).is_err());
    }

    #[test]
    fn normalizer_constant_series_errors() {
        let mut norm = FeatureNormalizer::new();
        assert!(norm.fit(&[5.0, 5.0, 5.0]).is_err());
    }

    // CrossSectionalRanker
    #[test]
    fn ranker_basic() {
        let ranked = CrossSectionalRanker::rank(&[30.0, 10.0, 20.0]);
        assert!((ranked[0] - 1.0).abs() < 1e-10);
        assert!((ranked[1] - 1.0 / 3.0).abs() < 1e-10);
        assert!((ranked[2] - 2.0 / 3.0).abs() < 1e-10);
    }

    #[test]
    fn ranker_empty() {
        assert!(CrossSectionalRanker::rank(&[]).is_empty());
    }

    #[test]
    fn ranker_single() {
        let ranked = CrossSectionalRanker::rank(&[42.0]);
        assert!((ranked[0] - 1.0).abs() < 1e-10);
    }

    // LaggedFeatures
    #[test]
    fn lagged_features_basic() {
        let series: Vec<f64> = (1..=25).map(|i| i as f64).collect();
        let lf = LaggedFeatures::compute(&series);
        // lag1[0] = NaN, lag1[1] = series[0] = 1.0
        assert!(lf.lag1[0].is_nan());
        assert!((lf.lag1[1] - 1.0).abs() < 1e-10);
        // lag5[4] = NaN, lag5[5] = series[0] = 1.0
        assert!(lf.lag5[4].is_nan());
        assert!((lf.lag5[5] - 1.0).abs() < 1e-10);
    }

    #[test]
    fn lagged_features_lag21_length() {
        let series: Vec<f64> = (1..=30).map(|i| i as f64).collect();
        let lf = LaggedFeatures::compute(&series);
        assert_eq!(lf.lag21.len(), series.len());
        assert!(lf.lag21[20].is_nan());
        assert!((lf.lag21[21] - 1.0).abs() < 1e-10);
    }
}
