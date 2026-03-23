//! Variance swap pricing and realized variance tracking.
//!
//! Provides [`VarianceSwap`] for P&L / fair-strike / replication-cost calculations,
//! [`RealizedVarianceTracker`] for rolling log-return–based variance estimation, and
//! [`VixReplication`] for VIX-formula replication from option strips.

use std::collections::VecDeque;

// ─────────────────────────────────────────────────────────────────────────────
//  VarianceSwap
// ─────────────────────────────────────────────────────────────────────────────

/// A variance swap contract.
///
/// The holder receives `notional * (realized_variance - strike_variance)` at
/// maturity.  The [`vega_notional`](VarianceSwap::vega_notional) field stores
/// the conventional vega-notional used when quoting the trade.
#[derive(Debug, Clone)]
pub struct VarianceSwap {
    /// Variance notional in currency units (notional per unit of variance).
    pub notional: f64,
    /// Strike variance (annualised, e.g. 0.04 for a 20 vol strike).
    pub strike_variance: f64,
    /// Time to maturity in years.
    pub maturity_years: f64,
    /// Vega notional (informational; derived as `notional * 2 * sqrt(strike_variance)`).
    pub vega_notional: f64,
}

impl VarianceSwap {
    /// P&L at maturity given `realized_variance`.
    ///
    /// ```text
    /// pnl = notional * (realized_variance - strike_variance)
    /// ```
    pub fn pnl(&self, realized_variance: f64) -> f64 {
        self.notional * (realized_variance - self.strike_variance)
    }

    /// Fair strike implied by the vol surface.
    ///
    /// ```text
    /// fair_strike = sigma²_atm * (1 + skew_adjustment)
    /// ```
    pub fn fair_strike(vol_surface_atm: f64, skew_adjustment: f64) -> f64 {
        vol_surface_atm * vol_surface_atm * (1.0 + skew_adjustment)
    }

