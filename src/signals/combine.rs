//! # Module: signals::combine
//!
//! ## Responsibility
//! Ensemble combiners that merge N binary or continuous signals into a single
//! actionable output using majority vote, weighted sum, or Bayesian update.
//!
//! ## Guarantees
//! - All combiners are zero-panic; inputs are validated at construction and per-update
//! - Weights are validated to be finite and non-negative at construction
//! - `BayesianCombiner` posterior is always clamped to `[ε, 1-ε]` to prevent log(0)
//! - Signal names are stored for interpretability

use crate::error::FinError;

/// The direction of a binary signal vote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Vote {
    /// Bullish signal (buy/long).
    Bull,
    /// Bearish signal (sell/short).
    Bear,
    /// No directional view (abstain).
    Neutral,
}

impl Vote {
    /// Returns `1.0` for `Bull`, `-1.0` for `Bear`, `0.0` for `Neutral`.
    pub fn as_f64(self) -> f64 {
        match self {
            Vote::Bull => 1.0,
            Vote::Bear => -1.0,
            Vote::Neutral => 0.0,
        }
    }

    /// Converts a signed continuous value to a `Vote`.
    /// Positive → `Bull`, negative → `Bear`, zero → `Neutral`.
    pub fn from_f64(v: f64) -> Self {
        if v > 0.0 {
            Vote::Bull
        } else if v < 0.0 {
            Vote::Bear
        } else {
            Vote::Neutral
        }
    }
}

/// Combined signal output from an ensemble combiner.
#[derive(Debug, Clone, PartialEq)]
pub struct EnsembleOutput {
    /// The combined directional vote.
    pub direction: Vote,
    /// Raw numeric score (positive = bullish, negative = bearish).
    pub score: f64,
    /// Confidence in [0, 1]. Interpretation varies by combiner.
    pub confidence: f64,
}

// ─────────────────────────────────────────
//  VotingCombiner – simple majority vote
// ─────────────────────────────────────────

/// Majority-vote ensemble combiner.
///
/// Each signal casts a [`Vote`] (Bull / Bear / Neutral). The direction with the
/// most votes wins. Ties are resolved as `Neutral`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::combine::{VotingCombiner, Vote};
///
/// let mut vc = VotingCombiner::new(vec![
///     "rsi_signal".to_owned(),
///     "macd_signal".to_owned(),
///     "adx_signal".to_owned(),
/// ]).unwrap();
///
/// let out = vc.update(&[Vote::Bull, Vote::Bull, Vote::Bear]).unwrap();
/// assert_eq!(out.direction, Vote::Bull);
/// ```
#[derive(Debug)]
pub struct VotingCombiner {
    /// Names of the N signals in vote order.
    names: Vec<String>,
}

impl VotingCombiner {
    /// Constructs a `VotingCombiner` with the given signal names.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if `names` is empty.
    pub fn new(names: Vec<String>) -> Result<Self, FinError> {
        if names.is_empty() {
            return Err(FinError::InvalidInput(
                "VotingCombiner requires at least one signal".to_owned(),
            ));
        }
        Ok(Self { names })
    }

    /// Returns the number of signals.
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// Returns `true` if no signals are registered.
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// Returns the signal names.
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// Combines `votes` using majority rule.
    ///
    /// `votes.len()` must equal `self.len()`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if `votes.len() != self.len()`.
    pub fn update(&self, votes: &[Vote]) -> Result<EnsembleOutput, FinError> {
        if votes.len() != self.names.len() {
            return Err(FinError::InvalidInput(format!(
                "expected {} votes, got {}",
                self.names.len(),
                votes.len()
            )));
        }
        let bulls = votes.iter().filter(|v| **v == Vote::Bull).count();
        let bears = votes.iter().filter(|v| **v == Vote::Bear).count();
        let total = votes.len();

        let direction = if bulls > bears && bulls > total / 2 {
            Vote::Bull
        } else if bears > bulls && bears > total / 2 {
            Vote::Bear
        } else {
            Vote::Neutral
        };

        let score = votes.iter().map(|v| v.as_f64()).sum::<f64>() / total as f64;
        let max_side = bulls.max(bears) as f64;
        let confidence = if total == 0 { 0.0 } else { max_side / total as f64 };

        Ok(EnsembleOutput { direction, score, confidence })
    }
}

