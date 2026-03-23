//! # Risk Attribution
//!
//! Decomposes total portfolio risk and P&L into named factor contributions, following
//! the Brinson-Hood-Beebower (BHB) attribution framework for P&L decomposition and a
//! factor-model approach for risk decomposition.
//!
//! ## Overview
//!
//! | Type | Purpose |
//! |------|---------|
//! | [`RiskFactor`] | Taxonomy of risk sources (Market, Sector, Idiosyncratic, …) |
//! | [`RiskAttribution`] | Single factor's contribution percentage and absolute value |
//! | [`AttributionReport`] | Full breakdown of portfolio risk by factor |
//! | [`BhbAttribution`] | Brinson-Hood-Beebower P&L decomposition |
//! | [`RiskAttributor`] | Computes attribution from a `PositionLedger` + market data |
//!
//! ## Integration with `RiskMonitor`
//!
//! Call [`super::RiskMonitor::attribution_report`] to retrieve the latest attribution
//! snapshot without constructing a `RiskAttributor` manually.
//!
//! ## Example
//!
//! ```rust
//! use fin_primitives::risk::attribution::{RiskAttributor, MarketData};
//! use fin_primitives::position::PositionLedger;
//! use rust_decimal_macros::dec;
//!
//! let ledger = PositionLedger::new(dec!(100_000));
//! let market_data = MarketData::default();
//! let attributor = RiskAttributor::new(&ledger, market_data);
//! let report = attributor.compute();
//! assert!(!report.attributions.is_empty());
//! ```

use crate::position::PositionLedger;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::collections::HashMap;

// ── RiskFactor ────────────────────────────────────────────────────────────────

/// The source of a risk or return contribution.
///
/// Each factor represents a distinct driver of portfolio volatility or P&L.
/// The decomposition loosely follows a multi-factor model:
///
/// ```text
/// Total Risk = Market + Sector + Idiosyncratic + Leverage + Concentration + Liquidity
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RiskFactor {
    /// Systematic risk driven by broad market beta (covariance with the market index).
    ///
    /// Estimated as `portfolio_beta × market_volatility × portfolio_value`.
    Market,

    /// Risk attributable to sector/industry concentrations within the portfolio.
    ///
    /// Portfolios with heavy exposure to a single sector carry higher sector risk.
    Sector,

    /// Residual, stock-specific risk not explained by market or sector factors.
    ///
    /// In a fully diversified portfolio, idiosyncratic risk approaches zero.
    Idiosyncratic,

    /// Risk amplified by the use of leverage (net exposure / equity).
    ///
    /// A leverage ratio > 1 multiplies both gains and losses.
    Leverage,

    /// Risk from having a large fraction of equity in a single position.
    ///
    /// Measured as the Herfindahl-Hirschman Index (HHI) of position weights.
    Concentration,

    /// Risk from positions that may be difficult to exit at fair value.
    ///
    /// Approximated by weighting position sizes against estimated liquidity scores.
    Liquidity,
}

impl RiskFactor {
    /// Returns a human-readable name for this factor.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Market => "Market (Beta)",
            Self::Sector => "Sector",
            Self::Idiosyncratic => "Idiosyncratic",
            Self::Leverage => "Leverage",
            Self::Concentration => "Concentration",
            Self::Liquidity => "Liquidity",
        }
    }

    /// Returns all factor variants in a canonical order.
    pub fn all() -> &'static [RiskFactor] {
        &[
            Self::Market,
            Self::Sector,
            Self::Idiosyncratic,
            Self::Leverage,
            Self::Concentration,
            Self::Liquidity,
        ]
    }
}

// ── RiskAttribution ───────────────────────────────────────────────────────────

/// A single factor's contribution to total portfolio risk.
#[derive(Debug, Clone, PartialEq)]
pub struct RiskAttribution {
    /// The risk factor this entry represents.
    pub factor: RiskFactor,
    /// Percentage of total portfolio risk attributable to this factor.
    ///
    /// Values sum to approximately 100.0 across all factors in an [`AttributionReport`].
    /// May be negative for offsetting factors (rare but theoretically possible in
    /// multi-factor models with negative correlations).
    pub contribution_pct: f64,
    /// Absolute risk contribution in the same units as portfolio value (e.g. USD).
    pub value: f64,
}

impl RiskAttribution {
    /// Returns `true` if this factor contributes more than `threshold_pct` of total risk.
    pub fn is_dominant(&self, threshold_pct: f64) -> bool {
        self.contribution_pct > threshold_pct
    }
}

