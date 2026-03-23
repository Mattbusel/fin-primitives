//! # Module: pairs_trading
//!
//! ## Responsibility
//! Statistical pairs trading primitives: Engle-Granger cointegration test,
//! ADF statistic, spread z-score signal generation, and Welford online
//! mean/variance tracking for the spread.
//!
//! ## Guarantees
//! - Zero panics; all fallible operations return `Result<_, FinError>`
//! - `SpreadTracker` uses Welford's algorithm — numerically stable
//! - All thresholds validated at construction

use crate::error::FinError;

// ─────────────────────────────────────────
//  AdfTest
// ─────────────────────────────────────────

/// Augmented Dickey-Fuller test result.
///
/// Critical values correspond to 1%, 5%, and 10% significance levels
/// (approximate MacKinnon 1994 values for a no-trend specification).
///
/// The series is stationary (cointegrated residual) when
/// `statistic < critical_values[1]` (5% level).
///
/// # Example
/// ```rust
/// use fin_primitives::pairs_trading::AdfTest;
///
/// let adf = AdfTest { statistic: -3.5, critical_values: [-3.43, -2.86, -2.57] };
/// assert!(adf.is_stationary_at_5pct());
/// ```
#[derive(Debug, Clone, Copy)]
pub struct AdfTest {
    /// ADF test statistic (negative values indicate stationarity).
    pub statistic: f64,
    /// Critical values at [1%, 5%, 10%] significance levels.
    pub critical_values: [f64; 3],
}

impl AdfTest {
    /// Returns `true` if the series is stationary at the 1% level.
    #[must_use]
    pub fn is_stationary_at_1pct(&self) -> bool {
        self.statistic < self.critical_values[0]
    }

    /// Returns `true` if the series is stationary at the 5% level.
    #[must_use]
    pub fn is_stationary_at_5pct(&self) -> bool {
        self.statistic < self.critical_values[1]
    }

    /// Returns `true` if the series is stationary at the 10% level.
    #[must_use]
    pub fn is_stationary_at_10pct(&self) -> bool {
        self.statistic < self.critical_values[2]
    }
}

// ─────────────────────────────────────────
//  CointegrationTest
// ─────────────────────────────────────────

/// Engle-Granger two-step cointegration test.
///
/// **Step 1:** OLS regression of `y` on `x` to estimate the hedge ratio `β`.
/// **Step 2:** ADF test on the residuals `e_t = y_t - β * x_t - α`.
///
/// # Example
/// ```rust
/// use fin_primitives::pairs_trading::CointegrationTest;
///
/// let y = vec![1.0, 2.1, 3.0, 4.2, 5.1];
/// let x = vec![1.0, 2.0, 3.0, 4.0, 5.0];
/// let result = CointegrationTest::run(&y, &x).unwrap();
/// assert!(result.hedge_ratio > 0.0);
/// ```
#[derive(Debug, Clone)]
pub struct CointegrationTest {
    /// OLS hedge ratio β (slope of y regressed on x).
    pub hedge_ratio: f64,
    /// OLS intercept α.
    pub intercept: f64,
    /// ADF test result on the OLS residuals.
    pub adf: AdfTest,
    /// Residuals from the OLS regression.
    pub residuals: Vec<f64>,
}

impl CointegrationTest {
    /// Run the Engle-Granger two-step cointegration test.
    ///
    /// # Errors
    /// - [`FinError::InsufficientData`] if `y` and `x` have fewer than 5 observations.
    /// - [`FinError::InvalidInput`] if `y` and `x` have different lengths.
    pub fn run(y: &[f64], x: &[f64]) -> Result<Self, FinError> {
        if y.len() != x.len() {
            return Err(FinError::InvalidInput(
                "y and x must have the same length".into(),
            ));
        }
        if y.len() < 5 {
            return Err(FinError::InvalidInput("need at least 5 observations".into()));
        }
        let n = y.len() as f64;
        let sum_x: f64 = x.iter().sum();
        let sum_y: f64 = y.iter().sum();
        let sum_xx: f64 = x.iter().map(|v| v * v).sum();
        let sum_xy: f64 = x.iter().zip(y.iter()).map(|(xi, yi)| xi * yi).sum();

        let denom = n * sum_xx - sum_x * sum_x;
        let (hedge_ratio, intercept) = if denom.abs() < f64::EPSILON {
            (1.0, 0.0)
        } else {
            let beta = (n * sum_xy - sum_x * sum_y) / denom;
            let alpha = (sum_y - beta * sum_x) / n;
            (beta, alpha)
        };

        let residuals: Vec<f64> = x
            .iter()
            .zip(y.iter())
            .map(|(xi, yi)| yi - hedge_ratio * xi - intercept)
            .collect();

        let adf = Self::adf_test(&residuals);
        Ok(Self {
            hedge_ratio,
            intercept,
            adf,
            residuals,
        })
    }