// ─────────────────────────────────────────
//  WeightedCombiner – weighted sum
// ─────────────────────────────────────────

/// Weighted-sum ensemble combiner.
///
/// Each signal contributes a continuous score in `[-1, 1]` multiplied by its weight.
/// The aggregate is compared against configurable thresholds to determine direction.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::combine::{WeightedCombiner, Vote};
///
/// let mut wc = WeightedCombiner::new(
///     vec!["rsi".to_owned(), "macd".to_owned()],
///     vec![0.6, 0.4],
///     0.2,   // bull_threshold
///     -0.2,  // bear_threshold
/// ).unwrap();
///
/// let out = wc.update(&[0.8, 0.5]).unwrap();
/// assert_eq!(out.direction, Vote::Bull);
/// ```
#[derive(Debug)]
pub struct WeightedCombiner {
    names: Vec<String>,
    weights: Vec<f64>,
    bull_threshold: f64,
    bear_threshold: f64,
    weight_sum: f64,
}

impl WeightedCombiner {
    /// Constructs a `WeightedCombiner`.
    ///
    /// # Parameters
    /// - `names`: signal names (must be non-empty, length must match `weights`).
    /// - `weights`: non-negative finite weights; need not sum to 1 (normalized internally).
    /// - `bull_threshold`: weighted score above which direction is `Bull`.
    /// - `bear_threshold`: weighted score below which direction is `Bear`.
    ///
    /// # Errors
    /// - [`FinError::InvalidInput`] if `names` is empty, lengths differ, any weight is
    ///   negative or non-finite, or the total weight is zero.
    pub fn new(
        names: Vec<String>,
        weights: Vec<f64>,
        bull_threshold: f64,
        bear_threshold: f64,
    ) -> Result<Self, FinError> {
        if names.is_empty() {
            return Err(FinError::InvalidInput(
                "WeightedCombiner requires at least one signal".to_owned(),
            ));
        }
        if names.len() != weights.len() {
            return Err(FinError::InvalidInput(format!(
                "names length {} != weights length {}",
                names.len(),
                weights.len()
            )));
        }
        for (i, &w) in weights.iter().enumerate() {
            if !w.is_finite() || w < 0.0 {
                return Err(FinError::InvalidInput(format!(
                    "weight[{i}] must be finite and non-negative, got {w}"
                )));
            }
        }
        let weight_sum: f64 = weights.iter().sum();
        if weight_sum == 0.0 {
            return Err(FinError::InvalidInput(
                "sum of weights must be > 0".to_owned(),
            ));
        }
        if bear_threshold > bull_threshold {
            return Err(FinError::InvalidInput(
                "bear_threshold must be <= bull_threshold".to_owned(),
            ));
        }
        Ok(Self { names, weights, bull_threshold, bear_threshold, weight_sum })
    }

    /// Returns the signal names.
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// Returns the weights.
    pub fn weights(&self) -> &[f64] {
        &self.weights
    }

    /// Combines continuous scores using a weighted sum.
    ///
    /// Each `score` should be in `[-1.0, 1.0]`; values outside that range are clamped.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if `scores.len() != self.len()`.
    pub fn update(&self, scores: &[f64]) -> Result<EnsembleOutput, FinError> {
        if scores.len() != self.names.len() {
            return Err(FinError::InvalidInput(format!(
                "expected {} scores, got {}",
                self.names.len(),
                scores.len()
            )));
        }
        let weighted: f64 = scores
            .iter()
            .zip(&self.weights)
            .map(|(s, w)| s.clamp(-1.0, 1.0) * w)
            .sum::<f64>()
            / self.weight_sum;

        let direction = if weighted > self.bull_threshold {
            Vote::Bull
        } else if weighted < self.bear_threshold {
            Vote::Bear
        } else {
            Vote::Neutral
        };

        // Confidence: how far the score is from the nearest threshold, normalized to [0,1]
        let dist_from_neutral = weighted.abs();
        let confidence = dist_from_neutral.min(1.0);

        Ok(EnsembleOutput { direction, score: weighted, confidence })
    }

