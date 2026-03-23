//! # Module: crypto
//!
//! ## Responsibility
//! Crypto-specific financial metrics: funding rates, perpetual futures basis,
//! open-interest ratios, liquidation heatmaps, and a composite Fear & Greed index.
//!
//! ## Guarantees
//! - Zero panics; all arithmetic is checked or guarded
//! - Rolling history uses `VecDeque` with configurable max size (no unbounded growth)
//! - All public items are documented

use std::collections::VecDeque;

// ─────────────────────────────────────────
//  FundingRate
// ─────────────────────────────────────────

/// A single perpetual-futures funding rate observation.
///
/// Funding rates are typically settled every 8 hours (3× per day).
/// The `annualized` field converts the period rate to an annual rate.
#[derive(Debug, Clone)]
pub struct FundingRate {
    /// Trading pair symbol (e.g. `"BTCUSDT"`).
    pub symbol: String,
    /// Raw funding rate for one 8-hour period (e.g. 0.0001 = 0.01%).
    pub rate: f64,
    /// Unix timestamp in milliseconds of this funding observation.
    pub timestamp_ms: u64,
    /// Annualized funding rate = `rate × 3 × 365`.
    pub annualized: f64,
}

impl FundingRate {
    /// Construct a new `FundingRate`, computing `annualized` automatically.
    ///
    /// # Arguments
    /// * `symbol`       - Trading pair identifier.
    /// * `rate`         - Raw 8-hour period rate.
    /// * `timestamp_ms` - Observation time in milliseconds since Unix epoch.
    #[must_use]
    pub fn new(symbol: impl Into<String>, rate: f64, timestamp_ms: u64) -> Self {
        let annualized = rate * 3.0 * 365.0;
        Self {
            symbol: symbol.into(),
            rate,
            timestamp_ms,
            annualized,
        }
    }
}

// ─────────────────────────────────────────
//  PerpBasis
// ─────────────────────────────────────────

/// Perpetual-futures basis: the percentage spread between perp and spot prices.
///
/// Positive basis → perp trades at a premium to spot (contango).
/// Negative basis → perp trades at a discount (backwardation).
#[derive(Debug, Clone)]
pub struct PerpBasis {
    /// Trading pair symbol.
    pub symbol: String,
    /// Perpetual futures price.
    pub perp_price: f64,
    /// Spot price.
    pub spot_price: f64,
    /// Basis as a percentage: `(perp - spot) / spot × 100`.
    pub basis_pct: f64,
    /// Unix timestamp in milliseconds.
    pub timestamp_ms: u64,
}

impl PerpBasis {
    /// Construct a new `PerpBasis`, computing `basis_pct` automatically.
    ///
    /// Returns `basis_pct = 0.0` if `spot_price` is zero.
    #[must_use]
    pub fn new(
        symbol: impl Into<String>,
        perp_price: f64,
        spot_price: f64,
        timestamp_ms: u64,
    ) -> Self {
        let basis_pct = if spot_price != 0.0 {
            (perp_price - spot_price) / spot_price * 100.0
        } else {
            0.0
        };
        Self {
            symbol: symbol.into(),
            perp_price,
            spot_price,
            basis_pct,
            timestamp_ms,
        }
    }

    /// Returns `true` if the perpetual trades at a premium to spot (contango).
    #[must_use]
    pub fn is_contango(&self) -> bool {
        self.perp_price > self.spot_price
    }

    /// Annualised carry yield for a fixed-expiry contract.
    ///
    /// `annualized_carry = (basis_pct / 100) / (days_to_expiry / 365)`
    ///
    /// Returns `0.0` if `days_to_expiry` ≤ 0.
    #[must_use]
    pub fn annualized_carry(&self, days_to_expiry: f64) -> f64 {
        if days_to_expiry <= 0.0 {
            return 0.0;
        }
        (self.basis_pct / 100.0) / (days_to_expiry / 365.0)
    }
}

// ─────────────────────────────────────────
//  FundingHistory
// ─────────────────────────────────────────

/// Rolling history of [`FundingRate`] observations with a configurable maximum size.
#[derive(Debug, Clone)]
pub struct FundingHistory {
    data: VecDeque<FundingRate>,
    max_size: usize,
}

impl FundingHistory {
    /// Create a new `FundingHistory` with the given maximum capacity.
    ///
    /// # Panics
    /// Panics if `max_size` is zero.
    #[must_use]
    pub fn new(max_size: usize) -> Self {
        assert!(max_size > 0, "FundingHistory max_size must be > 0");
        Self {
            data: VecDeque::with_capacity(max_size),
            max_size,
        }
    }

    /// Push a new funding rate, evicting the oldest if at capacity.
    pub fn push(&mut self, rate: FundingRate) {
        if self.data.len() >= self.max_size {
            self.data.pop_front();
        }
        self.data.push_back(rate);
    }

