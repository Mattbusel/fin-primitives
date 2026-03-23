//! Credit risk models: scoring, expected loss, VaR, z-spread, and migration matrices.

/// Credit scorecard for PD estimation via weighted attribute scoring.
pub struct CreditScorecard {
    /// Score component based on borrower age (higher = better).
    pub age_score: f64,
    /// Score component based on annual income (higher = better).
    pub income_score: f64,
    /// Score component based on debt-to-income ratio (higher = better).
    pub debt_ratio_score: f64,
    /// Score component based on payment history (higher = better).
    pub payment_history_score: f64,
    /// Score component based on credit utilization rate (higher = better).
    pub credit_utilization_score: f64,
}

impl CreditScorecard {
    /// Weighted sum of all component scores.
    pub fn total_score(&self) -> f64 {
        self.age_score * 0.10
            + self.income_score * 0.25
            + self.debt_ratio_score * 0.20
            + self.payment_history_score * 0.35
            + self.credit_utilization_score * 0.10
    }

    /// Logistic probability of default: 1 / (1 + exp((total_score - 500) / 50)).
    pub fn pd_estimate(&self) -> f64 {
        let s = self.total_score();
        1.0 / (1.0 + ((s - 500.0) / 50.0).exp())
    }
}

/// Loan exposure details for expected-loss and EAD calculation.
pub struct LoanExposure {
    /// Original principal amount of the loan.
    pub principal: f64,
    /// Current outstanding balance.
    pub outstanding: f64,
    /// Total committed credit facility amount.
    pub committed: f64,
    /// Loss given default (0..1).
    pub lgd: f64,
    /// Remaining maturity in years.
    pub maturity_years: f64,
}

impl LoanExposure {
    /// Exposure at default: outstanding + 50% of undrawn committed facility.
    pub fn ead(&self) -> f64 {
        self.outstanding + 0.5 * (self.committed - self.outstanding).max(0.0)
    }

    /// Expected loss = EAD × LGD × PD.
    pub fn expected_loss(&self, pd: f64) -> f64 {
        self.ead() * self.lgd * pd
    }
}

/// Credit VaR via Monte Carlo simulation of Bernoulli defaults.
pub struct CreditVaR {
    /// Confidence level (e.g. 0.99 for 99% VaR).
    pub confidence: f64,
    /// Time horizon in years for the VaR calculation.
    pub time_horizon_years: f64,
}

impl CreditVaR {
    /// Monte Carlo credit VaR.
    ///
    /// `exposures` — slice of (LoanExposure, pd) pairs.
    /// Returns the loss at the (1 - confidence) percentile of the simulated loss distribution.
    pub fn calculate(exposures: &[(LoanExposure, f64)]) -> f64 {
        Self::calculate_with_params(exposures, 0.99, 42)
    }

    /// Calculate with explicit confidence and seed.
    pub fn calculate_with_params(
        exposures: &[(LoanExposure, f64)],
        confidence: f64,
        seed: u64,
    ) -> f64 {
        let n_sims: usize = 10_000;
        let mut losses = Vec::with_capacity(n_sims);
        let mut state = seed;

        for _ in 0..n_sims {
            let mut sim_loss = 0.0;
            for (exp, pd) in exposures {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let u = (state >> 11) as f64 / (1u64 << 53) as f64;
                if u < *pd {
                    sim_loss += exp.ead() * exp.lgd;
                }
            }
            losses.push(sim_loss);
        }

        losses.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let idx = ((1.0 - confidence) * n_sims as f64) as usize;
        let idx = idx.min(n_sims - 1);
        losses[idx]
    }
}

/// Z-spread calculator: finds the spread over a risk-free curve that equates
/// PV of cashflows to the market price.
pub struct ZSpreadCalculator;

impl ZSpreadCalculator {
    /// Bisection search for the z-spread.
    ///
    /// - `cashflows`: (time_years, amount) pairs
    /// - `market_price`: observed market price of the bond
    /// - `risk_free_curve`: (time_years, rate) pairs (linearly interpolated)
    pub fn z_spread(
        cashflows: &[(f64, f64)],
        market_price: f64,
        risk_free_curve: &[(f64, f64)],
    ) -> f64 {
        let pv = |spread: f64| -> f64 {
            cashflows
                .iter()
                .map(|(t, cf)| {
                    let rf = interpolate_rate(risk_free_curve, *t);
                    let disc = (-(rf + spread) * t).exp();
                    cf * disc
                })
                .sum::<f64>()
        };

        let mut lo = -0.10_f64;
        let mut hi = 0.50_f64;
        for _ in 0..60 {
            let mid = (lo + hi) / 2.0;
            if pv(mid) > market_price {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        (lo + hi) / 2.0
    }
}

fn interpolate_rate(curve: &[(f64, f64)], t: f64) -> f64 {
    if curve.is_empty() {
        return 0.0;
    }
    if t <= curve[0].0 {
        return curve[0].1;
    }
    if t >= curve[curve.len() - 1].0 {
        return curve[curve.len() - 1].1;
    }
    for i in 1..curve.len() {
        if t <= curve[i].0 {
            let (t0, r0) = curve[i - 1];
            let (t1, r1) = curve[i];
            let frac = (t - t0) / (t1 - t0);
            return r0 + frac * (r1 - r0);
        }
    }
    curve[curve.len() - 1].1
}

/// Credit migration matrix over a 1-year horizon.
pub struct CreditMigration {
    /// Row i, col j = probability of migrating from rating i to rating j.
    pub transition_matrix: Vec<Vec<f64>>,
    /// Human-readable labels for each rating bucket (e.g. "AAA", "AA", ...).
    pub rating_labels: Vec<String>,
}

impl CreditMigration {
    /// Standard 7-rating transition matrix (AAA, AA, A, BBB, BB, B, CCC).
    pub fn from_standard() -> Self {
        let labels = ["AAA", "AA", "A", "BBB", "BB", "B", "CCC"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        // Each row sums to 1.0
        let matrix = vec![
            vec![0.9081, 0.0833, 0.0068, 0.0006, 0.0008, 0.0002, 0.0001, 0.0001],
            vec![0.0070, 0.9065, 0.0779, 0.0064, 0.0006, 0.0013, 0.0001, 0.0002],
            vec![0.0009, 0.0227, 0.9105, 0.0552, 0.0074, 0.0026, 0.0001, 0.0006],
            vec![0.0002, 0.0033, 0.0595, 0.8693, 0.0530, 0.0117, 0.0012, 0.0018],
            vec![0.0003, 0.0014, 0.0067, 0.0773, 0.8053, 0.0884, 0.0100, 0.0106],
            vec![0.0000, 0.0011, 0.0024, 0.0043, 0.0648, 0.8346, 0.0407, 0.0521],
            vec![0.0022, 0.0000, 0.0022, 0.0130, 0.0238, 0.1117, 0.6490, 0.1981],
        ];

        // Truncate each row to 7 elements (drop default column for migration purposes)
        let matrix = matrix
            .into_iter()
            .map(|row| row[..7].to_vec())
            .collect();

        CreditMigration {
            transition_matrix: matrix,
            rating_labels: labels,
        }
    }

    /// Migrate a rating using cumulative probability lookup.
    ///
    /// `rand` must be in [0, 1). Returns the new rating index.
    pub fn migrate_one_year(&self, rating_idx: usize, rand: f64) -> usize {
        let row = &self.transition_matrix[rating_idx];
        let mut cumulative = 0.0;
        for (j, &p) in row.iter().enumerate() {
            cumulative += p;
            if rand < cumulative {
                return j;
            }
        }
        row.len() - 1
    }
}