    /// Returns the number of signals.
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// Returns `true` if no signals are registered.
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

// ─────────────────────────────────────────
//  BayesianCombiner – Naive Bayes update
// ─────────────────────────────────────────

/// Naive Bayes probability combiner.
///
/// Maintains a posterior probability of the market being in a "bullish" state,
/// updated multiplicatively as each binary signal is observed.
///
/// Each signal has a configured likelihood ratio: `P(signal=Bull | market=Bull)` vs
/// `P(signal=Bull | market=Bear)`. The posterior is updated via Bayes' rule and
/// clamped to prevent numerical collapse.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::combine::{BayesianCombiner, Vote};
///
/// let mut bc = BayesianCombiner::new(
///     vec!["rsi".to_owned(), "macd".to_owned()],
///     vec![0.7, 0.65],  // P(bull signal | bull market)
///     vec![0.3, 0.4],   // P(bull signal | bear market)
///     0.5,              // prior P(bull)
/// ).unwrap();
///
/// let out = bc.update(&[Vote::Bull, Vote::Bull]).unwrap();
/// assert_eq!(out.direction, Vote::Bull);
/// assert!(out.confidence > 0.5);
/// ```
#[derive(Debug)]
pub struct BayesianCombiner {
    names: Vec<String>,
    /// P(signal_i = Bull | market = Bull) for each signal.
    p_bull_given_bull: Vec<f64>,
    /// P(signal_i = Bull | market = Bear) for each signal.
    p_bull_given_bear: Vec<f64>,
    /// Current posterior P(market = Bull).
    posterior: f64,
    /// Numerical clamp ε to avoid log(0).
    epsilon: f64,
}

impl BayesianCombiner {
    /// Constructs a `BayesianCombiner`.
    ///
    /// # Parameters
    /// - `names`: signal names.
    /// - `p_bull_given_bull`: `P(signal=Bull | market=Bull)` in `(0, 1)` for each signal.
    /// - `p_bull_given_bear`: `P(signal=Bull | market=Bear)` in `(0, 1)` for each signal.
    /// - `prior`: initial `P(market=Bull)` in `(0, 1)`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if lengths differ, probabilities are out of `(0,1)`,
    /// or `prior` is out of `(0,1)`.
    pub fn new(
        names: Vec<String>,
        p_bull_given_bull: Vec<f64>,
        p_bull_given_bear: Vec<f64>,
        prior: f64,
    ) -> Result<Self, FinError> {
        if names.is_empty() {
            return Err(FinError::InvalidInput(
                "BayesianCombiner requires at least one signal".to_owned(),
            ));
        }
        if names.len() != p_bull_given_bull.len() || names.len() != p_bull_given_bear.len() {
            return Err(FinError::InvalidInput(
                "names, p_bull_given_bull, and p_bull_given_bear must have the same length".to_owned(),
            ));
        }
        for (i, (&pb, &pn)) in p_bull_given_bull.iter().zip(&p_bull_given_bear).enumerate() {
            if pb <= 0.0 || pb >= 1.0 {
                return Err(FinError::InvalidInput(format!(
                    "p_bull_given_bull[{i}] must be in (0,1), got {pb}"
                )));
            }
            if pn <= 0.0 || pn >= 1.0 {
                return Err(FinError::InvalidInput(format!(
                    "p_bull_given_bear[{i}] must be in (0,1), got {pn}"
                )));
            }
        }
        if prior <= 0.0 || prior >= 1.0 {
            return Err(FinError::InvalidInput(format!(
                "prior must be in (0,1), got {prior}"
            )));
        }
        Ok(Self {
            names,
            p_bull_given_bull,
            p_bull_given_bear,
            posterior: prior,
            epsilon: 1e-10,
        })
    }

    /// Returns the current posterior `P(market=Bull)`.
    pub fn posterior(&self) -> f64 {
        self.posterior
    }

    /// Resets the posterior to `prior`.
    pub fn reset(&mut self, prior: f64) {
        self.posterior = prior.clamp(self.epsilon, 1.0 - self.epsilon);
    }