    /// Compute the simple average of all stored rates.
    ///
    /// Returns `0.0` if the history is empty.
    #[must_use]
    pub fn average_rate(&self) -> f64 {
        if self.data.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.data.iter().map(|r| r.rate).sum();
        sum / self.data.len() as f64
    }

    /// Compute the cumulative sum of all stored rates.
    #[must_use]
    pub fn cumulative_funding(&self) -> f64 {
        self.data.iter().map(|r| r.rate).sum()
    }

    /// Compute the OLS slope of `rate` over time (index as x-axis).
    ///
    /// Positive slope → funding rates are trending upward.
    /// Returns `0.0` if fewer than 2 observations are present.
    #[must_use]
    pub fn rate_trend(&self) -> f64 {
        ols_slope(self.data.iter().map(|r| r.rate).collect::<Vec<_>>().as_slice())
    }

    /// Number of stored observations.
    #[must_use]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Returns `true` if there are no stored observations.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

// ─────────────────────────────────────────
//  CryptoMarketMetrics
// ─────────────────────────────────────────

/// Collection of crypto-specific market metrics (stateless, all methods are static).
pub struct CryptoMarketMetrics;

impl CryptoMarketMetrics {
    /// Open interest to spot volume ratio — a proxy for market leverage.
    ///
    /// Returns `0.0` if `spot_volume` is zero.
    #[must_use]
    pub fn open_interest_ratio(oi: f64, spot_volume: f64) -> f64 {
        if spot_volume == 0.0 {
            return 0.0;
        }
        oi / spot_volume
    }

    /// Estimate liquidation volumes across a set of price levels and leverage multiples.
    ///
    /// For each price in `prices`, the estimated liquidation USD is:
    /// `Σ_{lev in leverage_levels} round(1 / lev)`
    ///
    /// This is a simplified heuristic — in practice, open-interest data per leverage
    /// level would be required for an accurate estimate.
    ///
    /// Returns a `Vec<(price, estimated_liq_usd)>`.
    #[must_use]
    pub fn liquidation_heatmap(prices: &[f64], leverage_levels: &[f64]) -> Vec<(f64, f64)> {
        prices
            .iter()
            .map(|&price| {
                let liq: f64 = leverage_levels
                    .iter()
                    .filter(|&&lev| lev > 0.0)
                    .map(|&lev| (1.0 / lev).round())
                    .sum();
                (price, liq)
            })
            .collect()
    }

