//! # Module: arbitrage
//!
//! ## Responsibility
//! Cross-market and triangular arbitrage detection for financial markets.
//!
//! ## Guarantees
//! - Zero panics; all computations are pure functions returning `Option` or `Vec`
//! - `f64` is used intentionally for statistical computations (not prices)
//! - All spread values are in basis points (bps = 0.01%)

use std::collections::HashMap;

// ─── ArbitrageOpportunity ─────────────────────────────────────────────────────

/// A detected cross-market arbitrage opportunity for a single asset.
#[derive(Debug, Clone, PartialEq)]
pub struct ArbitrageOpportunity {
    /// Unique identifier for this opportunity.
    pub id: String,
    /// The asset/symbol being arbitraged.
    pub asset: String,
    /// The market to buy from (lower price).
    pub buy_market: String,
    /// The market to sell into (higher price).
    pub sell_market: String,
    /// Buy-side price.
    pub buy_price: f64,
    /// Sell-side price.
    pub sell_price: f64,
    /// Spread in basis points: (sell - buy) / buy * 10_000.
    pub spread_bps: f64,
    /// Estimated profit in USD (assumes 1 unit traded).
    pub estimated_profit_usd: f64,
    /// Confidence score in [0.0, 1.0].
    pub confidence: f64,
    /// Unix timestamp (ms) when detected.
    pub detected_at: u64,
}

// ─── ArbSignal ────────────────────────────────────────────────────────────────

/// Signal for statistical arbitrage positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArbSignal {
    /// Enter a new position.
    Enter,
    /// Exit the current position.
    Exit,
    /// Hold — no action.
    Hold,
}

// ─── TriangularArb ────────────────────────────────────────────────────────────

/// A detected triangular arbitrage opportunity across three currencies.
///
/// # Example
/// ```rust
/// use fin_primitives::arbitrage::TriangularArb;
///
/// // USD -> EUR -> GBP -> USD cycle
/// let arb = TriangularArb::detect(1.10, 1.15, 0.80);
/// // If rate_ab * rate_bc * rate_ca > 1 there is profit
/// if let Some(t) = arb {
///     assert!(t.profit_pct() > 0.0);
/// }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct TriangularArb {
    /// Base currency A.
    pub currency_a: String,
    /// Intermediate currency B.
    pub currency_b: String,
    /// Return currency C.
    pub currency_c: String,
    /// Exchange rate A -> B.
    pub rate_ab: f64,
    /// Exchange rate B -> C.
    pub rate_bc: f64,
    /// Exchange rate C -> A.
    pub rate_ca: f64,
    /// Profit percentage: (rate_ab * rate_bc * rate_ca - 1) * 100.
    pub profit_pct: f64,
}

impl TriangularArb {
    /// Detect a triangular arbitrage given three cross rates.
    ///
    /// Returns `Some` only when `rate_ab * rate_bc * rate_ca > 1.0` (i.e. profit > 0).
    pub fn detect(rate_ab: f64, rate_bc: f64, rate_ca: f64) -> Option<TriangularArb> {
        let product = rate_ab * rate_bc * rate_ca;
        if product > 1.0 {
            Some(TriangularArb {
                currency_a: "A".to_string(),
                currency_b: "B".to_string(),
                currency_c: "C".to_string(),
                rate_ab,
                rate_bc,
                rate_ca,
                profit_pct: (product - 1.0) * 100.0,
            })
        } else {
            None
        }
    }

    /// Profit percentage of this cycle: (rate_ab * rate_bc * rate_ca - 1.0) * 100.
    pub fn profit_pct(&self) -> f64 {
        (self.rate_ab * self.rate_bc * self.rate_ca - 1.0) * 100.0
    }
}

// ─── StatisticalArb ───────────────────────────────────────────────────────────