    /// Returns signal names.
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// Returns the number of signals.
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// Returns `true` if no signals are registered.
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// Updates the posterior using Naive Bayes given the observed votes.
    ///
    /// `votes.len()` must equal `self.len()`. Neutral votes are ignored.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if `votes.len() != self.len()`.
    pub fn update(&mut self, votes: &[Vote]) -> Result<EnsembleOutput, FinError> {
        if votes.len() != self.names.len() {
            return Err(FinError::InvalidInput(format!(
                "expected {} votes, got {}",
                self.names.len(),
                votes.len()
            )));
        }

        // Compute log-odds update
        let mut log_odds = (self.posterior / (1.0 - self.posterior)).ln();

        for (i, vote) in votes.iter().enumerate() {
            let p_bull = self.p_bull_given_bull[i];
            let p_bear = self.p_bull_given_bear[i];
            let lr = match vote {
                Vote::Bull => p_bull / p_bear,
                Vote::Bear => (1.0 - p_bull) / (1.0 - p_bear),
                Vote::Neutral => 1.0, // no information
            };
            // Clamp lr to prevent extreme updates
            let lr_clamped = lr.clamp(1e-6, 1e6);
            log_odds += lr_clamped.ln();
        }

        let new_posterior = 1.0 / (1.0 + (-log_odds).exp());
        self.posterior = new_posterior.clamp(self.epsilon, 1.0 - self.epsilon);

        let direction = if self.posterior > 0.5 {
            Vote::Bull
        } else if self.posterior < 0.5 {
            Vote::Bear
        } else {
            Vote::Neutral
        };

        // Score in [-1, 1]: map [0,1] posterior to [-1,1]
        let score = 2.0 * self.posterior - 1.0;
        let confidence = (self.posterior - 0.5).abs() * 2.0;

        Ok(EnsembleOutput { direction, score, confidence })
    }
}

/// Convenience ensemble wrapper that owns any combiner type behind a common interface.
///
/// For heterogeneous ensembles, use the individual combiner structs directly.
#[derive(Debug)]
pub struct SignalEnsemble {
    /// Human-readable name for this ensemble.
    pub name: String,
}

impl SignalEnsemble {
    /// Creates a named ensemble (descriptive wrapper only; use typed combiners for logic).
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── VotingCombiner tests ──────────────────────────────────────────────

    #[test]
    fn test_voting_majority_bull() {
        let vc = VotingCombiner::new(vec!["a".into(), "b".into(), "c".into()]).unwrap();
        let out = vc.update(&[Vote::Bull, Vote::Bull, Vote::Bear]).unwrap();
        assert_eq!(out.direction, Vote::Bull);
        assert!(out.score > 0.0);
    }

    #[test]
    fn test_voting_majority_bear() {
        let vc = VotingCombiner::new(vec!["a".into(), "b".into(), "c".into()]).unwrap();
        let out = vc.update(&[Vote::Bear, Vote::Bear, Vote::Bull]).unwrap();
        assert_eq!(out.direction, Vote::Bear);
        assert!(out.score < 0.0);
    }

    #[test]
    fn test_voting_tie_is_neutral() {
        let vc = VotingCombiner::new(vec!["a".into(), "b".into()]).unwrap();
        let out = vc.update(&[Vote::Bull, Vote::Bear]).unwrap();
        assert_eq!(out.direction, Vote::Neutral);
    }

    #[test]
    fn test_voting_empty_fails() {
        assert!(VotingCombiner::new(vec![]).is_err());
    }

    #[test]
    fn test_voting_wrong_length_fails() {
        let vc = VotingCombiner::new(vec!["a".into(), "b".into()]).unwrap();
        assert!(vc.update(&[Vote::Bull]).is_err());
    }

    // ── WeightedCombiner tests ────────────────────────────────────────────

    #[test]
    fn test_weighted_bull() {
        let wc = WeightedCombiner::new(
            vec!["a".into(), "b".into()],
            vec![0.7, 0.3],
            0.2,
            -0.2,
        )
        .unwrap();
        let out = wc.update(&[1.0, 0.5]).unwrap();
        assert_eq!(out.direction, Vote::Bull);
        assert!(out.score > 0.2);
    }

    #[test]
    fn test_weighted_bear() {
        let wc = WeightedCombiner::new(
            vec!["a".into(), "b".into()],
            vec![0.5, 0.5],
            0.2,
            -0.2,
        )
        .unwrap();
        let out = wc.update(&[-1.0, -0.8]).unwrap();
        assert_eq!(out.direction, Vote::Bear);
        assert!(out.score < -0.2);
    }