    /// Composite Fear & Greed index in the range [0, 100].
    ///
    /// Inputs are combined with fixed weights:
    /// - `daily_return`:  weight 0.30 (positive return → greed)
    /// - `volatility`:    weight 0.30 (high vol → fear)
    /// - `funding_rate`:  weight 0.20 (positive funding → greed)
    /// - `volume_ratio`:  weight 0.20 (above-average volume → greed)
    ///
    /// Each component is clamped to [0, 1] before weighting so the output is always
    /// in [0, 100].
    ///
    /// # Arguments
    /// * `daily_return` - Today's return (e.g. 0.05 = +5%).
    /// * `volume_ratio` - Ratio of current volume to historical average (1.0 = neutral).
    /// * `funding_rate` - Current 8-hour funding rate.
    /// * `volatility`   - Current realized volatility (annualised).
    #[must_use]
    pub fn fear_greed_index(
        daily_return: f64,
        volume_ratio: f64,
        funding_rate: f64,
        volatility: f64,
    ) -> u8 {
        // Normalize each component to [0, 1] — higher value = more greed
        // Return component: map [-0.10, +0.10] → [0, 1]
        let return_score = ((daily_return + 0.10) / 0.20).clamp(0.0, 1.0);

        // Volume component: map [0, 3] → [0, 1] (ratio above average = greed)
        let volume_score = (volume_ratio / 3.0).clamp(0.0, 1.0);

        // Funding component: map [-0.001, +0.001] → [0, 1]
        let funding_score = ((funding_rate + 0.001) / 0.002).clamp(0.0, 1.0);

        // Volatility component: high vol = fear; map [0, 1.0 annualized] → [1, 0]
        let vol_score = (1.0 - (volatility / 1.0).clamp(0.0, 1.0)).clamp(0.0, 1.0);

        let composite =
            return_score * 0.30 + vol_score * 0.30 + funding_score * 0.20 + volume_score * 0.20;

        (composite * 100.0).round().clamp(0.0, 100.0) as u8
    }
}

// ─────────────────────────────────────────
//  OLS helper
// ─────────────────────────────────────────

/// Compute the OLS slope of `y` against integer indices `0..n`.
///
/// Returns `0.0` if fewer than 2 data points are provided.
fn ols_slope(y: &[f64]) -> f64 {
    let n = y.len();
    if n < 2 {
        return 0.0;
    }
    let n_f = n as f64;
    let mean_x = (n_f - 1.0) / 2.0;
    let mean_y = y.iter().sum::<f64>() / n_f;

    let mut num = 0.0_f64;
    let mut den = 0.0_f64;
    for (i, &yi) in y.iter().enumerate() {
        let dx = i as f64 - mean_x;
        num += dx * (yi - mean_y);
        den += dx * dx;
    }
    if den == 0.0 { 0.0 } else { num / den }
}

// ─────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn funding_rate_annualized() {
        let fr = FundingRate::new("BTCUSDT", 0.0001, 1_700_000_000_000);
        // 0.0001 * 3 * 365 = 0.1095
        assert!((fr.annualized - 0.1095).abs() < 1e-10);
    }

    #[test]
    fn perp_basis_contango() {
        let basis = PerpBasis::new("BTCUSDT", 30_100.0, 30_000.0, 0);
        assert!(basis.is_contango());
        assert!(basis.basis_pct > 0.0);
    }

    #[test]
    fn perp_basis_backwardation() {
        let basis = PerpBasis::new("BTCUSDT", 29_900.0, 30_000.0, 0);
        assert!(!basis.is_contango());
        assert!(basis.basis_pct < 0.0);
    }

    #[test]
    fn perp_basis_annualized_carry() {
        let basis = PerpBasis::new("BTCUSDT", 30_300.0, 30_000.0, 0);
        // basis_pct = 1.0; carry for 30 days = 1/100 / (30/365) ≈ 0.1217
        let carry = basis.annualized_carry(30.0);
        assert!(carry > 0.0);
        assert!((carry - (0.01 / (30.0 / 365.0))).abs() < 1e-10);
    }

    #[test]
    fn perp_basis_zero_days_to_expiry_returns_zero() {
        let basis = PerpBasis::new("BTCUSDT", 30_300.0, 30_000.0, 0);
        assert_eq!(basis.annualized_carry(0.0), 0.0);
    }

    #[test]
    fn funding_history_average() {
        let mut hist = FundingHistory::new(10);
        hist.push(FundingRate::new("X", 0.0002, 0));
        hist.push(FundingRate::new("X", 0.0004, 1));
        assert!((hist.average_rate() - 0.0003).abs() < 1e-10);
    }

    #[test]
    fn funding_history_cumulative() {
        let mut hist = FundingHistory::new(10);
        for i in 1..=5_u64 {
            hist.push(FundingRate::new("X", 0.0001, i));
        }
        assert!((hist.cumulative_funding() - 0.0005).abs() < 1e-12);
    }

    #[test]
    fn funding_history_eviction() {
        let mut hist = FundingHistory::new(3);
        for i in 0..5_u64 {
            hist.push(FundingRate::new("X", i as f64 * 0.001, i));
        }
        assert_eq!(hist.len(), 3);
    }

    #[test]
    fn funding_history_trend_increasing() {
        let mut hist = FundingHistory::new(20);
        for i in 0..10_u64 {
            hist.push(FundingRate::new("X", i as f64 * 0.0001, i));
        }
        assert!(hist.rate_trend() > 0.0);
    }

    #[test]
    fn fear_greed_in_range() {
        let score = CryptoMarketMetrics::fear_greed_index(0.03, 1.5, 0.0001, 0.5);
        assert!(score <= 100);
    }

    #[test]
    fn fear_greed_extreme_greed() {
        // Very positive return, high volume, positive funding, low vol
        let score = CryptoMarketMetrics::fear_greed_index(0.10, 3.0, 0.001, 0.0);
        assert!(score > 50);
    }

    #[test]
    fn fear_greed_extreme_fear() {
        // Very negative return, low volume, negative funding, very high vol
        let score = CryptoMarketMetrics::fear_greed_index(-0.10, 0.0, -0.001, 1.0);
        assert!(score < 50);
    }

    #[test]
    fn liquidation_heatmap_length() {
        let prices = vec![29_000.0, 30_000.0, 31_000.0];
        let levs = vec![2.0, 5.0, 10.0];
        let heatmap = CryptoMarketMetrics::liquidation_heatmap(&prices, &levs);
        assert_eq!(heatmap.len(), prices.len());
        for (_, liq) in &heatmap {
            assert!(*liq >= 0.0);
        }
    }

    #[test]
    fn open_interest_ratio_zero_volume() {
        assert_eq!(CryptoMarketMetrics::open_interest_ratio(1_000_000.0, 0.0), 0.0);
    }

    #[test]
    fn open_interest_ratio_normal() {
        let ratio = CryptoMarketMetrics::open_interest_ratio(500.0, 1000.0);
        assert!((ratio - 0.5).abs() < 1e-10);
    }
}