/// Statistical arbitrage pair with z-score based signalling.
#[derive(Debug, Clone, PartialEq)]
pub struct StatisticalArb {
    /// First leg symbol.
    pub symbol_a: String,
    /// Second leg symbol.
    pub symbol_b: String,
    /// Hedge ratio (units of B per unit of A).
    pub hedge_ratio: f64,
    /// Current spread value: price_a - hedge_ratio * price_b.
    pub spread: f64,
    /// Z-score of the current spread relative to its historical distribution.
    pub z_score: f64,
    /// Trading signal derived from the z-score.
    pub signal: ArbSignal,
}

// ─── ArbitrageScanner ─────────────────────────────────────────────────────────

/// Scans market data for cross-market and triangular arbitrage opportunities.
///
/// # Example
/// ```rust
/// use fin_primitives::arbitrage::ArbitrageScanner;
/// use std::collections::HashMap;
///
/// let mut markets: HashMap<String, HashMap<String, f64>> = HashMap::new();
/// let mut nyse = HashMap::new();
/// nyse.insert("AAPL".to_string(), 150.00_f64);
/// markets.insert("NYSE".to_string(), nyse);
/// let mut nasdaq = HashMap::new();
/// nasdaq.insert("AAPL".to_string(), 150.50_f64);
/// markets.insert("NASDAQ".to_string(), nasdaq);
///
/// let opps = ArbitrageScanner::scan_cross_market(&markets);
/// assert!(!opps.is_empty());
/// ```
pub struct ArbitrageScanner;

impl ArbitrageScanner {
    /// Find same-asset price discrepancies across markets.
    ///
    /// `markets` maps market name -> (symbol -> price).
    /// Returns one `ArbitrageOpportunity` per (asset, buy_market, sell_market) triple
    /// where a positive spread exists.
    pub fn scan_cross_market(
        markets: &HashMap<String, HashMap<String, f64>>,
    ) -> Vec<ArbitrageOpportunity> {
        let mut opportunities = Vec::new();

        // Collect all asset prices across markets
        let mut asset_prices: HashMap<&str, Vec<(&str, f64)>> = HashMap::new();
        for (market, symbols) in markets {
            for (symbol, &price) in symbols {
                asset_prices
                    .entry(symbol.as_str())
                    .or_default()
                    .push((market.as_str(), price));
            }
        }

        // For each asset, find the cheapest and most expensive market
        for (asset, price_list) in &asset_prices {
            if price_list.len() < 2 {
                continue;
            }

            // Find min and max price markets
            let mut min_market = price_list[0].0;
            let mut min_price = price_list[0].1;
            let mut max_market = price_list[0].0;
            let mut max_price = price_list[0].1;

            for &(market, price) in price_list {
                if price < min_price {
                    min_price = price;
                    min_market = market;
                }
                if price > max_price {
                    max_price = price;
                    max_market = market;
                }
            }

            if min_price <= 0.0 || min_market == max_market {
                continue;
            }

            let spread_bps = (max_price - min_price) / min_price * 10_000.0;
            let estimated_profit_usd = max_price - min_price;
            // Confidence grows with spread but saturates at 1.0
            let confidence = (spread_bps / 100.0).min(1.0);

            opportunities.push(ArbitrageOpportunity {
                id: format!("{}-{}-{}", asset, min_market, max_market),
                asset: (*asset).to_string(),
                buy_market: min_market.to_string(),
                sell_market: max_market.to_string(),
                buy_price: min_price,
                sell_price: max_price,
                spread_bps,
                estimated_profit_usd,
                confidence,
                detected_at: 0,
            });
        }

        opportunities
    }

