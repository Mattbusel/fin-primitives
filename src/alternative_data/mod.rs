//! # Module: alternative_data
//!
//! Alternative data integration: sentiment signals, web traffic proxies,
//! satellite imagery, credit card data, job postings, and patent filings.
//!
//! ## Key Types
//!
//! - [`AltDataSource`] — enum of alternative data source categories
//! - [`AltDataPoint`] — a single timestamped observation from a source
//! - [`SentimentSignal`] — aggregated social sentiment for a symbol
//! - [`AltDataAggregator`] — ingestion, lookup, composite scoring, and anomaly detection

use std::collections::HashMap;

// ─────────────────────────────────────────
//  AltDataSource
// ─────────────────────────────────────────

/// Categories of alternative data sources.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AltDataSource {
    /// Social media and news sentiment (Twitter, Reddit, StockTwits, etc.).
    SocialSentiment,
    /// Web traffic metrics (SimilarWeb, Alexa rank, search trends).
    WebTraffic,
    /// Satellite imagery-derived signals (retail parking lots, oil tank levels, etc.).
    SatelliteImagery,
    /// Credit card transaction data aggregates.
    CreditCardData,
    /// Job posting counts and hiring velocity.
    JobPostings,
    /// Patent application filings.
    PatentFilings,
}

impl AltDataSource {
    /// Typical refresh cadence for this source type (hours).
    pub fn refresh_frequency_hours(&self) -> u8 {
        match self {
            AltDataSource::SocialSentiment => 1,
            AltDataSource::WebTraffic => 24,
            AltDataSource::SatelliteImagery => 24,
            AltDataSource::CreditCardData => 24,
            AltDataSource::JobPostings => 24,
            AltDataSource::PatentFilings => 168, // weekly
        }
    }
}

// ─────────────────────────────────────────
//  AltDataPoint
// ─────────────────────────────────────────

/// A single timestamped alternative data observation.
#[derive(Debug, Clone)]
pub struct AltDataPoint {
    /// Source category.
    pub source: AltDataSource,
    /// Ticker / symbol this data pertains to.
    pub symbol: String,
    /// Numeric signal value (interpretation is source-dependent).
    pub value: f64,
    /// Confidence in this data point (0.0–1.0).
    pub confidence: f64,
    /// Unix timestamp (seconds) when this data was recorded.
    pub timestamp: u64,
    /// Arbitrary key-value metadata (e.g. `"provider"`, `"region"`).
    pub metadata: HashMap<String, String>,
}

// ─────────────────────────────────────────
//  SentimentSignal
// ─────────────────────────────────────────

/// Aggregated social sentiment for a given symbol.
#[derive(Debug, Clone)]
pub struct SentimentSignal {
    /// Ticker symbol.
    pub symbol: String,
    /// Composite sentiment score in `[-1.0, 1.0]` (negative = bearish, positive = bullish).
    pub score: f64,
    /// Total message / post volume used to compute the score.
    pub volume: u64,
    /// Number of distinct sources contributing.
    pub source_count: u32,
    /// Fraction of bullish messages (0.0–1.0).
    pub bullish_pct: f64,
}

// ─────────────────────────────────────────
//  AltDataAggregator
// ─────────────────────────────────────────

/// Ingests and queries alternative data across sources and symbols.
///
/// Internally keyed by `(symbol, source)`. Only the most-recent data point per
/// `(symbol, source)` pair is retained for `latest()` queries; all historical
/// values are retained for anomaly scoring.
pub struct AltDataAggregator {
    /// Latest data point per (symbol, source).
    latest: HashMap<(String, String), AltDataPoint>,
    /// Full history per (symbol, source) for statistics.
    history: HashMap<(String, String), Vec<f64>>,
}

impl AltDataAggregator {
    /// Create an empty aggregator.
    pub fn new() -> Self {
        Self {
            latest: HashMap::new(),
            history: HashMap::new(),
        }
    }

    fn key(symbol: &str, source: &AltDataSource) -> (String, String) {
        (symbol.to_string(), format!("{source:?}"))
    }

    /// Ingest a new data point, updating the latest observation and history.
    pub fn ingest(&mut self, point: AltDataPoint) {
        let k = Self::key(&point.symbol, &point.source);
        self.history
            .entry(k.clone())
            .or_default()
            .push(point.value);
        self.latest.insert(k, point);
    }

    /// Return the most-recent data point for `(symbol, source)`, if any.
    pub fn latest(&self, symbol: &str, source: &AltDataSource) -> Option<&AltDataPoint> {
        let k = Self::key(symbol, source);
        self.latest.get(&k)
    }