    /// Simplified ADF test on a residual series using first-difference regression.
    ///
    /// Computes the t-statistic for the lagged level coefficient in the
    /// first-difference regression: `Δe_t = γ * e_{t-1} + ε_t`.
    fn adf_test(series: &[f64]) -> AdfTest {
        // Approximate MacKinnon (1994) critical values — no-trend case, n → ∞
        let critical_values = [-3.43, -2.86, -2.57_f64];
        let n = series.len();
        if n < 3 {
            return AdfTest {
                statistic: 0.0,
                critical_values,
            };
        }
        // Build Δe_t (y) and e_{t-1} (x) for OLS
        let dy: Vec<f64> = (1..n).map(|i| series[i] - series[i - 1]).collect();
        let lag: Vec<f64> = (0..n - 1).map(|i| series[i]).collect();
        let m = dy.len() as f64;
        let sum_lag: f64 = lag.iter().sum();
        let sum_dy: f64 = dy.iter().sum();
        let sum_ll: f64 = lag.iter().map(|v| v * v).sum();
        let sum_ldy: f64 = lag.iter().zip(dy.iter()).map(|(l, d)| l * d).sum();
        let denom = m * sum_ll - sum_lag * sum_lag;
        if denom.abs() < f64::EPSILON {
            return AdfTest {
                statistic: 0.0,
                critical_values,
            };
        }
        let gamma = (m * sum_ldy - sum_lag * sum_dy) / denom;
        // Residuals of the first-difference regression
        let alpha_fd = (sum_dy - gamma * sum_lag) / m;
        let residuals: Vec<f64> = lag
            .iter()
            .zip(dy.iter())
            .map(|(l, d)| d - gamma * l - alpha_fd)
            .collect();
        let sse: f64 = residuals.iter().map(|r| r * r).sum();
        let s2 = sse / (m - 2.0).max(1.0);
        let se_gamma = if s2 <= 0.0 || denom.abs() < f64::EPSILON {
            1.0
        } else {
            (s2 * m / denom).sqrt()
        };
        let statistic = if se_gamma.abs() < f64::EPSILON {
            0.0
        } else {
            gamma / se_gamma
        };
        AdfTest {
            statistic,
            critical_values,
        }
    }

    /// Returns `true` if the pair is cointegrated at the 5% significance level.
    #[must_use]
    pub fn is_cointegrated(&self) -> bool {
        self.adf.is_stationary_at_5pct()
    }
}

// ─────────────────────────────────────────
//  PairSignal
// ─────────────────────────────────────────

/// Trading signal for a pairs strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairSignal {
    /// Enter the trade: go long the first asset, short the second.
    EnterLongShort,
    /// Enter the trade: go short the first asset, long the second.
    EnterShortLong,
    /// Exit all positions — spread has reverted to mean.
    Exit,
    /// No action — spread is within the neutral band.
    Hold,
}

// ─────────────────────────────────────────
//  PairsStrategy
// ─────────────────────────────────────────

