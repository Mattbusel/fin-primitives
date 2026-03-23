//! # Module: regime::hmm
//!
//! ## Responsibility
//! 2-state Hidden Markov Model (HMM) with Gaussian emissions and Viterbi decoding
//! for market regime detection.
//!
//! ## Guarantees
//! - Zero panics on valid inputs; returns empty Vec for empty input
//! - Log-space Viterbi avoids underflow on long sequences
//! - All public functions documented

use std::f64::consts::PI;

// ─────────────────────────────────────────
//  RegimeState
// ─────────────────────────────────────────

/// A two-state market regime label produced by the HMM Viterbi decoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegimeState {
    /// Bull market regime: positive drift, low volatility.
    Bull,
    /// Bear market regime: negative drift, high volatility.
    Bear,
}

// ─────────────────────────────────────────
//  HmmParams
// ─────────────────────────────────────────

/// Parameters for the 2-state Hidden Markov Model.
///
/// State 0 = Bull, State 1 = Bear.
#[derive(Debug, Clone)]
pub struct HmmParams {
    /// Transition matrix: `transition[from][to]` = P(next=to | current=from).
    pub transition: [[f64; 2]; 2],
    /// Emission mean for each state: `emission_mean[state]`.
    pub emission_mean: [f64; 2],
    /// Emission standard deviation for each state: `emission_std[state]`.
    pub emission_std: [f64; 2],
    /// Initial state probabilities: `initial[state]`.
    pub initial: [f64; 2],
}

impl HmmParams {
    /// Default parameters calibrated to financial return series.
    ///
    /// - Bull: mean=0.001, std=0.010
    /// - Bear: mean=-0.002, std=0.025
    /// - Transition: [[0.95, 0.05], [0.10, 0.90]]
    /// - Initial: [0.7, 0.3]
    pub fn default_financial() -> Self {
        Self {
            transition: [[0.95, 0.05], [0.10, 0.90]],
            emission_mean: [0.001, -0.002],
            emission_std: [0.010, 0.025],
            initial: [0.7, 0.3],
        }
    }
}

// ─────────────────────────────────────────
//  Gaussian emission
// ─────────────────────────────────────────

/// Gaussian probability density function.
///
/// Returns the density of `x` under N(`mean`, `std`²).
/// Returns 0.0 if `std` ≤ 0.
#[must_use]
pub fn gaussian_pdf(x: f64, mean: f64, std: f64) -> f64 {
    if std <= 0.0 {
        return 0.0;
    }
    let z = (x - mean) / std;
    (1.0 / (std * (2.0 * PI).sqrt())) * (-0.5 * z * z).exp()
}

// ─────────────────────────────────────────
//  Viterbi
// ─────────────────────────────────────────

/// Run the Viterbi algorithm in log-space on a return series.
///
/// Returns the most-likely state sequence (same length as `returns`).
/// Returns an empty `Vec` if `returns` is empty.
///
/// # Arguments
/// * `returns` - Slice of log-returns or price returns.
/// * `params`  - HMM parameters (see [`HmmParams::default_financial`]).
#[must_use]
pub fn viterbi(returns: &[f64], params: &HmmParams) -> Vec<RegimeState> {
    let n = returns.len();
    if n == 0 {
        return Vec::new();
    }

    const NUM_STATES: usize = 2;

    // log_delta[t][s] = log-probability of most-likely path ending in state s at time t
    let mut log_delta = vec![[0.0f64; NUM_STATES]; n];
    // psi[t][s] = predecessor state on the most-likely path to state s at time t
    let mut psi = vec![[0usize; NUM_STATES]; n];

    // ── Initialisation ──────────────────────────────────────────────────────
    for s in 0..NUM_STATES {
        let emission = gaussian_pdf(returns[0], params.emission_mean[s], params.emission_std[s]);
        let log_em = if emission > 0.0 { emission.ln() } else { f64::NEG_INFINITY };
        let log_init = if params.initial[s] > 0.0 {
            params.initial[s].ln()
        } else {
            f64::NEG_INFINITY
        };
        log_delta[0][s] = log_init + log_em;
        psi[0][s] = 0;
    }

    // ── Recursion ────────────────────────────────────────────────────────────
    for t in 1..n {
        for s in 0..NUM_STATES {
            let emission = gaussian_pdf(returns[t], params.emission_mean[s], params.emission_std[s]);
            let log_em = if emission > 0.0 { emission.ln() } else { f64::NEG_INFINITY };

            let mut best_val = f64::NEG_INFINITY;
            let mut best_prev = 0;
            for prev in 0..NUM_STATES {
                let tr = params.transition[prev][s];
                let log_tr = if tr > 0.0 { tr.ln() } else { f64::NEG_INFINITY };
                let val = log_delta[t - 1][prev] + log_tr;
                if val > best_val {
                    best_val = val;
                    best_prev = prev;
                }
            }
            log_delta[t][s] = best_val + log_em;
            psi[t][s] = best_prev;
        }
    }

    // ── Termination ──────────────────────────────────────────────────────────
    let mut best_last = 0;
    let mut best_val = f64::NEG_INFINITY;
    for s in 0..NUM_STATES {
        if log_delta[n - 1][s] > best_val {
            best_val = log_delta[n - 1][s];
            best_last = s;
        }
    }

    // ── Backtrack ────────────────────────────────────────────────────────────
    let mut path = vec![0usize; n];
    path[n - 1] = best_last;
    for t in (0..n - 1).rev() {
        path[t] = psi[t + 1][path[t + 1]];
    }

    path.into_iter()
        .map(|s| if s == 0 { RegimeState::Bull } else { RegimeState::Bear })
        .collect()
}