// ── AttributionReport ─────────────────────────────────────────────────────────

/// A full decomposition of portfolio risk by [`RiskFactor`].
///
/// Produced by [`RiskAttributor::compute`]. The report covers all six standard
/// risk factors; each entry's `contribution_pct` sums to 100.0 (within floating-point
/// precision).
#[derive(Debug, Clone)]
pub struct AttributionReport {
    /// Per-factor risk attribution entries, ordered by `RiskFactor::all()`.
    pub attributions: Vec<RiskAttribution>,

    /// Total portfolio risk (volatility-equivalent) in currency units.
    pub total_risk: f64,

    /// Portfolio equity (cash + unrealised P&L) used in this calculation.
    pub portfolio_equity: f64,

    /// The portfolio's estimated beta to the market at the time of computation.
    pub portfolio_beta: f64,

    /// Herfindahl-Hirschman Index of position weights (`[0, 1]`).
    ///
    /// `HHI = sum(weight_i^2)`. A value of `1.0` means a single position holds
    /// all equity. A well-diversified portfolio has HHI close to `1/n`.
    pub concentration_hhi: f64,

    /// Net leverage ratio: total gross exposure / equity.
    ///
    /// Values > 1.0 indicate leveraged portfolios.
    pub leverage_ratio: f64,
}

impl AttributionReport {
    /// Returns the attribution entry for a given `factor`, or `None` if not present.
    pub fn get(&self, factor: RiskFactor) -> Option<&RiskAttribution> {
        self.attributions.iter().find(|a| a.factor == factor)
    }