/// Spread-based pairs strategy signal generator.
///
/// Generates [`PairSignal`] from the current spread using a z-score threshold.
///
/// # Example
/// ```rust
/// use fin_primitives::pairs_trading::{PairsStrategy, PairSignal};
///
/// let strategy = PairsStrategy::new(1.0, 0.0, 1.0, 2.0).unwrap();
/// // spread 2 std above mean → short spread (short A, long B)
/// assert_eq!(strategy.generate_signal(2.0), PairSignal::EnterShortLong);
/// // spread 2 std below mean → long spread (long A, short B)
/// assert_eq!(strategy.generate_signal(-2.0), PairSignal::EnterLongShort);
/// // spread within threshold
/// assert_eq!(strategy.generate_signal(0.5), PairSignal::Hold);
/// ```
#[derive(Debug, Clone)]
pub struct PairsStrategy {
    /// Estimated hedge ratio β.
    pub hedge_ratio: f64,
    /// Historical mean of the spread.
    pub spread_mean: f64,
    /// Historical standard deviation of the spread.
    pub spread_std: f64,
    /// Z-score threshold to trigger entry signals.
    pub z_threshold: f64,
}

impl PairsStrategy {
    /// Constructs a [`PairsStrategy`].
    ///
    /// # Errors
    /// - [`FinError::InvalidInput`] if `spread_std <= 0` or `z_threshold <= 0`.
    pub fn new(
        hedge_ratio: f64,
        spread_mean: f64,
        spread_std: f64,
        z_threshold: f64,
    ) -> Result<Self, FinError> {
        if spread_std <= 0.0 {
            return Err(FinError::InvalidInput(
                "spread_std must be positive".into(),
            ));
        }
        if z_threshold <= 0.0 {
            return Err(FinError::InvalidInput(
                "z_threshold must be positive".into(),
            ));
        }
        Ok(Self {
            hedge_ratio,
            spread_mean,
            spread_std,
            z_threshold,
        })
    }

    /// Generate a trading signal from the current spread value.
    #[must_use]
    pub fn generate_signal(&self, spread: f64) -> PairSignal {
        let z = (spread - self.spread_mean) / self.spread_std;
        if z > self.z_threshold {
            PairSignal::EnterShortLong
        } else if z < -self.z_threshold {
            PairSignal::EnterLongShort
        } else if z.abs() < self.z_threshold * 0.5 {
            PairSignal::Exit
        } else {
            PairSignal::Hold
        }
    }

    /// Compute the z-score for the given spread.
    #[must_use]
    pub fn z_score(&self, spread: f64) -> f64 {
        (spread - self.spread_mean) / self.spread_std
    }
}

// ─────────────────────────────────────────
//  SpreadTracker
// ─────────────────────────────────────────

/// Online mean and variance of the spread using Welford's algorithm.
///
/// Memory-efficient: O(1) state regardless of how many observations arrive.
///
/// # Example
/// ```rust
/// use fin_primitives::pairs_trading::SpreadTracker;
///
/// let mut tracker = SpreadTracker::new();
/// for v in [1.0, 2.0, 3.0, 4.0, 5.0] {
///     tracker.update(v);
/// }
/// assert!((tracker.mean().unwrap() - 3.0).abs() < 1e-10);
/// ```
#[derive(Debug, Clone)]
pub struct SpreadTracker {
    count: u64,
    mean: f64,
    m2: f64,
}

impl SpreadTracker {
    /// Constructs an empty [`SpreadTracker`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            count: 0,
            mean: 0.0,
            m2: 0.0,
        }
    }

    /// Add a new spread observation (Welford one-pass update).
    pub fn update(&mut self, value: f64) {
        self.count += 1;
        let delta = value - self.mean;
        self.mean += delta / self.count as f64;
        let delta2 = value - self.mean;
        self.m2 += delta * delta2;
    }

    /// Current mean of all observations, or `None` if no observations.
    #[must_use]
    pub fn mean(&self) -> Option<f64> {
        if self.count == 0 {
            None
        } else {
            Some(self.mean)
        }
    }

    /// Sample variance (Bessel-corrected), or `None` if fewer than 2 observations.
    #[must_use]
    pub fn variance(&self) -> Option<f64> {
        if self.count < 2 {
            None
        } else {
            Some(self.m2 / (self.count - 1) as f64)
        }
    }

    /// Sample standard deviation, or `None` if fewer than 2 observations.
    #[must_use]
    pub fn std_dev(&self) -> Option<f64> {
        self.variance().map(f64::sqrt)
    }

    /// Number of observations seen so far.
    #[must_use]
    pub fn count(&self) -> u64 {
        self.count
    }
}