    /// Scan for triangular arbitrage given a map of "A/B" -> rate.
    ///
    /// Enumerates all currency triples present in the rate map and returns
    /// those with a product > 1.0 (profitable cycle).
    pub fn scan_triangular(rates: &HashMap<String, f64>) -> Vec<TriangularArb> {
        // Parse all currencies from pair keys
        let mut currencies: Vec<String> = Vec::new();
        for key in rates.keys() {
            let parts: Vec<&str> = key.split('/').collect();
            if parts.len() == 2 {
                let a = parts[0].to_string();
                let b = parts[1].to_string();
                if !currencies.contains(&a) {
                    currencies.push(a);
                }
                if !currencies.contains(&b) {
                    currencies.push(b);
                }
            }
        }

        let mut results = Vec::new();
        let n = currencies.len();

        // Enumerate all triples (i, j, k)
        for i in 0..n {
            for j in 0..n {
                if j == i {
                    continue;
                }
                for k in 0..n {
                    if k == i || k == j {
                        continue;
                    }
                    let ca = &currencies[i];
                    let cb = &currencies[j];
                    let cc = &currencies[k];

                    let key_ab = format!("{}/{}", ca, cb);
                    let key_bc = format!("{}/{}", cb, cc);
                    let key_ca = format!("{}/{}", cc, ca);

                    if let (Some(&rate_ab), Some(&rate_bc), Some(&rate_ca)) = (
                        rates.get(&key_ab),
                        rates.get(&key_bc),
                        rates.get(&key_ca),
                    ) {
                        let product = rate_ab * rate_bc * rate_ca;
                        if product > 1.0 {
                            results.push(TriangularArb {
                                currency_a: ca.clone(),
                                currency_b: cb.clone(),
                                currency_c: cc.clone(),
                                rate_ab,
                                rate_bc,
                                rate_ca,
                                profit_pct: (product - 1.0) * 100.0,
                            });
                        }
                    }
                }
            }
        }

        results
    }

    /// Filter opportunities to only those with spread >= `min_bps`.
    pub fn filter_by_min_profit(
        opps: Vec<ArbitrageOpportunity>,
        min_bps: f64,
    ) -> Vec<ArbitrageOpportunity> {
        opps.into_iter()
            .filter(|o| o.spread_bps >= min_bps)
            .collect()
    }

