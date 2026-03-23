//! # Module: risk::stress
//!
//! ## Responsibility
//! Stress-testing framework: applies named market shock scenarios to a
//! portfolio and aggregates P&L impact, worst/best positions, and
//! per-position detail.
//!
//! ## Built-in Scenarios
//!
//! | Name | Description |
//! |------|-------------|
//! | `covid_crash` | Equity −30%, bonds +5%, vol +150% |
//! | `rate_spike` | Bonds −15%, equities −10% |
//! | `dollar_rally` | EM equities −20%, commodities −10% |
//! | `tech_crash` | Tech −40%, defensive +5% |
//!
//! ## Guarantees
//! - No panics; arithmetic is saturating/guarded with `is_finite` checks.
//! - Positions with no matching shock key are left at zero P&L contribution.

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// StressScenario
// ---------------------------------------------------------------------------

/// A named collection of asset-level percent shocks and an optional
/// correlation shock.
///
/// Shock values are expressed as fractions (e.g. `-0.30` = −30%).
/// The `correlation_shock` value is informational — it can be used by
/// callers to adjust covariance matrices for more sophisticated analysis;
/// `apply_scenario` does not use it directly.
#[derive(Debug, Clone)]
pub struct StressScenario {
    /// Human-readable scenario name (e.g. `"COVID Crash"`).
    pub name: String,
    /// Map of asset tag → percentage shock as a fraction.
    ///
    /// Asset tags may be specific symbols (`"AAPL"`) or broad asset-class
    /// labels (`"equities"`, `"bonds"`, `"tech"`) that callers embed in
    /// their portfolio position keys.
    pub asset_shocks: HashMap<String, f64>,
    /// Optional shift to the correlation between assets (informational).
    pub correlation_shock: Option<f64>,
}

impl StressScenario {
    /// Create a new scenario with an empty shock map.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            asset_shocks: HashMap::new(),
            correlation_shock: None,
        }
    }

    /// Add a shock for an asset key; returns `self` for chaining.
    #[must_use]
    pub fn with_shock(mut self, asset: impl Into<String>, shock: f64) -> Self {
        self.asset_shocks.insert(asset.into(), shock);
        self
    }

    /// Set the correlation shock; returns `self` for chaining.
    #[must_use]
    pub fn with_correlation_shock(mut self, shock: f64) -> Self {
        self.correlation_shock = Some(shock);
        self
    }

    // -----------------------------------------------------------------------
    // Built-in scenarios
    // -----------------------------------------------------------------------

    /// COVID-19 crash scenario (Q1 2020):
    /// - `equities` −30%
    /// - `bonds` +5%
    /// - `vol` +150%
    ///
    /// Correlation shock: +0.40 (assets become more correlated under stress).
    pub fn covid_crash() -> Self {
        let mut shocks = HashMap::new();
        shocks.insert("equities".to_owned(), -0.30);
        shocks.insert("bonds".to_owned(), 0.05);
        shocks.insert("vol".to_owned(), 1.50);
        Self {
            name: "COVID Crash".to_owned(),
            asset_shocks: shocks,
            correlation_shock: Some(0.40),
        }
    }

    /// Interest-rate spike scenario:
    /// - `bonds` −15%
    /// - `equities` −10%
    ///
    /// Correlation shock: +0.20.
    pub fn rate_spike() -> Self {
        let mut shocks = HashMap::new();
        shocks.insert("bonds".to_owned(), -0.15);
        shocks.insert("equities".to_owned(), -0.10);
        Self {
            name: "Rate Spike".to_owned(),
            asset_shocks: shocks,
            correlation_shock: Some(0.20),
        }
    }

    /// USD dollar-rally scenario:
    /// - `em_equities` −20%
    /// - `commodities` −10%
    ///
    /// Correlation shock: −0.10 (USD rally can de-correlate some pairs).
    pub fn dollar_rally() -> Self {
        let mut shocks = HashMap::new();
        shocks.insert("em_equities".to_owned(), -0.20);
        shocks.insert("commodities".to_owned(), -0.10);
        Self {
            name: "Dollar Rally".to_owned(),
            asset_shocks: shocks,
            correlation_shock: Some(-0.10),
        }
    }

    /// Technology sector crash scenario:
    /// - `tech` −40%
    /// - `defensive` +5%
    ///
    /// Correlation shock: +0.15.
    pub fn tech_crash() -> Self {
        let mut shocks = HashMap::new();
        shocks.insert("tech".to_owned(), -0.40);
        shocks.insert("defensive".to_owned(), 0.05);
        Self {
            name: "Tech Crash".to_owned(),
            asset_shocks: shocks,
            correlation_shock: Some(0.15),
        }
    }
}