// ─────────────────────────────────────────
//  RegimeReport
// ─────────────────────────────────────────

/// Summary statistics produced by [`RegimeAnalyzer::analyze`].
#[derive(Debug, Clone)]
pub struct RegimeReport {
    /// Full state sequence, one entry per return observation.
    pub states: Vec<RegimeState>,
    /// Contiguous bull-regime intervals as `(start_idx, end_idx)` inclusive.
    pub bull_periods: Vec<(usize, usize)>,
    /// Contiguous bear-regime intervals as `(start_idx, end_idx)` inclusive.
    pub bear_periods: Vec<(usize, usize)>,
    /// Fraction of observations classified as Bull, in [0, 1].
    pub bull_fraction: f64,
    /// Mean duration (in bars) of bull periods.
    pub mean_bull_duration: f64,
    /// Mean duration (in bars) of bear periods.
    pub mean_bear_duration: f64,
}

/// Find contiguous runs of a specific [`RegimeState`] in a state sequence.
///
/// Returns a vector of `(start, end)` index pairs (inclusive) for each run.
#[must_use]
pub fn find_periods(states: &[RegimeState], target: RegimeState) -> Vec<(usize, usize)> {
    let mut periods = Vec::new();
    let mut i = 0;
    while i < states.len() {
        if states[i] == target {
            let start = i;
            while i < states.len() && states[i] == target {
                i += 1;
            }
            periods.push((start, i - 1));
        } else {
            i += 1;
        }
    }
    periods
}

fn mean_duration(periods: &[(usize, usize)]) -> f64 {
    if periods.is_empty() {
        return 0.0;
    }
    let total: usize = periods.iter().map(|(s, e)| e - s + 1).sum();
    total as f64 / periods.len() as f64
}

// ─────────────────────────────────────────
//  RegimeAnalyzer
// ─────────────────────────────────────────

/// High-level wrapper: runs Viterbi and computes a [`RegimeReport`].
#[derive(Debug, Clone)]
pub struct RegimeAnalyzer {
    params: HmmParams,
}

impl RegimeAnalyzer {
    /// Create a new analyzer with the given HMM parameters.
    #[must_use]
    pub fn new(params: HmmParams) -> Self {
        Self { params }
    }

    /// Create a new analyzer with [`HmmParams::default_financial`].
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(HmmParams::default_financial())
    }

    /// Decode the most-likely regime sequence and compute summary statistics.
    ///
    /// Returns a [`RegimeReport`] containing the decoded states and aggregate metrics.
    #[must_use]
    pub fn analyze(&self, returns: &[f64]) -> RegimeReport {
        let states = viterbi(returns, &self.params);
        let n = states.len();

        let bull_count = states.iter().filter(|&&s| s == RegimeState::Bull).count();
        let bull_fraction = if n == 0 { 0.0 } else { bull_count as f64 / n as f64 };

        let bull_periods = find_periods(&states, RegimeState::Bull);
        let bear_periods = find_periods(&states, RegimeState::Bear);

        let mean_bull_duration = mean_duration(&bull_periods);
        let mean_bear_duration = mean_duration(&bear_periods);

        RegimeReport {
            states,
            bull_periods,
            bear_periods,
            bull_fraction,
            mean_bull_duration,
            mean_bear_duration,
        }
    }
}

// ─────────────────────────────────────────
//  Regime-Adjusted Sharpe
// ─────────────────────────────────────────