    /// Compute a composite signal for `symbol` as a weighted average of all
    /// available source values.
    ///
    /// `weights` maps source debug names (e.g. `"SocialSentiment"`) to their weight.
    /// Sources absent from the map receive weight 1.0.
    /// Returns `0.0` if no data is available for the symbol.
    pub fn composite_signal(&self, symbol: &str, weights: &HashMap<String, f64>) -> f64 {
        let mut weighted_sum = 0.0_f64;
        let mut total_weight = 0.0_f64;

        for ((sym, src_key), point) in &self.latest {
            if sym != symbol {
                continue;
            }
            let w = weights.get(src_key).copied().unwrap_or(1.0);
            weighted_sum += point.value * w;
            total_weight += w;
        }

        if total_weight == 0.0 {
            0.0
        } else {
            weighted_sum / total_weight
        }
    }

    /// Compute the Pearson correlation between the composite signals of two symbols
    /// across matching `(symbol, source)` time series.
    ///
    /// Uses all historical values for each source shared by both symbols.
    /// Returns `None` if there are fewer than 2 aligned observations.
    pub fn signal_correlation(&self, symbol_a: &str, symbol_b: &str) -> Option<f64> {
        // Collect per-source history for both symbols.
        let mut pairs: Vec<(f64, f64)> = Vec::new();

        // Gather all source keys present for symbol_a.
        for (sym_src, vals_a) in &self.history {
            if sym_src.0 != symbol_a {
                continue;
            }
            let src_key = &sym_src.1;
            let key_b = (symbol_b.to_string(), src_key.clone());
            if let Some(vals_b) = self.history.get(&key_b) {
                // Align by taking the shorter length.
                let n = vals_a.len().min(vals_b.len());
                for i in 0..n {
                    pairs.push((vals_a[i], vals_b[i]));
                }
            }
        }

        let n = pairs.len();
        if n < 2 {
            return None;
        }

        let n_f = n as f64;
        let mean_a = pairs.iter().map(|p| p.0).sum::<f64>() / n_f;
        let mean_b = pairs.iter().map(|p| p.1).sum::<f64>() / n_f;

        let (cov, var_a, var_b) = pairs.iter().fold((0.0, 0.0, 0.0), |(c, va, vb), &(a, b)| {
            let da = a - mean_a;
            let db = b - mean_b;
            (c + da * db, va + da * da, vb + db * db)
        });

        let denom = (var_a * var_b).sqrt();
        if denom == 0.0 {
            None
        } else {
            Some(cov / denom)
        }
    }

    /// Compute the z-score of the latest value for `(symbol, source)` relative to
    /// the historical distribution for that source.
    ///
    /// Returns `None` if fewer than 2 historical values exist.
    pub fn anomaly_score(&self, symbol: &str, source: &AltDataSource) -> Option<f64> {
        let k = Self::key(symbol, source);
        let history = self.history.get(&k)?;
        let latest = self.latest.get(&k)?;
        let n = history.len();
        if n < 2 {
            return None;
        }
        let mean = history.iter().sum::<f64>() / n as f64;
        let variance =
            history.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1) as f64;
        let std = variance.sqrt();
        if std == 0.0 {
            return Some(0.0);
        }
        Some((latest.value - mean) / std)
    }

    /// Return all `(symbol, source)` pairs whose latest observation is older than
    /// `max_age_hours` relative to `now` (Unix seconds).
    pub fn freshness_check(
        &self,
        max_age_hours: u8,
        now: u64,
    ) -> Vec<(String, AltDataSource)> {
        let max_age_secs = max_age_hours as u64 * 3600;
        let mut stale = Vec::new();

        for point in self.latest.values() {
            let age = now.saturating_sub(point.timestamp);
            if age > max_age_secs {
                stale.push((point.symbol.clone(), point.source.clone()));
            }
        }

        stale
    }
}

impl Default for AltDataAggregator {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_point(symbol: &str, source: AltDataSource, value: f64, timestamp: u64) -> AltDataPoint {
        AltDataPoint {
            source,
            symbol: symbol.to_string(),
            value,
            confidence: 0.9,
            timestamp,
            metadata: HashMap::new(),
        }
    }

    // ── AltDataSource ──────────────────────────────────────────────────────

    #[test]
    fn refresh_frequency_social() {
        assert_eq!(AltDataSource::SocialSentiment.refresh_frequency_hours(), 1);
    }

    #[test]
    fn refresh_frequency_patent() {
        assert_eq!(AltDataSource::PatentFilings.refresh_frequency_hours(), 168);
    }

    // ── AltDataAggregator::ingest / latest ────────────────────────────────

    #[test]
    fn latest_none_before_ingest() {
        let agg = AltDataAggregator::new();
        assert!(agg.latest("AAPL", &AltDataSource::SocialSentiment).is_none());
    }

    #[test]
    fn latest_after_ingest() {
        let mut agg = AltDataAggregator::new();
        agg.ingest(make_point("AAPL", AltDataSource::SocialSentiment, 0.7, 1_000));
        let p = agg.latest("AAPL", &AltDataSource::SocialSentiment).unwrap();
        assert!((p.value - 0.7).abs() < 1e-12);
    }

