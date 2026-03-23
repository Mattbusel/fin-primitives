//! Credit Default Swap pricing

/// CDS contract specification.
///
/// Protection leg PV = sum_i N*(1-R)*[Q(t_{i-1})-Q(t_i)]*D(t_i)
/// where Q(t) = exp(-lambda*t) is the survival probability and D(t) = exp(-r*t).
pub struct CdsContract {
    /// Notional principal
    pub notional: f64,
    /// Running coupon in basis points
    pub spread_bps: f64,
    /// Contract maturity in years
    pub maturity_years: f64,
    /// Recovery rate (fraction, e.g. 0.40)
    pub recovery_rate: f64,
    /// Coupon payment frequency per year (typically 4 for quarterly)
    pub payment_freq: u32,
}

impl CdsContract {
    /// Construct a CDS contract (quarterly payments by default).
    pub fn new(notional: f64, spread_bps: f64, maturity: f64, recovery: f64) -> Self {
        CdsContract { notional, spread_bps, maturity_years: maturity, recovery_rate: recovery, payment_freq: 4 }
    }

    fn discount(t: f64, risk_free_rate: f64) -> f64 {
        (-risk_free_rate * t).exp()
    }

    fn survival_prob(t: f64, hazard_rate: f64) -> f64 {
        (-hazard_rate * t).exp()
    }

    /// Present value of the protection leg (expected loss payments).
    pub fn protection_leg_pv(&self, hazard_rate: f64, risk_free_rate: f64) -> f64 {
        let n_steps = (self.maturity_years * 365.0) as u32;
        let dt = self.maturity_years / n_steps as f64;
        let mut pv = 0.0;
        let mut q_prev = 1.0f64;
        for i in 1..=n_steps {
            let t = i as f64 * dt;
            let q = Self::survival_prob(t, hazard_rate);
            let d = Self::discount(t, risk_free_rate);
            pv += (1.0 - self.recovery_rate) * (q_prev - q) * d;
            q_prev = q;
        }
        self.notional * pv
    }

    /// Present value of the premium (fee) leg.
    pub fn premium_leg_pv(&self, hazard_rate: f64, risk_free_rate: f64) -> f64 {
        let spread = self.spread_bps / 10000.0;
        let dt = 1.0 / self.payment_freq as f64;
        let n_periods = (self.maturity_years * self.payment_freq as f64) as u32;
        let mut pv = 0.0;
        for i in 1..=n_periods {
            let t = i as f64 * dt;
            let q = Self::survival_prob(t, hazard_rate);
            let d = Self::discount(t, risk_free_rate);
            pv += spread * dt * q * d;
        }
        self.notional * pv
    }

    /// Par spread in basis points: the spread that sets protection_pv = premium_pv.
    pub fn par_spread(&self, hazard_rate: f64, risk_free_rate: f64) -> f64 {
        let protection = self.protection_leg_pv(hazard_rate, risk_free_rate);
        let dt = 1.0 / self.payment_freq as f64;
        let n_periods = (self.maturity_years * self.payment_freq as f64) as u32;
        let mut risky_annuity = 0.0f64;
        for i in 1..=n_periods {
            let t = i as f64 * dt;
            risky_annuity += dt * Self::survival_prob(t, hazard_rate) * Self::discount(t, risk_free_rate);
        }
        if risky_annuity < 1e-10 { return 0.0; }
        (protection / (self.notional * risky_annuity)) * 10000.0
    }

    /// Mark-to-market value to the protection buyer.
    pub fn mtm(&self, hazard_rate: f64, risk_free_rate: f64) -> f64 {
        self.protection_leg_pv(hazard_rate, risk_free_rate)
            - self.premium_leg_pv(hazard_rate, risk_free_rate)
    }

    /// Implied hazard rate from a market spread (bisection over 50 iterations).
    pub fn implied_hazard_rate(&self, market_spread_bps: f64, risk_free_rate: f64) -> f64 {
        let target_spread = market_spread_bps;
        let mut lo = 0.0001f64;
        let mut hi = 0.5f64;
        for _ in 0..50 {
            let mid = (lo + hi) / 2.0;
            let mut cds = CdsContract::new(self.notional, 0.0, self.maturity_years, self.recovery_rate);
            cds.payment_freq = self.payment_freq;
            let spread = cds.par_spread(mid, risk_free_rate);
            if spread < target_spread { lo = mid; } else { hi = mid; }
        }
        (lo + hi) / 2.0
    }

    /// CS01: sensitivity of MTM to a 1bp increase in the running spread.
    pub fn cs01(&self, hazard_rate: f64, risk_free_rate: f64) -> f64 {
        let v_up = {
            let mut cds = CdsContract::new(self.notional, self.spread_bps, self.maturity_years, self.recovery_rate);
            cds.payment_freq = self.payment_freq;
            let h_up = cds.implied_hazard_rate(self.spread_bps * 10000.0 / 10000.0 + 1.0, risk_free_rate);
            cds.mtm(h_up, risk_free_rate)
        };
        let v_down = {
            let mut cds = CdsContract::new(self.notional, self.spread_bps, self.maturity_years, self.recovery_rate);
            cds.payment_freq = self.payment_freq;
            let h_down = cds.implied_hazard_rate((self.spread_bps * 10000.0 / 10000.0 - 1.0).max(0.1), risk_free_rate);
            cds.mtm(h_down, risk_free_rate)
        };
        (v_up - v_down) / 2.0
    }
}