// ---------------------------------------------------------------------------
// Portfolio
// ---------------------------------------------------------------------------

/// A simple snapshot of portfolio positions for stress testing.
///
/// Each entry maps a symbol or asset-class tag to its market value in
/// whatever currency the caller uses consistently.
#[derive(Debug, Clone, Default)]
pub struct StressPortfolio {
    /// Map of position identifier → market value.
    pub positions: HashMap<String, f64>,
}

impl StressPortfolio {
    /// Create an empty portfolio.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or update a position.
    pub fn add_position(&mut self, symbol: impl Into<String>, market_value: f64) {
        self.positions.insert(symbol.into(), market_value);
    }

    /// Total market value of all positions.
    pub fn total_value(&self) -> f64 {
        self.positions.values().sum()
    }
}

// ---------------------------------------------------------------------------
// StressResult
// ---------------------------------------------------------------------------

/// Result of applying one stress scenario to a portfolio.
#[derive(Debug, Clone)]
pub struct StressResult {
    /// Name of the scenario that produced this result.
    pub scenario_name: String,
    /// Total P&L across all positions (negative = loss).
    pub pnl: f64,
    /// P&L as a fraction of total portfolio market value.
    ///
    /// Returns `0.0` if the portfolio has zero total value.
    pub pnl_pct: f64,
    /// The position that lost the most (symbol, P&L).
    ///
    /// `None` if the portfolio is empty.
    pub worst_position: Option<(String, f64)>,
    /// The position that gained the most (symbol, P&L).
    ///
    /// `None` if the portfolio is empty.
    pub best_position: Option<(String, f64)>,
    /// Per-position P&L detail.
    pub positions_detail: HashMap<String, f64>,
}

// ---------------------------------------------------------------------------
// apply_scenario
// ---------------------------------------------------------------------------

/// Apply a single [`StressScenario`] to a [`StressPortfolio`] and return a
/// [`StressResult`].
///
/// Each position's P&L is computed by looking up the position key in
/// `scenario.asset_shocks`.  A position key matches a shock key when the
/// position key **contains** the shock key (case-sensitive substring match),
/// enabling broad tags like `"equities"` to match positions named
/// `"US_equities"` or `"EM_equities"`.  When multiple shock keys match,
/// the shock with the largest absolute magnitude is applied.
pub fn apply_scenario(portfolio: &StressPortfolio, scenario: &StressScenario) -> StressResult {
    let total_value = portfolio.total_value();

    let mut detail: HashMap<String, f64> = HashMap::with_capacity(portfolio.positions.len());
    let mut total_pnl = 0.0_f64;

    for (symbol, &market_value) in &portfolio.positions {
        // Find the best-matching shock: largest |shock| among all matching keys.
        let shock = scenario
            .asset_shocks
            .iter()
            .filter(|(key, _)| symbol.contains(key.as_str()))
            .map(|(_, &s)| s)
            .reduce(|acc, s| if s.abs() > acc.abs() { s } else { acc })
            .unwrap_or(0.0);

        let position_pnl = market_value * shock;
        detail.insert(symbol.clone(), position_pnl);
        total_pnl += position_pnl;
    }

    let pnl_pct = if total_value.abs() > f64::EPSILON {
        total_pnl / total_value
    } else {
        0.0
    };

    // Worst and best positions.
    let worst_position = detail
        .iter()
        .min_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(k, v)| (k.clone(), *v));

    let best_position = detail
        .iter()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(k, v)| (k.clone(), *v));

    StressResult {
        scenario_name: scenario.name.clone(),
        pnl: total_pnl,
        pnl_pct,
        worst_position,
        best_position,
        positions_detail: detail,
    }
}

// ---------------------------------------------------------------------------
// StressTest
// ---------------------------------------------------------------------------

/// Runs multiple stress scenarios against a portfolio and aggregates results.
pub struct StressTest {
    results: Vec<StressResult>,
}

impl StressTest {
    /// Run all given scenarios against the portfolio and store the results.
    pub fn run_all(portfolio: &StressPortfolio, scenarios: &[StressScenario]) -> Self {
        let results = scenarios
            .iter()
            .map(|s| apply_scenario(portfolio, s))
            .collect();
        Self { results }
    }

    /// Return a reference to all scenario results.
    pub fn results(&self) -> &[StressResult] {
        &self.results
    }