    #[test]
    fn test_weighted_neutral_in_deadzone() {
        let wc = WeightedCombiner::new(
            vec!["a".into()],
            vec![1.0],
            0.5,
            -0.5,
        )
        .unwrap();
        let out = wc.update(&[0.1]).unwrap();
        assert_eq!(out.direction, Vote::Neutral);
    }

    #[test]
    fn test_weighted_negative_weight_fails() {
        assert!(WeightedCombiner::new(
            vec!["a".into()],
            vec![-0.1],
            0.2,
            -0.2,
        )
        .is_err());
    }

    #[test]
    fn test_weighted_zero_total_weight_fails() {
        assert!(WeightedCombiner::new(
            vec!["a".into(), "b".into()],
            vec![0.0, 0.0],
            0.2,
            -0.2,
        )
        .is_err());
    }

    #[test]
    fn test_weighted_wrong_length_fails() {
        let wc = WeightedCombiner::new(
            vec!["a".into()],
            vec![1.0],
            0.2,
            -0.2,
        )
        .unwrap();
        assert!(wc.update(&[0.5, 0.3]).is_err());
    }

    // ── BayesianCombiner tests ────────────────────────────────────────────

    #[test]
    fn test_bayesian_bull_votes_increase_posterior() {
        let mut bc = BayesianCombiner::new(
            vec!["a".into(), "b".into()],
            vec![0.7, 0.65],
            vec![0.3, 0.35],
            0.5,
        )
        .unwrap();
        let out = bc.update(&[Vote::Bull, Vote::Bull]).unwrap();
        assert_eq!(out.direction, Vote::Bull);
        assert!(bc.posterior() > 0.5);
    }

    #[test]
    fn test_bayesian_bear_votes_decrease_posterior() {
        let mut bc = BayesianCombiner::new(
            vec!["a".into()],
            vec![0.7],
            vec![0.3],
            0.5,
        )
        .unwrap();
        let out = bc.update(&[Vote::Bear]).unwrap();
        assert_eq!(out.direction, Vote::Bear);
        assert!(bc.posterior() < 0.5);
    }

    #[test]
    fn test_bayesian_neutral_vote_no_change() {
        let mut bc = BayesianCombiner::new(
            vec!["a".into()],
            vec![0.7],
            vec![0.3],
            0.6,
        )
        .unwrap();
        let before = bc.posterior();
        let out = bc.update(&[Vote::Neutral]).unwrap();
        assert!((bc.posterior() - before).abs() < 1e-10);
        assert_eq!(out.direction, Vote::Bull);
    }

    #[test]
    fn test_bayesian_invalid_prior_fails() {
        assert!(BayesianCombiner::new(vec!["a".into()], vec![0.7], vec![0.3], 0.0).is_err());
        assert!(BayesianCombiner::new(vec!["a".into()], vec![0.7], vec![0.3], 1.0).is_err());
    }

    #[test]
    fn test_bayesian_invalid_likelihood_fails() {
        assert!(BayesianCombiner::new(vec!["a".into()], vec![0.0], vec![0.3], 0.5).is_err());
        assert!(BayesianCombiner::new(vec!["a".into()], vec![0.7], vec![1.0], 0.5).is_err());
    }

    #[test]
    fn test_bayesian_reset() {
        let mut bc = BayesianCombiner::new(
            vec!["a".into()],
            vec![0.7],
            vec![0.3],
            0.5,
        )
        .unwrap();
        bc.update(&[Vote::Bull]).unwrap();
        let after_update = bc.posterior();
        bc.reset(0.5);
        assert!((bc.posterior() - 0.5).abs() < 1e-6);
        assert_ne!(after_update, 0.5);
    }

    #[test]
    fn test_vote_as_f64() {
        assert_eq!(Vote::Bull.as_f64(), 1.0);
        assert_eq!(Vote::Bear.as_f64(), -1.0);
        assert_eq!(Vote::Neutral.as_f64(), 0.0);
    }

    #[test]
    fn test_vote_from_f64() {
        assert_eq!(Vote::from_f64(0.5), Vote::Bull);
        assert_eq!(Vote::from_f64(-0.5), Vote::Bear);
        assert_eq!(Vote::from_f64(0.0), Vote::Neutral);
    }

    #[test]
    fn test_signal_ensemble_new() {
        let se = SignalEnsemble::new("my_ensemble");
        assert_eq!(se.name, "my_ensemble");
    }
}