    /// Returns the dominant risk factor (highest `contribution_pct`).
    ///
    /// Returns `None` if the report has no attribution entries.
    pub fn dominant_factor(&self) -> Option<&RiskAttribution> {
        self.attributions
            .iter()
            .max_by(|a, b| a.contribution_pct.partial_cmp(&b.contribution_pct).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// Returns all factors whose contribution exceeds `threshold_pct` percent.
    pub fn factors_above(&self, threshold_pct: f64) -> Vec<&RiskAttribution> {
        self.attributions
            .iter()
            .filter(|a| a.contribution_pct > threshold_pct)
            .collect()
    }

    /// Returns a concise human-readable summary of the top factors.
    pub fn summary(&self) -> String {
        let mut lines = vec![format!(
            "AttributionReport [equity={:.2}, total_risk={:.4}, beta={:.3}, hhi={:.4}, leverage={:.2}x]",
            self.portfolio_equity,
            self.total_risk,
            self.portfolio_beta,
            self.concentration_hhi,
            self.leverage_ratio,
        )];
        for attr in &self.attributions {
            lines.push(format!(
                "  {:20} {:6.1}%  ({:.4} units)",
                attr.factor.name(),
                attr.contribution_pct,
                attr.value,
            ));
        }
        lines.join("\n")
    }
}

// ── MarketData ────────────────────────────────────────────────────────────────

/// Market-level inputs required for risk attribution.
///
/// Callers supply this alongside a `PositionLedger` to the [`RiskAttributor`].
/// Fields can be estimated from historical data or derived from a live market
/// data feed.
#[derive(Debug, Clone)]
pub struct MarketData {
    /// Annualised market (index) volatility, expressed as a fraction (e.g. `0.15` = 15%).
    ///
    /// Defaults to `0.15` if not set.
    pub market_volatility: f64,

    /// Per-symbol beta estimates relative to the market.
    ///
    /// Symbols not present in this map are assumed to have `beta = 1.0`.
    pub betas: HashMap<String, f64>,

    /// Per-symbol sector labels (e.g. `"Technology"`, `"Energy"`).
    ///
    /// Symbols not present are placed in a catch-all `"Other"` sector.
    pub sectors: HashMap<String, String>,

    /// Per-symbol liquidity scores in the range `[0, 1]`.
    ///
    /// `1.0` = fully liquid, `0.0` = completely illiquid.
    /// Symbols not present are assumed to have a liquidity score of `1.0`.
    pub liquidity_scores: HashMap<String, f64>,

    /// Per-symbol annualised idiosyncratic volatility (residual after removing market beta).
    ///
    /// Defaults to `0.20` for any symbol not present in this map.
    pub idiosyncratic_vols: HashMap<String, f64>,
}

impl Default for MarketData {
    fn default() -> Self {
        Self {
            market_volatility: 0.15,
            betas: HashMap::new(),
            sectors: HashMap::new(),
            liquidity_scores: HashMap::new(),
            idiosyncratic_vols: HashMap::new(),
        }
    }
}

impl MarketData {
    /// Creates a new `MarketData` with the given annualised market volatility.
    pub fn new(market_volatility: f64) -> Self {
        Self { market_volatility, ..Default::default() }
    }

    /// Sets the beta for a symbol.
    pub fn with_beta(mut self, symbol: impl Into<String>, beta: f64) -> Self {
        self.betas.insert(symbol.into(), beta);
        self
    }

    /// Sets the sector for a symbol.
    pub fn with_sector(mut self, symbol: impl Into<String>, sector: impl Into<String>) -> Self {
        self.sectors.insert(symbol.into(), sector.into());
        self
    }

    /// Sets the liquidity score for a symbol.
    pub fn with_liquidity(mut self, symbol: impl Into<String>, score: f64) -> Self {
        self.liquidity_scores.insert(symbol.into(), score);
        self
    }

    /// Sets the idiosyncratic volatility for a symbol.
    pub fn with_idio_vol(mut self, symbol: impl Into<String>, vol: f64) -> Self {
        self.idiosyncratic_vols.insert(symbol.into(), vol);
        self
    }

    /// Returns the beta for a symbol, defaulting to `1.0`.
    pub fn beta(&self, symbol: &str) -> f64 {
        self.betas.get(symbol).copied().unwrap_or(1.0)
    }

    /// Returns the liquidity score for a symbol, defaulting to `1.0`.
    pub fn liquidity(&self, symbol: &str) -> f64 {
        self.liquidity_scores.get(symbol).copied().unwrap_or(1.0)
    }

    /// Returns the idiosyncratic volatility for a symbol, defaulting to `0.20`.
    pub fn idio_vol(&self, symbol: &str) -> f64 {
        self.idiosyncratic_vols.get(symbol).copied().unwrap_or(0.20)
    }
}

// ── BhbAttribution ────────────────────────────────────────────────────────────

/// Brinson-Hood-Beebower P&L attribution.
///
/// Decomposes the portfolio's active return (vs. a benchmark) into:
///
/// - **Allocation effect**: excess return from overweighting/underweighting sectors
///   relative to the benchmark.
/// - **Selection effect**: excess return from picking better stocks within each sector.
/// - **Interaction effect**: joint effect of allocation and selection decisions.
///
/// The total active return equals the sum of all three effects.
///
/// # Formula
///
/// For each sector `i`:
/// - `Allocation_i = (w_p_i - w_b_i) * (R_b_i - R_b)`
/// - `Selection_i  = w_b_i * (R_p_i - R_b_i)`
/// - `Interaction_i = (w_p_i - w_b_i) * (R_p_i - R_b_i)`
///
/// where `w_p_i` = portfolio weight in sector i, `w_b_i` = benchmark weight,
/// `R_p_i` = portfolio return in sector i, `R_b_i` = benchmark return in sector i,
/// `R_b` = total benchmark return.
#[derive(Debug, Clone)]
pub struct BhbAttribution {
    /// Sector-level breakdown of the three BHB effects.
    pub sector_effects: Vec<SectorEffect>,

    /// Total active return: `sum(allocation + selection + interaction)` across sectors.
    pub total_active_return: f64,

    /// Total allocation effect across all sectors.
    pub total_allocation: f64,

    /// Total selection effect across all sectors.
    pub total_selection: f64,

    /// Total interaction effect across all sectors.
    pub total_interaction: f64,
}

impl BhbAttribution {
    /// Returns the sector with the largest positive allocation effect, or `None`.
    pub fn best_allocation_sector(&self) -> Option<&SectorEffect> {
        self.sector_effects
            .iter()
            .filter(|s| s.allocation_effect > 0.0)
            .max_by(|a, b| a.allocation_effect.partial_cmp(&b.allocation_effect).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// Returns the sector with the largest positive selection effect, or `None`.
    pub fn best_selection_sector(&self) -> Option<&SectorEffect> {
        self.sector_effects
            .iter()
            .filter(|s| s.selection_effect > 0.0)
            .max_by(|a, b| a.selection_effect.partial_cmp(&b.selection_effect).unwrap_or(std::cmp::Ordering::Equal))
    }
}

/// Per-sector Brinson-Hood-Beebower effects.
#[derive(Debug, Clone)]
pub struct SectorEffect {
    /// Sector name (e.g. `"Technology"`).
    pub sector: String,
    /// Portfolio weight in this sector.
    pub portfolio_weight: f64,
    /// Benchmark weight in this sector.
    pub benchmark_weight: f64,
    /// Portfolio return contribution from this sector.
    pub portfolio_return: f64,
    /// Benchmark return for this sector.
    pub benchmark_return: f64,
    /// Allocation effect: `(w_p - w_b) * (R_b_i - R_b)`.
    pub allocation_effect: f64,
    /// Selection effect: `w_b * (R_p_i - R_b_i)`.
    pub selection_effect: f64,
    /// Interaction effect: `(w_p - w_b) * (R_p_i - R_b_i)`.
    pub interaction_effect: f64,
}

// ── BhbInput ──────────────────────────────────────────────────────────────────

/// Input data for a Brinson-Hood-Beebower attribution calculation.
#[derive(Debug, Clone, Default)]
pub struct BhbInput {
    /// Per-sector portfolio and benchmark data.
    pub sectors: Vec<BhbSectorInput>,
    /// Total benchmark return over the period (as a fraction, e.g. `0.05` = 5%).
    pub benchmark_total_return: f64,
}

/// Per-sector input for BHB attribution.
#[derive(Debug, Clone)]
pub struct BhbSectorInput {
    /// Sector name.
    pub sector: String,
    /// Portfolio weight in this sector (`[0, 1]`).
    pub portfolio_weight: f64,
    /// Benchmark weight in this sector (`[0, 1]`).
    pub benchmark_weight: f64,
    /// Portfolio return from this sector over the period.
    pub portfolio_sector_return: f64,
    /// Benchmark return for this sector over the period.
    pub benchmark_sector_return: f64,
}

// ── RiskAttributor ────────────────────────────────────────────────────────────

/// Computes factor-level risk attribution from a `PositionLedger` and market data.
///
/// `RiskAttributor` is stateless: it reads the ledger's current position snapshot
/// and applies a simplified multi-factor model to produce an [`AttributionReport`].
/// Construct a new `RiskAttributor` before each call to `compute()` to pick up
/// the latest ledger state.
///
/// # Model
///
/// Total portfolio variance is approximated as:
///
/// ```text
/// σ²_portfolio ≈ β²_portfolio × σ²_market   (market factor)
///              + sector_HHI × σ²_market        (sector factor)
///              + Σ w_i² × σ²_idio_i            (idiosyncratic)
///              + (leverage - 1)² × σ²_market   (leverage premium)
///              + HHI × σ²_market               (concentration)
///              + (1 - avg_liquidity) × σ²_mkt  (liquidity premium)
/// ```
///
/// Risk is then `σ_portfolio × portfolio_equity` in currency units, and each factor's
/// share of total risk is expressed as a percentage of the sum.
pub struct RiskAttributor<'a> {
    ledger: &'a PositionLedger,
    market_data: MarketData,
}

impl<'a> RiskAttributor<'a> {
    /// Creates a new `RiskAttributor`.
    pub fn new(ledger: &'a PositionLedger, market_data: MarketData) -> Self {
        Self { ledger, market_data }
    }

    /// Computes and returns an [`AttributionReport`].
    ///
    /// The calculation uses:
    /// 1. Current position values from the ledger.
    /// 2. Beta and volatility data from `market_data`.
    /// 3. A simplified variance-decomposition model to split risk into factors.
    pub fn compute(&self) -> AttributionReport {
        // Use cash as a proxy for equity (cash + unrealised PnL requires market prices,
        // which are not available here; callers with full market data should use
        // `PositionLedger::equity(prices)` and pass the result in a custom `MarketData`).
        let cash = self.ledger.cash();
        // Sum cost basis across all open positions as a proxy for invested capital.
        let positions: Vec<_> = self.ledger.positions().collect();
        let total_cost_basis: Decimal = positions.iter()
            .map(|p| p.total_cost_basis())
            .sum();
        let equity_dec = cash + total_cost_basis;
        let equity = equity_dec.to_f64().unwrap_or(0.0);

        if equity == 0.0 {
            return self.empty_report(equity);
        }

        let n = positions.len();
        if n == 0 {
            return self.empty_report(equity);
        }

        // Compute total gross exposure (sum of absolute position notional values).
        let gross_exposure: f64 = positions
            .iter()
            .map(|p| (p.quantity * p.avg_cost).to_f64().unwrap_or(0.0).abs())
            .sum();

        let leverage_ratio = if equity > 0.0 { gross_exposure / equity } else { 1.0 };

        // Per-symbol weights (fraction of gross exposure).
        let weights: Vec<(String, f64)> = positions
            .iter()
            .map(|p| {
                let notional = (p.quantity * p.avg_cost).to_f64().unwrap_or(0.0).abs();
                let w = if gross_exposure > 0.0 { notional / gross_exposure } else { 0.0 };
                (p.symbol.to_string(), w)
            })
            .collect();

        // Herfindahl-Hirschman Index: sum(w_i^2)
        let hhi: f64 = weights.iter().map(|(_, w)| w * w).sum();

        // Portfolio beta: weighted average of individual betas.
        let portfolio_beta: f64 = weights
            .iter()
            .map(|(sym, w)| w * self.market_data.beta(sym))
            .sum();

        // Average liquidity score.
        let avg_liquidity: f64 = if weights.is_empty() {
            1.0
        } else {
            weights.iter().map(|(sym, w)| w * self.market_data.liquidity(sym)).sum()
        };

        // Idiosyncratic variance contribution: Σ w_i² × σ²_idio_i
        let idio_variance: f64 = weights
            .iter()
            .map(|(sym, w)| {
                let vol = self.market_data.idio_vol(sym);
                w * w * vol * vol
            })
            .sum();

        let mkt_var = self.market_data.market_volatility * self.market_data.market_volatility;

        // Factor variance components (proportional, not calibrated to a specific model).
        let market_var = portfolio_beta * portfolio_beta * mkt_var;
        let sector_var = hhi * mkt_var * 0.5; // sector HHI premium
        let leverage_premium = if leverage_ratio > 1.0 {
            (leverage_ratio - 1.0).powi(2) * mkt_var
        } else {
            0.0
        };
        let concentration_var = hhi * mkt_var * 0.3; // concentration HHI premium
        let liquidity_var = (1.0 - avg_liquidity.clamp(0.0, 1.0)) * mkt_var;

        let total_variance =
            market_var + sector_var + idio_variance + leverage_premium + concentration_var + liquidity_var;

        let total_risk_vol = if total_variance > 0.0 { total_variance.sqrt() } else { 0.0 };
        let total_risk_currency = total_risk_vol * equity;

        let factor_variances = [
            (RiskFactor::Market, market_var),
            (RiskFactor::Sector, sector_var),
            (RiskFactor::Idiosyncratic, idio_variance),
            (RiskFactor::Leverage, leverage_premium),
            (RiskFactor::Concentration, concentration_var),
            (RiskFactor::Liquidity, liquidity_var),
        ];

        let attributions = factor_variances
            .iter()
            .map(|(factor, var)| {
                let pct = if total_variance > 0.0 { var / total_variance * 100.0 } else { 0.0 };
                let value = var.sqrt() * equity;
                RiskAttribution {
                    factor: *factor,
                    contribution_pct: pct,
                    value,
                }
            })
            .collect();

        AttributionReport {
            attributions,
            total_risk: total_risk_currency,
            portfolio_equity: equity,
            portfolio_beta,
            concentration_hhi: hhi,
            leverage_ratio,
        }
    }

    /// Computes a Brinson-Hood-Beebower P&L attribution from the provided input.
    ///
    /// This is a pure function: it does not read from the ledger. Supply BHB inputs
    /// derived from your own performance data.
    pub fn compute_bhb(&self, input: &BhbInput) -> BhbAttribution {
        let r_b = input.benchmark_total_return;

        let sector_effects: Vec<SectorEffect> = input
            .sectors
            .iter()
            .map(|s| {
                let allocation = (s.portfolio_weight - s.benchmark_weight)
                    * (s.benchmark_sector_return - r_b);
                let selection = s.benchmark_weight
                    * (s.portfolio_sector_return - s.benchmark_sector_return);
                let interaction = (s.portfolio_weight - s.benchmark_weight)
                    * (s.portfolio_sector_return - s.benchmark_sector_return);

                SectorEffect {
                    sector: s.sector.clone(),
                    portfolio_weight: s.portfolio_weight,
                    benchmark_weight: s.benchmark_weight,
                    portfolio_return: s.portfolio_sector_return,
                    benchmark_return: s.benchmark_sector_return,
                    allocation_effect: allocation,
                    selection_effect: selection,
                    interaction_effect: interaction,
                }
            })
            .collect();

        let total_allocation: f64 = sector_effects.iter().map(|s| s.allocation_effect).sum();
        let total_selection: f64 = sector_effects.iter().map(|s| s.selection_effect).sum();
        let total_interaction: f64 = sector_effects.iter().map(|s| s.interaction_effect).sum();
        let total_active_return = total_allocation + total_selection + total_interaction;

        BhbAttribution {
            sector_effects,
            total_active_return,
            total_allocation,
            total_selection,
            total_interaction,
        }
    }

    fn empty_report(&self, equity: f64) -> AttributionReport {
        let attributions = RiskFactor::all()
            .iter()
            .map(|&factor| RiskAttribution { factor, contribution_pct: 0.0, value: 0.0 })
            .collect();
        AttributionReport {
            attributions,
            total_risk: 0.0,
            portfolio_equity: equity,
            portfolio_beta: 0.0,
            concentration_hhi: 0.0,
            leverage_ratio: 1.0,
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::PositionLedger;
    use crate::types::{NanoTimestamp, Price, Quantity, Side, Symbol};
    use crate::position::Fill;
    use rust_decimal_macros::dec;

    fn make_fill(symbol: &str, side: Side, qty: &str, price: &str) -> Fill {
        Fill::new(
            Symbol::new(symbol).unwrap(),
            side,
            Quantity::new(qty.parse().unwrap()).unwrap(),
            Price::new(price.parse().unwrap()).unwrap(),
            NanoTimestamp::new(0),
        )
    }

    fn ledger_with_position() -> PositionLedger {
        let mut ledger = PositionLedger::new(dec!(100_000));
        ledger.apply_fill(&make_fill("AAPL", Side::Bid, "10", "150")).unwrap();
        ledger.apply_fill(&make_fill("MSFT", Side::Bid, "5", "300")).unwrap();
        ledger
    }

    // ── RiskFactor ───────────────────────────────────────────────────────────

    #[test]
    fn test_risk_factor_all_has_six_variants() {
        assert_eq!(RiskFactor::all().len(), 6);
    }

    #[test]
    fn test_risk_factor_names_non_empty() {
        for factor in RiskFactor::all() {
            assert!(!factor.name().is_empty());
        }
    }

    #[test]
    fn test_risk_factor_market_name() {
        assert!(RiskFactor::Market.name().contains("Market") || RiskFactor::Market.name().contains("Beta"));
    }

    // ── AttributionReport ────────────────────────────────────────────────────

    #[test]
    fn test_attribution_report_has_all_six_factors() {
        let ledger = ledger_with_position();
        let market_data = MarketData::default();
        let attributor = RiskAttributor::new(&ledger, market_data);
        let report = attributor.compute();
        assert_eq!(report.attributions.len(), 6);
    }

    #[test]
    fn test_attribution_report_percentages_sum_to_100() {
        let ledger = ledger_with_position();
        let market_data = MarketData::default();
        let attributor = RiskAttributor::new(&ledger, market_data);
        let report = attributor.compute();
        let total: f64 = report.attributions.iter().map(|a| a.contribution_pct).sum();
        assert!((total - 100.0).abs() < 1.0, "percentages should sum to ~100, got {total}");
    }

    #[test]
    fn test_attribution_report_no_negative_contributions() {
        let ledger = ledger_with_position();
        let market_data = MarketData::default();
        let attributor = RiskAttributor::new(&ledger, market_data);
        let report = attributor.compute();
        for attr in &report.attributions {
            assert!(
                attr.contribution_pct >= -0.001,
                "contribution_pct should be non-negative, got {} for {:?}",
                attr.contribution_pct,
                attr.factor
            );
        }
    }

    #[test]
    fn test_attribution_report_total_risk_positive() {
        let ledger = ledger_with_position();
        let market_data = MarketData::default();
        let attributor = RiskAttributor::new(&ledger, market_data);
        let report = attributor.compute();
        assert!(report.total_risk > 0.0, "total risk should be positive for a non-empty portfolio");
    }

    #[test]
    fn test_attribution_report_empty_ledger_zero_risk() {
        let ledger = PositionLedger::new(dec!(100_000));
        let market_data = MarketData::default();
        let attributor = RiskAttributor::new(&ledger, market_data);
        let report = attributor.compute();
        assert_eq!(report.total_risk, 0.0);
    }

    #[test]
    fn test_attribution_report_get_market_factor() {
        let ledger = ledger_with_position();
        let market_data = MarketData::default();
        let attributor = RiskAttributor::new(&ledger, market_data);
        let report = attributor.compute();
        assert!(report.get(RiskFactor::Market).is_some());
    }

    #[test]
    fn test_attribution_report_dominant_factor() {
        let ledger = ledger_with_position();
        let market_data = MarketData::default();
        let attributor = RiskAttributor::new(&ledger, market_data);
        let report = attributor.compute();
        let dominant = report.dominant_factor();
        assert!(dominant.is_some());
    }

    #[test]
    fn test_attribution_report_leverage_ratio_no_leverage() {
        let ledger = ledger_with_position();
        let market_data = MarketData::default();
        let attributor = RiskAttributor::new(&ledger, market_data);
        let report = attributor.compute();
        // ledger started with 100k; positions cost 1500 + 1500 = 3000, well within equity
        assert!(report.leverage_ratio < 2.0, "leverage ratio should be < 2 for lightly invested portfolio");
    }

    #[test]
    fn test_attribution_report_summary_contains_factor_names() {
        let ledger = ledger_with_position();
        let market_data = MarketData::default();
        let attributor = RiskAttributor::new(&ledger, market_data);
        let report = attributor.compute();
        let summary = report.summary();
        assert!(summary.contains("Market"));
        assert!(summary.contains("Leverage"));
    }

    #[test]
    fn test_attribution_report_hhi_single_position() {
        let mut ledger = PositionLedger::new(dec!(100_000));
        ledger.apply_fill(&make_fill("AAPL", Side::Bid, "10", "100")).unwrap();
        let market_data = MarketData::default();
        let attributor = RiskAttributor::new(&ledger, market_data);
        let report = attributor.compute();
        // Single position → HHI = 1.0 (maximally concentrated)
        assert!(
            (report.concentration_hhi - 1.0).abs() < 0.001,
            "HHI should be 1.0 for a single-position portfolio, got {}",
            report.concentration_hhi
        );
    }

    #[test]
    fn test_attribution_report_beta_with_custom_betas() {
        let ledger = ledger_with_position();
        let market_data = MarketData::default()
            .with_beta("AAPL", 1.5)
            .with_beta("MSFT", 1.2);
        let attributor = RiskAttributor::new(&ledger, market_data);
        let report = attributor.compute();
        // Weighted average beta should be between 1.2 and 1.5
        assert!(report.portfolio_beta >= 1.2, "beta should be >= 1.2, got {}", report.portfolio_beta);
        assert!(report.portfolio_beta <= 1.5, "beta should be <= 1.5, got {}", report.portfolio_beta);
    }

    #[test]
    fn test_risk_attribution_is_dominant() {
        let attr = RiskAttribution {
            factor: RiskFactor::Market,
            contribution_pct: 60.0,
            value: 1000.0,
        };
        assert!(attr.is_dominant(50.0));
        assert!(!attr.is_dominant(70.0));
    }

    // ── BHB Attribution ──────────────────────────────────────────────────────

    #[test]
    fn test_bhb_total_active_return_equals_sum_of_effects() {
        let ledger = PositionLedger::new(dec!(100_000));
        let market_data = MarketData::default();
        let attributor = RiskAttributor::new(&ledger, market_data);

        let input = BhbInput {
            benchmark_total_return: 0.05,
            sectors: vec![
                BhbSectorInput {
                    sector: "Technology".into(),
                    portfolio_weight: 0.60,
                    benchmark_weight: 0.40,
                    portfolio_sector_return: 0.08,
                    benchmark_sector_return: 0.06,
                },
                BhbSectorInput {
                    sector: "Energy".into(),
                    portfolio_weight: 0.40,
                    benchmark_weight: 0.60,
                    portfolio_sector_return: 0.02,
                    benchmark_sector_return: 0.04,
                },
            ],
        };

        let bhb = attributor.compute_bhb(&input);
        let expected = bhb.total_allocation + bhb.total_selection + bhb.total_interaction;
        assert!(
            (bhb.total_active_return - expected).abs() < 1e-10,
            "total_active_return should equal sum of effects"
        );
    }

    #[test]
    fn test_bhb_allocation_effect_positive_for_overweight_outperformer() {
        let ledger = PositionLedger::new(dec!(100_000));
        let market_data = MarketData::default();
        let attributor = RiskAttributor::new(&ledger, market_data);

        // Overweight a sector that outperformed the benchmark
        let input = BhbInput {
            benchmark_total_return: 0.04,
            sectors: vec![BhbSectorInput {
                sector: "Tech".into(),
                portfolio_weight: 0.70, // overweight vs benchmark 0.50
                benchmark_weight: 0.50,
                portfolio_sector_return: 0.10,
                benchmark_sector_return: 0.08, // sector beat benchmark
            }],
        };
        let bhb = attributor.compute_bhb(&input);
        // allocation = (0.70 - 0.50) * (0.08 - 0.04) = 0.20 * 0.04 = 0.008 > 0
        assert!(
            bhb.total_allocation > 0.0,
            "allocation should be positive for overweight outperforming sector"
        );
    }

    #[test]
    fn test_bhb_sector_effects_count_matches_input() {
        let ledger = PositionLedger::new(dec!(100_000));
        let market_data = MarketData::default();
        let attributor = RiskAttributor::new(&ledger, market_data);

        let input = BhbInput {
            benchmark_total_return: 0.05,
            sectors: vec![
                BhbSectorInput {
                    sector: "A".into(),
                    portfolio_weight: 0.5,
                    benchmark_weight: 0.5,
                    portfolio_sector_return: 0.05,
                    benchmark_sector_return: 0.05,
                },
                BhbSectorInput {
                    sector: "B".into(),
                    portfolio_weight: 0.5,
                    benchmark_weight: 0.5,
                    portfolio_sector_return: 0.05,
                    benchmark_sector_return: 0.05,
                },
            ],
        };

        let bhb = attributor.compute_bhb(&input);
        assert_eq!(bhb.sector_effects.len(), 2);
    }

    #[test]
    fn test_bhb_zero_active_return_when_weights_and_returns_match_benchmark() {
        let ledger = PositionLedger::new(dec!(100_000));
        let market_data = MarketData::default();
        let attributor = RiskAttributor::new(&ledger, market_data);

        let input = BhbInput {
            benchmark_total_return: 0.05,
            sectors: vec![BhbSectorInput {
                sector: "All".into(),
                portfolio_weight: 1.0,
                benchmark_weight: 1.0,
                portfolio_sector_return: 0.05,
                benchmark_sector_return: 0.05,
            }],
        };

        let bhb = attributor.compute_bhb(&input);
        assert!(
            bhb.total_active_return.abs() < 1e-10,
            "active return should be zero when portfolio mirrors benchmark"
        );
    }

    #[test]
    fn test_bhb_best_allocation_sector() {
        let ledger = PositionLedger::new(dec!(100_000));
        let market_data = MarketData::default();
        let attributor = RiskAttributor::new(&ledger, market_data);

        let input = BhbInput {
            benchmark_total_return: 0.04,
            sectors: vec![
                BhbSectorInput {
                    sector: "Tech".into(),
                    portfolio_weight: 0.70,
                    benchmark_weight: 0.50,
                    portfolio_sector_return: 0.10,
                    benchmark_sector_return: 0.08,
                },
                BhbSectorInput {
                    sector: "Energy".into(),
                    portfolio_weight: 0.30,
                    benchmark_weight: 0.50,
                    portfolio_sector_return: 0.02,
                    benchmark_sector_return: 0.03,
                },
            ],
        };
        let bhb = attributor.compute_bhb(&input);
        let best = bhb.best_allocation_sector();
        assert!(best.is_some());
        assert_eq!(best.unwrap().sector, "Tech");
    }

    // ── MarketData ───────────────────────────────────────────────────────────

    #[test]
    fn test_market_data_default_beta_is_one() {
        let md = MarketData::default();
        assert_eq!(md.beta("UNKNOWN_SYM"), 1.0);
    }

    #[test]
    fn test_market_data_default_liquidity_is_one() {
        let md = MarketData::default();
        assert_eq!(md.liquidity("UNKNOWN_SYM"), 1.0);
    }

    #[test]
    fn test_market_data_default_idio_vol() {
        let md = MarketData::default();
        assert_eq!(md.idio_vol("UNKNOWN_SYM"), 0.20);
    }

    #[test]
    fn test_market_data_custom_beta() {
        let md = MarketData::default().with_beta("TSLA", 2.0);
        assert_eq!(md.beta("TSLA"), 2.0);
    }
}