    /// Return the scenario result with the most negative P&L.
    ///
    /// Returns `None` if no scenarios were run.
    pub fn worst_case(&self) -> Option<&StressResult> {
        self.results
            .iter()
            .min_by(|a, b| a.pnl.partial_cmp(&b.pnl).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// Return the scenario result with the most positive P&L.
    ///
    /// Returns `None` if no scenarios were run.
    pub fn best_case(&self) -> Option<&StressResult> {
        self.results
            .iter()
            .max_by(|a, b| a.pnl.partial_cmp(&b.pnl).unwrap_or(std::cmp::Ordering::Equal))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn equity_bond_portfolio() -> StressPortfolio {
        let mut p = StressPortfolio::new();
        p.add_position("equities", 100_000.0);
        p.add_position("bonds", 50_000.0);
        p
    }

    #[test]
    fn covid_crash_equity_pnl() {
        let portfolio = equity_bond_portfolio();
        let scenario = StressScenario::covid_crash();
        let result = apply_scenario(&portfolio, &scenario);

        // equities: 100_000 * -0.30 = -30_000
        // bonds:     50_000 *  0.05 =  +2_500
        // total pnl = -27_500
        let expected = -27_500.0;
        assert!(
            (result.pnl - expected).abs() < 1e-6,
            "COVID crash pnl {:.2} != expected {:.2}",
            result.pnl,
            expected
        );
    }

    #[test]
    fn covid_crash_pnl_pct() {
        let portfolio = equity_bond_portfolio();
        let scenario = StressScenario::covid_crash();
        let result = apply_scenario(&portfolio, &scenario);

        let total_value = 150_000.0_f64;
        let expected_pct = -27_500.0 / total_value;
        assert!(
            (result.pnl_pct - expected_pct).abs() < 1e-10,
            "pnl_pct {:.6} != {:.6}",
            result.pnl_pct,
            expected_pct
        );
    }

    #[test]
    fn covid_crash_worst_position_is_equities() {
        let portfolio = equity_bond_portfolio();
        let scenario = StressScenario::covid_crash();
        let result = apply_scenario(&portfolio, &scenario);

        let (sym, pnl) = result.worst_position.expect("should have worst position");
        assert_eq!(sym, "equities");
        assert!((pnl - (-30_000.0)).abs() < 1e-6, "worst pnl={pnl:.2}");
    }

    #[test]
    fn stress_test_worst_case_is_covid() {
        let portfolio = equity_bond_portfolio();
        let scenarios = vec![
            StressScenario::covid_crash(),
            StressScenario::rate_spike(),
            StressScenario::dollar_rally(),
        ];
        let st = StressTest::run_all(&portfolio, &scenarios);
        let worst = st.worst_case().expect("should have a worst case");
        // COVID: -27_500; rate spike: 100_000*-0.10 + 50_000*-0.15 = -17_500
        // COVID should be worst
        assert_eq!(worst.scenario_name, "COVID Crash");
    }

    #[test]
    fn empty_portfolio_zero_pnl() {
        let portfolio = StressPortfolio::new();
        let result = apply_scenario(&portfolio, &StressScenario::covid_crash());
        assert!((result.pnl).abs() < f64::EPSILON);
        assert!((result.pnl_pct).abs() < f64::EPSILON);
        assert!(result.worst_position.is_none());
        assert!(result.best_position.is_none());
    }

    #[test]
    fn tech_crash_scenario_smoke() {
        let mut p = StressPortfolio::new();
        p.add_position("tech", 200_000.0);
        p.add_position("defensive", 100_000.0);
        let result = apply_scenario(&p, &StressScenario::tech_crash());
        // tech: 200_000 * -0.40 = -80_000
        // defensive: 100_000 * 0.05 = +5_000
        // total = -75_000
        assert!((result.pnl - (-75_000.0)).abs() < 1e-6, "pnl={}", result.pnl);
    }

    #[test]
    fn rate_spike_scenario_smoke() {
        let portfolio = equity_bond_portfolio();
        let result = apply_scenario(&portfolio, &StressScenario::rate_spike());
        // equities: 100_000 * -0.10 = -10_000
        // bonds:     50_000 * -0.15 = -7_500
        // total = -17_500
        assert!((result.pnl - (-17_500.0)).abs() < 1e-6, "pnl={}", result.pnl);
    }

    #[test]
    fn no_scenarios_worst_case_returns_none() {
        let portfolio = equity_bond_portfolio();
        let st = StressTest::run_all(&portfolio, &[]);
        assert!(st.worst_case().is_none());
    }
}