    #[test]
    fn latest_updated_on_reingest() {
        let mut agg = AltDataAggregator::new();
        agg.ingest(make_point("AAPL", AltDataSource::SocialSentiment, 0.5, 1_000));
        agg.ingest(make_point("AAPL", AltDataSource::SocialSentiment, 0.9, 2_000));
        let p = agg.latest("AAPL", &AltDataSource::SocialSentiment).unwrap();
        assert!((p.value - 0.9).abs() < 1e-12);
    }

    // ── composite_signal ──────────────────────────────────────────────────

    #[test]
    fn composite_signal_no_data() {
        let agg = AltDataAggregator::new();
        let weights = HashMap::new();
        assert_eq!(agg.composite_signal("AAPL", &weights), 0.0);
    }

    #[test]
    fn composite_signal_single_source() {
        let mut agg = AltDataAggregator::new();
        agg.ingest(make_point("AAPL", AltDataSource::SocialSentiment, 0.6, 1_000));
        let weights = HashMap::new();
        let cs = agg.composite_signal("AAPL", &weights);
        assert!((cs - 0.6).abs() < 1e-12);
    }

    #[test]
    fn composite_signal_weighted() {
        let mut agg = AltDataAggregator::new();
        agg.ingest(make_point("AAPL", AltDataSource::SocialSentiment, 0.8, 1_000));
        agg.ingest(make_point("AAPL", AltDataSource::WebTraffic, 0.4, 1_000));
        let mut weights = HashMap::new();
        weights.insert("SocialSentiment".to_string(), 2.0);
        weights.insert("WebTraffic".to_string(), 1.0);
        let cs = agg.composite_signal("AAPL", &weights);
        // (0.8*2 + 0.4*1) / 3 = 2.0/3 ≈ 0.6667
        assert!((cs - 2.0 / 3.0).abs() < 1e-12);
    }

    // ── signal_correlation ────────────────────────────────────────────────

    #[test]
    fn signal_correlation_insufficient() {
        let mut agg = AltDataAggregator::new();
        agg.ingest(make_point("AAPL", AltDataSource::SocialSentiment, 0.5, 1_000));
        assert!(agg.signal_correlation("AAPL", "GOOG").is_none());
    }

    #[test]
    fn signal_correlation_perfect_positive() {
        let mut agg = AltDataAggregator::new();
        for i in 0..5u64 {
            agg.ingest(make_point("AAPL", AltDataSource::SocialSentiment, i as f64, i));
            agg.ingest(make_point("GOOG", AltDataSource::SocialSentiment, i as f64 * 2.0, i));
        }
        let corr = agg.signal_correlation("AAPL", "GOOG").unwrap();
        assert!((corr - 1.0).abs() < 1e-10);
    }

    // ── anomaly_score ─────────────────────────────────────────────────────

    #[test]
    fn anomaly_score_none_before_ingest() {
        let agg = AltDataAggregator::new();
        assert!(agg.anomaly_score("AAPL", &AltDataSource::SocialSentiment).is_none());
    }

    #[test]
    fn anomaly_score_computed() {
        let mut agg = AltDataAggregator::new();
        // Mean = 1.0, std ~= 1.0 (sample); latest = 3.0 → z ≈ 2.0
        agg.ingest(make_point("AAPL", AltDataSource::WebTraffic, 0.0, 1));
        agg.ingest(make_point("AAPL", AltDataSource::WebTraffic, 1.0, 2));
        agg.ingest(make_point("AAPL", AltDataSource::WebTraffic, 3.0, 3));
        let z = agg.anomaly_score("AAPL", &AltDataSource::WebTraffic).unwrap();
        // mean([0,1,3])=4/3, sample std of [0,1,3]
        let vals = [0.0_f64, 1.0, 3.0];
        let mean = vals.iter().sum::<f64>() / 3.0;
        let std = (vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / 2.0).sqrt();
        let expected = (3.0 - mean) / std;
        assert!((z - expected).abs() < 1e-10);
    }

    // ── freshness_check ───────────────────────────────────────────────────

    #[test]
    fn freshness_check_empty() {
        let agg = AltDataAggregator::new();
        assert!(agg.freshness_check(24, 100_000).is_empty());
    }

    #[test]
    fn freshness_check_fresh() {
        let mut agg = AltDataAggregator::new();
        let now = 10_000_u64;
        agg.ingest(make_point("AAPL", AltDataSource::SocialSentiment, 0.5, now - 60));
        assert!(agg.freshness_check(1, now).is_empty());
    }

    #[test]
    fn freshness_check_stale() {
        let mut agg = AltDataAggregator::new();
        let now = 100_000_u64;
        // Timestamp 3600 seconds ago but max_age is 0 hours → immediately stale
        agg.ingest(make_point("AAPL", AltDataSource::SocialSentiment, 0.5, now - 3_601));
        let stale = agg.freshness_check(1, now);
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].0, "AAPL");
    }
}