    /// Theoretical replication cost (present value of the fixed leg).
    ///
    /// ```text
    /// replication_cost = e^(-r * T) * notional * strike_variance
    /// ```
    pub fn replication_cost(&self, r: f64) -> f64 {
        (-r * self.maturity_years).exp() * self.notional * self.strike_variance
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  RealizedVarianceTracker
// ─────────────────────────────────────────────────────────────────────────────

/// Tracks realised variance from a rolling window of log returns.
///
/// Log returns are computed from consecutive price observations pushed via
/// [`push_price`](RealizedVarianceTracker::push_price).
#[derive(Debug, Clone)]
pub struct RealizedVarianceTracker {
    /// Rolling window of log returns.
    log_returns: VecDeque<f64>,
    /// Window length in trading days.
    pub period_days: u32,
    /// When `true`, variance is annualised by multiplying by 252.
    pub annualized: bool,
    /// Last price seen (needed to compute the next log return).
    last_price: Option<f64>,
}

impl RealizedVarianceTracker {
    /// Create a new tracker for a rolling window of `period_days` trading days.
    pub fn new(period_days: u32) -> Self {
        Self {
            log_returns: VecDeque::with_capacity(period_days as usize + 1),
            period_days,
            annualized: true,
            last_price: None,
        }
    }

    /// Ingest a new price observation; computes and stores the log return.
    pub fn push_price(&mut self, price: f64) {
        if price <= 0.0 {
            return;
        }
        if let Some(prev) = self.last_price {
            if prev > 0.0 {
                let lr = (price / prev).ln();
                self.log_returns.push_back(lr);
                if self.log_returns.len() > self.period_days as usize {
                    self.log_returns.pop_front();
                }
            }
        }
        self.last_price = Some(price);
    }

    /// Realised variance over the current window.
    ///
    /// Returns the sum of squared log returns, annualised by 252 if
    /// [`annualized`](RealizedVarianceTracker::annualized) is `true`.
    pub fn realized_variance(&self) -> f64 {
        let n = self.log_returns.len();
        if n == 0 {
            return 0.0;
        }
        let sum_sq: f64 = self.log_returns.iter().map(|r| r * r).sum();
        let daily_var = sum_sq / n as f64;
        if self.annualized {
            daily_var * 252.0
        } else {
            daily_var
        }
    }

    /// Realised volatility: `sqrt(realized_variance())`.
    pub fn realized_vol(&self) -> f64 {
        self.realized_variance().sqrt()
    }

    /// Variance accumulated so far in the current period (not yet annualised by
    /// period length — useful for partial-period mark-to-market).
    pub fn current_period_variance(&self) -> f64 {
        let sum_sq: f64 = self.log_returns.iter().map(|r| r * r).sum();
        if self.annualized {
            sum_sq * 252.0
        } else {
            sum_sq
        }
    }

    /// Number of log-return observations currently held.
    pub fn days_elapsed(&self) -> usize {
        self.log_returns.len()
    }

    /// Mark-to-market P&L on `swap` using the current realised variance.
    pub fn pnl_mtm(&self, swap: &VarianceSwap) -> f64 {
        swap.pnl(self.realized_variance())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  VixReplication
// ─────────────────────────────────────────────────────────────────────────────

/// VIX replication and analytic approximation utilities.
pub struct VixReplication;

impl VixReplication {
    /// Compute VIX² from a strip of option prices using the CBOE formula.
    ///
    /// `option_prices` is a slice of `(strike, call_price, put_price)`.
    /// `f` is the forward price, `r` the risk-free rate, `t` the expiry in years.
    ///
    /// ```text
    /// VIX² = (2/T) * Σ [ΔK / K² * e^(rT) * Q(K)] - (F/K₀ - 1)²
    /// ```
    ///
    /// where `Q(K)` is the OTM price (put if K < F, call if K ≥ F) and
    /// `K₀` is the first strike below the forward.
    pub fn vix_squared(option_prices: &[(f64, f64, f64)], f: f64, r: f64, t: f64) -> f64 {
        if option_prices.is_empty() || t <= 0.0 {
            return 0.0;
        }

        // Sort by strike.
        let mut sorted: Vec<(f64, f64, f64)> = option_prices.to_vec();
        sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        let n = sorted.len();
        let exp_rt = (r * t).exp();

        // Find K₀ (first strike at or below forward).
        let k0 = sorted
            .iter()
            .filter(|&&(k, _, _)| k <= f)
            .map(|&(k, _, _)| k)
            .last()
            .unwrap_or(sorted[0].0);

        let mut sum = 0.0;
        for i in 0..n {
            let (k, call, put) = sorted[i];

            // Δk: trapezoidal spacing.
            let dk = if n == 1 {
                k
            } else if i == 0 {
                sorted[1].0 - sorted[0].0
            } else if i == n - 1 {
                sorted[n - 1].0 - sorted[n - 2].0
            } else {
                (sorted[i + 1].0 - sorted[i - 1].0) / 2.0
            };

            // OTM option price.
            let q = if k < f { put } else { call };

            if k > 0.0 {
                sum += (dk / (k * k)) * exp_rt * q;
            }
        }

        let correction = if k0 > 0.0 {
            let ratio = f / k0 - 1.0;
            ratio * ratio
        } else {
            0.0
        };

        (2.0 / t) * sum - correction
    }

    /// Generate a BSM option strip at `n_strikes` evenly spaced around `spot`.
    ///
    /// Returns `(strike, call_price, put_price)` for each strike.
    pub fn strip_of_options(
        spot: f64,
        r: f64,
        sigma: f64,
        t: f64,
        n_strikes: usize,
    ) -> Vec<(f64, f64, f64)> {
        if n_strikes == 0 || spot <= 0.0 || sigma <= 0.0 || t <= 0.0 {
            return Vec::new();
        }

        let low = spot * 0.7;
        let high = spot * 1.3;
        let step = (high - low) / (n_strikes.saturating_sub(1).max(1)) as f64;

        (0..n_strikes)
            .map(|i| {
                let k = low + i as f64 * step;
                let (call, put) = bsm_call_put(spot, k, r, sigma, t);
                (k, call, put)
            })
            .collect()
    }

    /// Analytic VIX approximation: `VIX ≈ ATM 30-day vol × 100`.
    pub fn vix_analytic(atm_vol: f64) -> f64 {
        atm_vol * 100.0
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Internal helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Black-Scholes-Merton call and put prices.
fn bsm_call_put(s: f64, k: f64, r: f64, sigma: f64, t: f64) -> (f64, f64) {
    if s <= 0.0 || k <= 0.0 || sigma <= 0.0 || t <= 0.0 {
        return (0.0, 0.0);
    }
    let sqrt_t = t.sqrt();
    let d1 = ((s / k).ln() + (r + 0.5 * sigma * sigma) * t) / (sigma * sqrt_t);
    let d2 = d1 - sigma * sqrt_t;
    let nd1 = normal_cdf(d1);
    let nd2 = normal_cdf(d2);
    let disc = (-r * t).exp();
    let call = s * nd1 - k * disc * nd2;
    let put = call - s + k * disc; // put-call parity
    (call.max(0.0), put.max(0.0))
}

/// Cumulative standard normal distribution (Hart approximation).
fn normal_cdf(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.2316419 * x.abs());
    let poly = t
        * (0.319_381_530
            + t * (-0.356_563_782
                + t * (1.781_477_937 + t * (-1.821_255_978 + t * 1.330_274_429))));
    let pdf = (-0.5 * x * x).exp() / (2.0 * std::f64::consts::PI).sqrt();
    if x >= 0.0 {
        1.0 - pdf * poly
    } else {
        pdf * poly
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_variance_swap_pnl() {
        let vs = VarianceSwap {
            notional: 10_000.0,
            strike_variance: 0.04,
            maturity_years: 1.0,
            vega_notional: 4_000.0,
        };
        // realized == strike → zero P&L
        assert!((vs.pnl(0.04)).abs() < 1e-12);
        // realized > strike → positive P&L
        assert!(vs.pnl(0.05) > 0.0);
    }

    #[test]
    fn test_fair_strike() {
        let fs = VarianceSwap::fair_strike(0.20, 0.05);
        // 0.20² * 1.05 = 0.042
        assert!((fs - 0.042).abs() < 1e-12);
    }

    #[test]
    fn test_realized_variance_tracker() {
        let mut tracker = RealizedVarianceTracker::new(252);
        tracker.annualized = false;
        for &p in &[100.0_f64, 101.0, 102.0, 101.5, 103.0] {
            tracker.push_price(p);
        }
        assert_eq!(tracker.days_elapsed(), 4);
        assert!(tracker.realized_variance() >= 0.0);
        assert!(tracker.realized_vol() >= 0.0);
    }

    #[test]
    fn test_vix_analytic() {
        assert!((VixReplication::vix_analytic(0.20) - 20.0).abs() < 1e-10);
    }
}