/// Compute Sharpe ratios separately for bull and bear periods.
///
/// Returns `(bull_sharpe, bear_sharpe)`. Sharpe = mean / std_dev (annualised by
/// multiplying by √252). Returns `(0.0, 0.0)` if a regime has fewer than 2 observations.
#[must_use]
pub fn regime_adjusted_sharpe(returns: &[f64], states: &[RegimeState]) -> (f64, f64) {
    fn sharpe_for(vals: &[f64]) -> f64 {
        if vals.len() < 2 {
            return 0.0;
        }
        let mean = vals.iter().sum::<f64>() / vals.len() as f64;
        let variance =
            vals.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (vals.len() - 1) as f64;
        let std = variance.sqrt();
        if std == 0.0 {
            return 0.0;
        }
        mean / std * 252_f64.sqrt()
    }

    let bull_returns: Vec<f64> = returns
        .iter()
        .zip(states.iter())
        .filter(|(_, &s)| s == RegimeState::Bull)
        .map(|(&r, _)| r)
        .collect();

    let bear_returns: Vec<f64> = returns
        .iter()
        .zip(states.iter())
        .filter(|(_, &s)| s == RegimeState::Bear)
        .map(|(&r, _)| r)
        .collect();

    (sharpe_for(&bull_returns), sharpe_for(&bear_returns))
}

// ─────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viterbi_all_positive_returns_is_bull() {
        let returns: Vec<f64> = vec![0.005; 50];
        let params = HmmParams::default_financial();
        let states = viterbi(&returns, &params);
        assert_eq!(states.len(), returns.len());
        // All positive returns should decode to Bull
        assert!(states.iter().all(|&s| s == RegimeState::Bull));
    }

    #[test]
    fn viterbi_all_negative_returns_is_bear() {
        let returns: Vec<f64> = vec![-0.010; 50];
        let params = HmmParams::default_financial();
        let states = viterbi(&returns, &params);
        assert_eq!(states.len(), returns.len());
        assert!(states.iter().all(|&s| s == RegimeState::Bear));
    }

    #[test]
    fn viterbi_empty_returns_empty_states() {
        let states = viterbi(&[], &HmmParams::default_financial());
        assert!(states.is_empty());
    }

    #[test]
    fn state_sequence_length_matches_input() {
        let returns: Vec<f64> = (0..100).map(|i| if i % 3 == 0 { -0.005 } else { 0.003 }).collect();
        let params = HmmParams::default_financial();
        let states = viterbi(&returns, &params);
        assert_eq!(states.len(), returns.len());
    }

    #[test]
    fn bull_fraction_in_unit_interval() {
        let returns: Vec<f64> = (0..200)
            .map(|i| if i % 4 == 0 { -0.008 } else { 0.004 })
            .collect();
        let analyzer = RegimeAnalyzer::with_defaults();
        let report = analyzer.analyze(&returns);
        assert!(report.bull_fraction >= 0.0 && report.bull_fraction <= 1.0);
    }

    #[test]
    fn gaussian_pdf_peak_at_mean() {
        let pdf_at_mean = gaussian_pdf(0.0, 0.0, 1.0);
        let pdf_off_mean = gaussian_pdf(1.0, 0.0, 1.0);
        assert!(pdf_at_mean > pdf_off_mean);
    }

    #[test]
    fn gaussian_pdf_zero_std_returns_zero() {
        assert_eq!(gaussian_pdf(1.0, 1.0, 0.0), 0.0);
    }

    #[test]
    fn regime_adjusted_sharpe_returns_tuple() {
        let returns: Vec<f64> = (0..100).map(|i| if i % 5 == 0 { -0.01 } else { 0.002 }).collect();
        let params = HmmParams::default_financial();
        let states = viterbi(&returns, &params);
        let (bull_sharpe, bear_sharpe) = regime_adjusted_sharpe(&returns, &states);
        // Just check they are finite
        assert!(bull_sharpe.is_finite());
        assert!(bear_sharpe.is_finite());
    }

    #[test]
    fn find_periods_contiguous_runs() {
        let states = vec![
            RegimeState::Bull,
            RegimeState::Bull,
            RegimeState::Bear,
            RegimeState::Bear,
            RegimeState::Bull,
        ];
        let bull = find_periods(&states, RegimeState::Bull);
        assert_eq!(bull, vec![(0, 1), (4, 4)]);
        let bear = find_periods(&states, RegimeState::Bear);
        assert_eq!(bear, vec![(2, 3)]);
    }
}
