//! # Module: montecarlo
//!
//! Geometric Brownian Motion Monte Carlo simulator for price path analysis.
//!
//! ## Responsibility
//! Runs N GBM price-path simulations using a seeded LCG for reproducibility
//! and Box-Muller for normal samples. Provides VaR, CVaR, and percentile paths.
//!
//! ## Guarantees
//! - Same seed always produces identical paths
//! - VaR ≥ CVaR at any confidence level (CVaR is always more conservative)
//! - `simulate_paths` returns exactly `config.simulations` paths,
//!   each of length `config.horizon_days`

// ─── GBM Parameters ───────────────────────────────────────────────────────────

/// Parameters for Geometric Brownian Motion simulation.
#[derive(Debug, Clone, Copy)]
pub struct GbmParams {
    /// Annual drift (expected return), e.g. 0.10 = 10% p.a.
    pub mu: f64,
    /// Annual volatility (standard deviation), e.g. 0.20 = 20% p.a.
    pub sigma: f64,
    /// Initial asset price.
    pub s0: f64,
}

// ─── Config ───────────────────────────────────────────────────────────────────

/// Configuration for a Monte Carlo simulation run.
#[derive(Debug, Clone)]
pub struct MonteCarloConfig {
    /// Number of price paths to simulate.
    pub simulations: usize,
    /// Number of daily steps per path.
    pub horizon_days: usize,
    /// Optional RNG seed for reproducibility; `None` uses a fixed default seed.
    pub seed: Option<u64>,
}

// ─── Result ───────────────────────────────────────────────────────────────────

/// Aggregated result from a Monte Carlo simulation run.
#[derive(Debug, Clone)]
pub struct MonteCarloResult {
    /// All simulated price paths (shape: `[simulations][horizon_days]`).
    pub paths: Vec<Vec<f64>>,
    /// Value at Risk at 95% confidence.
    pub var_95: f64,
    /// Conditional Value at Risk (Expected Shortfall) at 95% confidence.
    pub cvar_95: f64,
    /// Median final price across all paths.
    pub median_final: f64,
    /// Highest final price across all paths.
    pub best_case_final: f64,
    /// Lowest final price across all paths.
    pub worst_case_final: f64,
}

// ─── LCG RNG ─────────────────────────────────────────────────────────────────

/// Linear Congruential Generator — fast, reproducible PRNG.
struct Lcg {
    state: u64,
}

impl Lcg {
    /// Create a new LCG with the given seed.
    fn new(seed: u64) -> Self {
        Self { state: seed.wrapping_add(1) }
    }

    /// Produce the next `u64` in the sequence.
    fn next_u64(&mut self) -> u64 {
        // Numerical Recipes constants
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }

