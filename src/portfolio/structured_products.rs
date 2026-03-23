//! CDO tranching, waterfall distributions, and Monte Carlo pricing.

/// A single tranche in a CDO structure.
#[derive(Clone, Debug)]
pub struct Tranche {
    /// Name of the tranche (e.g. "Super Senior", "Mezzanine", "Equity").
    pub name: String,
    /// Lower attachment point as a fraction of total notional (0..1).
    pub attachment: f64,
    /// Upper detachment point as a fraction of total notional (0..1).
    pub detachment: f64,
    /// Face value of this tranche.
    pub notional: f64,
    /// Coupon spread over risk-free rate (annualised fraction).
    pub coupon_spread: f64,
}

impl Tranche {
    /// Thickness of the tranche (detachment - attachment).
    pub fn thickness(&self) -> f64 {
        self.detachment - self.attachment
    }
}

/// CDO structure with tranches and portfolio parameters.
pub struct CdoStructure {
    /// All tranches in the CDO, ordered from most senior to most junior.
    pub tranches: Vec<Tranche>,
    /// Total notional of the reference portfolio.
    pub total_notional: f64,
    /// Recovery rate assumed for defaulted assets (0..1).
    pub recovery_rate: f64,
}

impl CdoStructure {
    /// Compute the loss allocated to each tranche given a portfolio loss rate.
    ///
    /// `portfolio_loss_rate` — fraction of total notional that has been lost (0..1).
    /// Returns a vector of loss amounts (in notional terms) per tranche.
    pub fn loss_to_tranche(&self, portfolio_loss_rate: f64) -> Vec<f64> {
        self.tranches
            .iter()
            .map(|t| {
                let loss_below_attachment = portfolio_loss_rate.min(t.attachment);
                let loss_up_to_detachment = portfolio_loss_rate.min(t.detachment);
                let tranche_loss_rate = (loss_up_to_detachment - loss_below_attachment)
                    / t.thickness().max(1e-12);
                tranche_loss_rate * t.notional
            })
            .collect()
    }
}

/// Senior-first cashflow waterfall engine.
pub struct WaterfallEngine {
    /// Tranches ordered from most senior to most junior.
    pub tranches: Vec<Tranche>,
}

impl WaterfallEngine {
    /// Distribute `available` cash senior-first across tranches.
    ///
    /// Returns the amount paid to each tranche in order.
    pub fn distribute_cashflow(&self, available: f64) -> Vec<f64> {
        let mut remaining = available;
        self.tranches
            .iter()
            .map(|t| {
                let coupon = t.notional * t.coupon_spread;
                let paid = coupon.min(remaining);
                remaining = (remaining - paid).max(0.0);
                paid
            })
            .collect()
    }
}

/// Monte Carlo CDO pricer using a Gaussian one-factor copula.
pub struct MonteCarloCdo {
    /// The CDO structure containing tranches and portfolio parameters.
    pub structure: CdoStructure,
    /// Number of Monte Carlo simulation paths.
    pub num_simulations: u64,
}

impl MonteCarloCdo {
    /// Price CDO tranches via Monte Carlo with Gaussian copula factor model.
    ///
    /// `default_probs` — PD for each reference entity (length N).
    /// `correlation`   — asset correlation (rho), shared factor loading.
    /// `seed`          — LCG seed for reproducibility.
    ///
    /// Returns average tranche loss rate for each tranche.
    pub fn price_tranches(
        &self,
        default_probs: &[f64],
        correlation: f64,
        seed: u64,
    ) -> Vec<f64> {
        let n = default_probs.len();
        if n == 0 {
            return vec![0.0; self.structure.tranches.len()];
        }

        // Precompute default thresholds: Phi^-1(pd_i)
        let thresholds: Vec<f64> = default_probs.iter().map(|&p| probit(p)).collect();

        let sqrt_rho = correlation.sqrt();
        let sqrt_one_minus_rho = (1.0 - correlation).sqrt();

        let mut tranche_loss_acc = vec![0.0f64; self.structure.tranches.len()];
        let mut state = seed;

        for _ in 0..self.num_simulations {
            // Draw common factor M ~ N(0,1)
            let (m, _) = box_muller(&mut state);

            // For each asset, draw idiosyncratic shock and check default
            let mut n_defaults = 0u64;
            let mut portfolio_loss = 0.0f64;
            let unit_loss = (1.0 - self.structure.recovery_rate) / n as f64;

            for (i, &thr) in thresholds.iter().enumerate() {
                let (eps, _) = box_muller(&mut state);
                let z_i = sqrt_rho * m + sqrt_one_minus_rho * eps;
                if z_i < thr {
                    n_defaults += 1;
                    portfolio_loss += unit_loss;
                }
            }
            let _ = n_defaults;

            let tranche_losses = self.structure.loss_to_tranche(portfolio_loss);
            for (acc, loss) in tranche_loss_acc.iter_mut().zip(tranche_losses.iter()) {
                *acc += loss;
            }
        }

        tranche_loss_acc
            .iter()
            .zip(self.structure.tranches.iter())
            .map(|(&acc, t)| acc / (self.num_simulations as f64 * t.notional.max(1e-12)))
            .collect()
    }
}

/// Box-Muller transform: generates two independent N(0,1) samples from LCG state.
fn box_muller(state: &mut u64) -> (f64, f64) {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let u1 = ((*state >> 11) as f64 + 1.0) / ((1u64 << 53) as f64 + 1.0);
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let u2 = (*state >> 11) as f64 / (1u64 << 53) as f64;
    let r = (-2.0 * u1.ln()).sqrt();
    let theta = std::f64::consts::TAU * u2;
    (r * theta.cos(), r * theta.sin())
}

/// Rational approximation of the probit (inverse normal CDF) function.
/// Abramowitz & Stegun approximation.
fn probit(p: f64) -> f64 {
    let p = p.clamp(1e-12, 1.0 - 1e-12);
    let t = if p < 0.5 {
        (-2.0 * p.ln()).sqrt()
    } else {
        (-2.0 * (1.0 - p).ln()).sqrt()
    };
    let c = [2.515517, 0.802853, 0.010328];
    let d = [1.432788, 0.189269, 0.001308];
    let num = c[0] + c[1] * t + c[2] * t * t;
    let den = 1.0 + d[0] * t + d[1] * t * t + d[2] * t * t * t;
    let x = t - num / den;
    if p < 0.5 { -x } else { x }
}