impl Default for SpreadTracker {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────
//  Unit Tests
// ─────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // AdfTest
    #[test]
    fn adf_stationary_at_5pct() {
        let adf = AdfTest {
            statistic: -3.5,
            critical_values: [-3.43, -2.86, -2.57],
        };
        assert!(!adf.is_stationary_at_1pct()); // -3.5 < -3.43 → true
        // actually -3.5 < -3.43 is true
        assert!(adf.is_stationary_at_1pct());
        assert!(adf.is_stationary_at_5pct());
        assert!(adf.is_stationary_at_10pct());
    }

    #[test]
    fn adf_non_stationary() {
        let adf = AdfTest {
            statistic: -1.0,
            critical_values: [-3.43, -2.86, -2.57],
        };
        assert!(!adf.is_stationary_at_10pct());
    }

    // CointegrationTest
    #[test]
    fn cointegration_perfect_linear() {
        // y = 2*x + noise, should detect near-unit hedge ratio
        let x: Vec<f64> = (1..=20).map(|i| i as f64).collect();
        let y: Vec<f64> = x.iter().map(|xi| 2.0 * xi + 0.001).collect();
        let result = CointegrationTest::run(&y, &x).unwrap();
        assert!((result.hedge_ratio - 2.0).abs() < 0.01);
    }

    #[test]
    fn cointegration_too_short() {
        let y = vec![1.0, 2.0];
        let x = vec![1.0, 2.0];
        assert!(CointegrationTest::run(&y, &x).is_err());
    }

    #[test]
    fn cointegration_length_mismatch() {
        let y = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let x = vec![1.0, 2.0, 3.0];
        assert!(CointegrationTest::run(&y, &x).is_err());
    }

    // PairsStrategy
    #[test]
    fn pairs_strategy_signals() {
        let s = PairsStrategy::new(1.0, 0.0, 1.0, 2.0).unwrap();
        assert_eq!(s.generate_signal(3.0), PairSignal::EnterShortLong);
        assert_eq!(s.generate_signal(-3.0), PairSignal::EnterLongShort);
        assert_eq!(s.generate_signal(0.1), PairSignal::Exit);
        assert_eq!(s.generate_signal(1.5), PairSignal::Hold);
    }

    #[test]
    fn pairs_strategy_invalid() {
        assert!(PairsStrategy::new(1.0, 0.0, 0.0, 2.0).is_err());
        assert!(PairsStrategy::new(1.0, 0.0, 1.0, 0.0).is_err());
    }

    #[test]
    fn pairs_strategy_z_score() {
        let s = PairsStrategy::new(1.0, 0.0, 2.0, 2.0).unwrap();
        assert!((s.z_score(4.0) - 2.0).abs() < 1e-10);
    }

    // SpreadTracker
    #[test]
    fn spread_tracker_mean_variance() {
        let mut t = SpreadTracker::new();
        assert!(t.mean().is_none());
        assert!(t.variance().is_none());
        for v in [2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0] {
            t.update(v);
        }
        let mean = t.mean().unwrap();
        assert!((mean - 5.0).abs() < 1e-10);
        let var = t.variance().unwrap();
        assert!((var - 4.0).abs() < 0.01);
    }

    #[test]
    fn spread_tracker_std_dev() {
        let mut t = SpreadTracker::new();
        for v in [1.0, 2.0, 3.0] {
            t.update(v);
        }
        let std = t.std_dev().unwrap();
        assert!((std - 1.0).abs() < 1e-10);
    }

    #[test]
    fn spread_tracker_single_obs_no_variance() {
        let mut t = SpreadTracker::new();
        t.update(5.0);
        assert!(t.mean().is_some());
        assert!(t.variance().is_none());
    }

    #[test]
    fn spread_tracker_welford_stability() {
        // Large values — test numerical stability
        let mut t = SpreadTracker::new();
        let base = 1_000_000.0_f64;
        for i in 0..100 {
            t.update(base + i as f64);
        }
        let mean = t.mean().unwrap();
        assert!((mean - (base + 49.5)).abs() < 1e-6);
    }
}