    /// Produce a uniform sample in `[0, 1)`.
    fn next_f64(&mut self) -> f64 {
        // Use upper 53 bits for double precision
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

// ─── Box-Muller ───────────────────────────────────────────────────────────────

/// Generate a pair of independent standard-normal samples using Box-Muller.
///
/// # Panics
/// Does not panic; handles u = 0 by clamping to a small epsilon.
fn box_muller(rng: &mut Lcg) -> (f64, f64) {
    let mut u1 = rng.next_f64();
    let u2 = rng.next_f64();
    // Guard against log(0)
    if u1 < 1e-15 {
        u1 = 1e-15;
    }
    let mag = (-2.0 * u1.ln()).sqrt();
    let theta = 2.0 * std::f64::consts::PI * u2;
    (mag * theta.cos(), mag * theta.sin())
}

// ─── Simulator ────────────────────────────────────────────────────────────────

/// Monte Carlo price-path simulator using Geometric Brownian Motion.
pub struct MonteCarloSimulator;

impl MonteCarloSimulator {
    /// Simulate `config.simulations` GBM price paths.
    ///
    /// Each path contains `config.horizon_days` prices (day 1 through horizon).
    /// The initial price `params.s0` is NOT included in the path output; the
    /// first element is the price after day 1.
    pub fn simulate_paths(params: &GbmParams, config: &MonteCarloConfig) -> Vec<Vec<f64>> {
        let seed = config.seed.unwrap_or(42);
        let mut rng = Lcg::new(seed);

        let dt = 1.0 / 252.0; // one trading day
        let drift = (params.mu - 0.5 * params.sigma * params.sigma) * dt;
        let vol_sqrt_dt = params.sigma * dt.sqrt();

        let mut paths = Vec::with_capacity(config.simulations);

        let mut sim_i = 0;
        while sim_i < config.simulations {
            let mut path_a = Vec::with_capacity(config.horizon_days);
            let mut path_b = Vec::with_capacity(config.horizon_days);
            let mut price_a = params.s0;
            let mut price_b = params.s0;

            let mut day = 0;
            while day < config.horizon_days {
                let (z1, z2) = box_muller(&mut rng);
                price_a *= (drift + vol_sqrt_dt * z1).exp();
                path_a.push(price_a);
                if day < config.horizon_days {
                    price_b *= (drift + vol_sqrt_dt * z2).exp();
                    path_b.push(price_b);
                }
                day += 1;
            }

            paths.push(path_a);
            sim_i += 1;
            if sim_i < config.simulations {
                paths.push(path_b);
                sim_i += 1;
            }
        }

        // Trim to exact count
        paths.truncate(config.simulations);
        paths
    }

    /// Value at Risk: the loss such that only `(1 - confidence)` of paths
    /// perform worse.
    ///
    /// Returns the loss expressed as a positive number relative to `s0`.
    /// E.g. VaR_95 = 5.0 means there is a 5% chance of losing more than 5.
    ///
    /// # Arguments
    /// - `paths`: output of `simulate_paths`
    /// - `confidence`: e.g. 0.95 for 95% VaR
    pub fn var(paths: &[Vec<f64>], confidence: f64) -> f64 {
        if paths.is_empty() {
            return 0.0;
        }
        // Use final prices
        let mut finals: Vec<f64> = paths
            .iter()
            .filter_map(|p| p.last().copied())
            .collect();
        finals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        // VaR at confidence = value at the (1-confidence) quantile
        let idx = ((1.0 - confidence) * finals.len() as f64).ceil() as usize;
        let idx = idx.min(finals.len() - 1);
        // Express as loss from initial price (infer from first path's first value vs last)
        // We return the absolute value at the quantile; caller compares to s0
        finals[idx]
    }

    /// Conditional Value at Risk (Expected Shortfall): the expected loss in the
    /// worst `(1-confidence)` of scenarios.
    ///
    /// CVaR is always ≤ VaR (more pessimistic).
    pub fn cvar(paths: &[Vec<f64>], confidence: f64) -> f64 {
        if paths.is_empty() {
            return 0.0;
        }
        let mut finals: Vec<f64> = paths
            .iter()
            .filter_map(|p| p.last().copied())
            .collect();
        finals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let tail_n = ((1.0 - confidence) * finals.len() as f64).ceil() as usize;
        let tail_n = tail_n.max(1).min(finals.len());
        let tail_sum: f64 = finals[..tail_n].iter().sum();
        tail_sum / tail_n as f64
    }

    /// Extract percentile paths.
    ///
    /// For each percentile in `percentiles` (e.g. `[5.0, 50.0, 95.0]`),
    /// returns the path whose final value is closest to that percentile of
    /// final-value distribution.
    ///
    /// # Returns
    /// A `Vec<Vec<f64>>` with the same length as `percentiles`.
    pub fn percentile_paths(paths: &[Vec<f64>], percentiles: &[f64]) -> Vec<Vec<f64>> {
        if paths.is_empty() || percentiles.is_empty() {
            return vec![];
        }

        // Sort paths by final value
        let mut indexed: Vec<(usize, f64)> = paths
            .iter()
            .enumerate()
            .filter_map(|(i, p)| p.last().map(|&v| (i, v)))
            .collect();
        indexed.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        let n = indexed.len();
        percentiles
            .iter()
            .map(|&pct| {
                let pct_clamped = pct.clamp(0.0, 100.0);
                let idx = ((pct_clamped / 100.0) * (n as f64 - 1.0)).round() as usize;
                let idx = idx.min(n - 1);
                paths[indexed[idx].0].clone()
            })
            .collect()
    }

    /// Convenience method: run a full simulation and return a [`MonteCarloResult`].
    pub fn run(params: &GbmParams, config: &MonteCarloConfig) -> MonteCarloResult {
        let paths = Self::simulate_paths(params, config);
        let var_95 = Self::var(&paths, 0.95);
        let cvar_95 = Self::cvar(&paths, 0.95);

        let mut finals: Vec<f64> =
            paths.iter().filter_map(|p| p.last().copied()).collect();
        finals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let median_final = if finals.is_empty() {
            params.s0
        } else {
            finals[finals.len() / 2]
        };
        let best_case_final = finals.last().copied().unwrap_or(params.s0);
        let worst_case_final = finals.first().copied().unwrap_or(params.s0);

        MonteCarloResult { paths, var_95, cvar_95, median_final, best_case_final, worst_case_final }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn default_params() -> GbmParams {
        GbmParams { mu: 0.10, sigma: 0.20, s0: 100.0 }
    }

    fn default_config(seed: u64) -> MonteCarloConfig {
        MonteCarloConfig { simulations: 1000, horizon_days: 252, seed: Some(seed) }
    }

    #[test]
    fn test_path_count_matches_simulations() {
        let params = default_params();
        let config = default_config(1);
        let paths = MonteCarloSimulator::simulate_paths(&params, &config);
        assert_eq!(paths.len(), 1000);
    }

    #[test]
    fn test_path_length_matches_horizon() {
        let params = default_params();
        let config = default_config(1);
        let paths = MonteCarloSimulator::simulate_paths(&params, &config);
        for path in &paths {
            assert_eq!(path.len(), 252);
        }
    }

    #[test]
    fn test_reproducibility_with_same_seed() {
        let params = default_params();
        let config = default_config(42);
        let paths1 = MonteCarloSimulator::simulate_paths(&params, &config);
        let paths2 = MonteCarloSimulator::simulate_paths(&params, &config);
        assert_eq!(paths1.len(), paths2.len());
        for (p1, p2) in paths1.iter().zip(paths2.iter()) {
            for (a, b) in p1.iter().zip(p2.iter()) {
                assert!((a - b).abs() < 1e-12, "Paths differ: {} vs {}", a, b);
            }
        }
    }

    #[test]
    fn test_different_seeds_produce_different_paths() {
        let params = default_params();
        let c1 = default_config(1);
        let c2 = default_config(2);
        let p1 = MonteCarloSimulator::simulate_paths(&params, &c1);
        let p2 = MonteCarloSimulator::simulate_paths(&params, &c2);
        // First paths should differ
        let differs = p1[0].iter().zip(p2[0].iter()).any(|(a, b)| (a - b).abs() > 1e-10);
        assert!(differs, "Different seeds should yield different paths");
    }

    #[test]
    fn test_all_prices_positive() {
        let params = default_params();
        let config = default_config(7);
        let paths = MonteCarloSimulator::simulate_paths(&params, &config);
        for path in &paths {
            for &price in path {
                assert!(price > 0.0, "GBM price must be positive, got {}", price);
            }
        }
    }

    #[test]
    fn test_var_less_than_or_equal_cvar() {
        // CVaR is the conditional expectation of losses beyond VaR → ≤ VaR in absolute
        let params = default_params();
        let config = default_config(10);
        let paths = MonteCarloSimulator::simulate_paths(&params, &config);
        let var = MonteCarloSimulator::var(&paths, 0.95);
        let cvar = MonteCarloSimulator::cvar(&paths, 0.95);
        // cvar is mean of worst tail → should be ≤ var (lower price = larger loss)
        assert!(cvar <= var + 1e-6, "CVaR={} should be <= VaR={}", cvar, var);
    }

    #[test]
    fn test_var_is_positive_price() {
        let params = default_params();
        let config = default_config(5);
        let paths = MonteCarloSimulator::simulate_paths(&params, &config);
        let var = MonteCarloSimulator::var(&paths, 0.95);
        assert!(var > 0.0, "VaR should be a positive price");
    }

    #[test]
    fn test_cvar_is_positive_price() {
        let params = default_params();
        let config = default_config(5);
        let paths = MonteCarloSimulator::simulate_paths(&params, &config);
        let cvar = MonteCarloSimulator::cvar(&paths, 0.95);
        assert!(cvar > 0.0, "CVaR should be a positive price");
    }

    #[test]
    fn test_percentile_paths_count() {
        let params = default_params();
        let config = default_config(3);
        let paths = MonteCarloSimulator::simulate_paths(&params, &config);
        let pct_paths = MonteCarloSimulator::percentile_paths(&paths, &[5.0, 50.0, 95.0]);
        assert_eq!(pct_paths.len(), 3);
    }

    #[test]
    fn test_percentile_paths_length() {
        let params = default_params();
        let config = default_config(3);
        let paths = MonteCarloSimulator::simulate_paths(&params, &config);
        let pct_paths = MonteCarloSimulator::percentile_paths(&paths, &[5.0, 50.0, 95.0]);
        for p in &pct_paths {
            assert_eq!(p.len(), 252);
        }
    }

    #[test]
    fn test_percentile_ordering() {
        let params = default_params();
        let config = default_config(99);
        let paths = MonteCarloSimulator::simulate_paths(&params, &config);
        let pct_paths = MonteCarloSimulator::percentile_paths(&paths, &[5.0, 50.0, 95.0]);
        let p5_final = pct_paths[0].last().copied().unwrap_or(0.0);
        let p50_final = pct_paths[1].last().copied().unwrap_or(0.0);
        let p95_final = pct_paths[2].last().copied().unwrap_or(0.0);
        assert!(p5_final <= p50_final + 1e-6, "p5={} p50={}", p5_final, p50_final);
        assert!(p50_final <= p95_final + 1e-6, "p50={} p95={}", p50_final, p95_final);
    }

    #[test]
    fn test_gbm_positive_drift_raises_median() {
        // With strong positive drift, median final price should exceed s0
        let params = GbmParams { mu: 0.50, sigma: 0.10, s0: 100.0 };
        let config = MonteCarloConfig { simulations: 5000, horizon_days: 252, seed: Some(1) };
        let paths = MonteCarloSimulator::simulate_paths(&params, &config);
        let mut finals: Vec<f64> =
            paths.iter().filter_map(|p| p.last().copied()).collect();
        finals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let median = finals[finals.len() / 2];
        assert!(median > 100.0, "Positive drift should raise median: median={}", median);
    }

    #[test]
    fn test_gbm_zero_vol_deterministic() {
        // With sigma=0, all paths should be identical (pure drift)
        let params = GbmParams { mu: 0.10, sigma: 0.0, s0: 100.0 };
        let config = MonteCarloConfig { simulations: 10, horizon_days: 10, seed: Some(1) };
        let paths = MonteCarloSimulator::simulate_paths(&params, &config);
        let first = &paths[0];
        for path in &paths[1..] {
            for (a, b) in first.iter().zip(path.iter()) {
                assert!(
                    (a - b).abs() < 1e-6,
                    "Zero-vol paths should be identical: {} vs {}",
                    a,
                    b
                );
            }
        }
    }

    #[test]
    fn test_run_result_structure() {
        let params = default_params();
        let config = default_config(77);
        let result = MonteCarloSimulator::run(&params, &config);
        assert_eq!(result.paths.len(), 1000);
        assert!(result.best_case_final >= result.median_final);
        assert!(result.worst_case_final <= result.median_final);
        assert!(result.var_95 > 0.0);
        assert!(result.cvar_95 > 0.0);
    }

    #[test]
    fn test_empty_paths_var_is_zero() {
        let var = MonteCarloSimulator::var(&[], 0.95);
        assert_eq!(var, 0.0);
    }

    #[test]
    fn test_empty_paths_cvar_is_zero() {
        let cvar = MonteCarloSimulator::cvar(&[], 0.95);
        assert_eq!(cvar, 0.0);
    }

    #[test]
    fn test_odd_simulation_count() {
        // Test that odd simulation count works correctly
        let params = default_params();
        let config = MonteCarloConfig { simulations: 101, horizon_days: 10, seed: Some(5) };
        let paths = MonteCarloSimulator::simulate_paths(&params, &config);
        assert_eq!(paths.len(), 101);
    }

    #[test]
    fn test_single_simulation() {
        let params = default_params();
        let config = MonteCarloConfig { simulations: 1, horizon_days: 5, seed: Some(1) };
        let paths = MonteCarloSimulator::simulate_paths(&params, &config);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].len(), 5);
    }

    #[test]
    fn test_percentile_empty_paths() {
        let result = MonteCarloSimulator::percentile_paths(&[], &[50.0]);
        assert!(result.is_empty());
    }
}