    /// Sort opportunities descending by confidence score (in-place).
    pub fn rank_by_confidence(opps: &mut Vec<ArbitrageOpportunity>) {
        opps.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn markets() -> HashMap<String, HashMap<String, f64>> {
        let mut m: HashMap<String, HashMap<String, f64>> = HashMap::new();
        let mut nyse = HashMap::new();
        nyse.insert("AAPL".to_string(), 150.00_f64);
        nyse.insert("GOOG".to_string(), 2800.00_f64);
        m.insert("NYSE".to_string(), nyse);

        let mut nasdaq = HashMap::new();
        nasdaq.insert("AAPL".to_string(), 150.30_f64);
        nasdaq.insert("GOOG".to_string(), 2800.00_f64);
        m.insert("NASDAQ".to_string(), nasdaq);

        m
    }

    #[test]
    fn test_triangular_arb_detect_profitable() {
        // 1.10 * 1.20 * 0.80 = 1.056 > 1 -> profitable
        let arb = TriangularArb::detect(1.10, 1.20, 0.80);
        assert!(arb.is_some());
        let arb = arb.unwrap();
        assert!(arb.profit_pct() > 0.0);
        let expected = (1.10 * 1.20 * 0.80 - 1.0) * 100.0;
        assert!((arb.profit_pct() - expected).abs() < 1e-9);
    }

    #[test]
    fn test_triangular_arb_detect_not_profitable() {
        // 1.0 * 1.0 * 0.9 = 0.9 < 1 -> no arbitrage
        let arb = TriangularArb::detect(1.0, 1.0, 0.9);
        assert!(arb.is_none());
    }

    #[test]
    fn test_triangular_arb_profit_pct_formula() {
        let arb = TriangularArb {
            currency_a: "USD".to_string(),
            currency_b: "EUR".to_string(),
            currency_c: "GBP".to_string(),
            rate_ab: 1.1,
            rate_bc: 1.15,
            rate_ca: 0.80,
            profit_pct: 0.0,
        };
        let expected = (1.1 * 1.15 * 0.80 - 1.0) * 100.0;
        assert!((arb.profit_pct() - expected).abs() < 1e-9);
    }

    #[test]
    fn test_scan_cross_market_finds_aapl() {
        let markets = markets();
        let opps = ArbitrageScanner::scan_cross_market(&markets);
        assert!(!opps.is_empty());
        let aapl_opp = opps.iter().find(|o| o.asset == "AAPL");
        assert!(aapl_opp.is_some());
        let o = aapl_opp.unwrap();
        assert_eq!(o.buy_market, "NYSE");
        assert_eq!(o.sell_market, "NASDAQ");
        assert!((o.buy_price - 150.00).abs() < 1e-9);
        assert!((o.sell_price - 150.30).abs() < 1e-9);
        // spread_bps = 0.30 / 150.00 * 10000 = 20 bps
        assert!((o.spread_bps - 20.0).abs() < 1e-6);
    }

    #[test]
    fn test_scan_cross_market_no_arb_same_price() {
        let markets = markets();
        let opps = ArbitrageScanner::scan_cross_market(&markets);
        // GOOG has same price on both exchanges, no arb
        let goog_opp = opps.iter().find(|o| o.asset == "GOOG");
        assert!(goog_opp.is_none());
    }

    #[test]
    fn test_filter_by_min_profit() {
        let markets = markets();
        let opps = ArbitrageScanner::scan_cross_market(&markets);
        // AAPL spread is 20 bps; filter to > 50 bps should remove it
        let filtered = ArbitrageScanner::filter_by_min_profit(opps, 50.0);
        assert!(filtered.is_empty());
    }

    #[test]
    fn test_rank_by_confidence() {
        let mut opps = vec![
            ArbitrageOpportunity {
                id: "1".to_string(),
                asset: "A".to_string(),
                buy_market: "M1".to_string(),
                sell_market: "M2".to_string(),
                buy_price: 100.0,
                sell_price: 101.0,
                spread_bps: 100.0,
                estimated_profit_usd: 1.0,
                confidence: 0.3,
                detected_at: 0,
            },
            ArbitrageOpportunity {
                id: "2".to_string(),
                asset: "B".to_string(),
                buy_market: "M1".to_string(),
                sell_market: "M2".to_string(),
                buy_price: 100.0,
                sell_price: 102.0,
                spread_bps: 200.0,
                estimated_profit_usd: 2.0,
                confidence: 0.9,
                detected_at: 0,
            },
        ];
        ArbitrageScanner::rank_by_confidence(&mut opps);
        assert_eq!(opps[0].id, "2");
        assert_eq!(opps[1].id, "1");
    }

    #[test]
    fn test_scan_triangular() {
        let mut rates = HashMap::new();
        // USD -> EUR -> GBP -> USD cycle with profit
        rates.insert("USD/EUR".to_string(), 0.91);
        rates.insert("EUR/GBP".to_string(), 0.86);
        rates.insert("GBP/USD".to_string(), 1.30); // product: 0.91*0.86*1.30 = 1.01738 > 1
        let results = ArbitrageScanner::scan_triangular(&rates);
        // Should find at least one profitable triple
        assert!(!results.is_empty());
        for r in &results {
            assert!(r.profit_pct() > 0.0);
        }
    }

    #[test]
    fn test_scan_triangular_no_arb() {
        let mut rates = HashMap::new();
        rates.insert("USD/EUR".to_string(), 0.91);
        rates.insert("EUR/GBP".to_string(), 0.86);
        rates.insert("GBP/USD".to_string(), 1.10); // product: 0.91*0.86*1.10 = 0.860 < 1
        let results = ArbitrageScanner::scan_triangular(&rates);
        assert!(results.is_empty());
    }

    #[test]
    fn test_statistical_arb_fields() {
        let sa = StatisticalArb {
            symbol_a: "SPY".to_string(),
            symbol_b: "IVV".to_string(),
            hedge_ratio: 1.02,
            spread: 0.5,
            z_score: 2.1,
            signal: ArbSignal::Enter,
        };
        assert_eq!(sa.signal, ArbSignal::Enter);
        assert!((sa.z_score - 2.1).abs() < 1e-9);
    }
}
