//! Forward contract pricing: equity, FX, commodity forwards, and forward curves.

/// Classification of forward contract underlying.
#[derive(Debug, Clone, PartialEq)]
pub enum ForwardType {
    /// Equity forward.
    Equity,
    /// Foreign-exchange forward.
    Fx,
    /// Commodity forward.
    Commodity,
    /// Interest-rate forward.
    Rate,
}

/// Equity forward contract.
#[derive(Debug, Clone)]
pub struct EquityForward {
    /// Current spot price.
    pub spot: f64,
    /// Strike (delivery) price.
    pub strike: f64,
    /// Risk-free rate (continuous, annual).
    pub r: f64,
    /// Continuous dividend yield (annual).
    pub q: f64,
    /// Time to expiry in years.
    pub t: f64,
}

impl EquityForward {
    /// Fair forward price: F = S * e^((r - q) * T).
    pub fn fair_forward(&self) -> f64 {
        self.spot * ((self.r - self.q) * self.t).exp()
    }

    /// P&L for long forward at expiry: spot_at_expiry - strike.
    pub fn pnl(&self, spot_at_expiry: f64) -> f64 {
        spot_at_expiry - self.strike
    }

    /// Delta of forward: e^(-q * T).
    pub fn delta(&self) -> f64 {
        (-self.q * self.t).exp()
    }

    /// Net carry cost: (e^(r*T) - e^(q*T)) * spot.
    pub fn carry_cost(&self) -> f64 {
        ((self.r * self.t).exp() - (self.q * self.t).exp()) * self.spot
    }
}

/// FX forward contract (covered interest parity).
#[derive(Debug, Clone)]
pub struct FxForward {
    /// Spot exchange rate (domestic per foreign).
    pub spot_rate: f64,
    /// Domestic risk-free rate (continuous, annual).
    pub domestic_rate: f64,
    /// Foreign risk-free rate (continuous, annual).
    pub foreign_rate: f64,
    /// Time to expiry in years.
    pub t: f64,
    /// Notional in domestic currency.
    pub notional_domestic: f64,
}

impl FxForward {
    /// CIP forward rate: S * e^((r_d - r_f) * T).
    pub fn forward_rate(&self) -> f64 {
        self.spot_rate * ((self.domestic_rate - self.foreign_rate) * self.t).exp()
    }

    /// NDF P&L at fixing: (forward_rate - fixing_rate) * notional_domestic.
    pub fn ndf_settlement(&self, fixing_rate: f64) -> f64 {
        (self.forward_rate() - fixing_rate) * self.notional_domestic
    }

    /// Forward points (forward rate minus spot rate), in pips (×10000).
    pub fn basis_points_fwd(&self) -> f64 {
        (self.forward_rate() - self.spot_rate) * 10_000.0
    }
}

/// Commodity forward contract.
#[derive(Debug, Clone)]
pub struct CommodityForward {
    /// Current spot price.
    pub spot: f64,
    /// Continuous storage cost rate (annual).
    pub storage_cost: f64,
    /// Continuous convenience yield (annual).
    pub convenience_yield: f64,
    /// Risk-free rate (continuous, annual).
    pub r: f64,
    /// Time to expiry in years.
    pub t: f64,
}

impl CommodityForward {
    /// Fair forward: S * e^((r + u - y) * T).
    pub fn fair_forward(&self) -> f64 {
        self.spot * (self.net_carry() * self.t).exp()
    }

    /// Net carry rate: r + storage_cost - convenience_yield.
    pub fn net_carry(&self) -> f64 {
        self.r + self.storage_cost - self.convenience_yield
    }
}

/// A forward curve defined by (maturity, forward_price) pillars.
#[derive(Debug, Clone)]
pub struct ForwardCurve {
    /// Sorted (maturity_years, forward_price) pairs.
    pub pillars: Vec<(f64, f64)>,
}

impl ForwardCurve {
    /// Construct a forward curve from spot rate and (maturity, rate) pairs.
    pub fn from_spot_rates(spot: f64, rates: &[(f64, f64)]) -> Self {
        let mut pillars: Vec<(f64, f64)> = rates
            .iter()
            .map(|&(t, r)| (t, spot * (r * t).exp()))
            .collect();
        pillars.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        Self { pillars }
    }

    /// Linear interpolation in log-time space.
    pub fn interpolate(&self, maturity: f64) -> f64 {
        if self.pillars.is_empty() {
            return 0.0;
        }
        if maturity <= self.pillars[0].0 {
            return self.pillars[0].1;
        }
        if maturity >= self.pillars[self.pillars.len() - 1].0 {
            return self.pillars[self.pillars.len() - 1].1;
        }
        // Find surrounding pillars
        for i in 0..self.pillars.len() - 1 {
            let (t0, f0) = self.pillars[i];
            let (t1, f1) = self.pillars[i + 1];
            if maturity >= t0 && maturity <= t1 {
                if (t1 - t0).abs() < 1e-12 {
                    return f0;
                }
                // Linear in log-time
                let log_t0 = t0.ln().max(-30.0);
                let log_t1 = t1.ln().max(-30.0);
                let log_t = maturity.ln().max(-30.0);
                let w = (log_t - log_t0) / (log_t1 - log_t0);
                return f0 + w * (f1 - f0);
            }
        }
        self.pillars[self.pillars.len() - 1].1
    }

    /// True if the curve is in contango (last pillar forward > first pillar forward).
    pub fn contango(&self) -> bool {
        if self.pillars.len() < 2 {
            return false;
        }
        self.pillars.last().map(|p| p.1).unwrap_or(0.0)
            > self.pillars.first().map(|p| p.1).unwrap_or(0.0)
    }

    /// True if the curve is in backwardation (last pillar forward < first pillar forward).
    pub fn backwardation(&self) -> bool {
        if self.pillars.len() < 2 {
            return false;
        }
        self.pillars.last().map(|p| p.1).unwrap_or(0.0)
            < self.pillars.first().map(|p| p.1).unwrap_or(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_equity_forward_fair_forward() {
        let fwd = EquityForward { spot: 100.0, strike: 102.0, r: 0.05, q: 0.02, t: 1.0 };
        let f = fwd.fair_forward();
        // F = 100 * e^(0.03) ≈ 103.045
        assert!((f - 103.045).abs() < 0.01, "fair_forward={f}");
    }

    #[test]
    fn test_equity_forward_pnl() {
        let fwd = EquityForward { spot: 100.0, strike: 100.0, r: 0.05, q: 0.0, t: 1.0 };
        assert!((fwd.pnl(110.0) - 10.0).abs() < 1e-9);
    }

    #[test]
    fn test_equity_forward_delta() {
        let fwd = EquityForward { spot: 100.0, strike: 100.0, r: 0.05, q: 0.02, t: 1.0 };
        let d = fwd.delta();
        assert!((d - (-0.02_f64).exp()).abs() < 1e-9);
    }

    #[test]
    fn test_fx_forward_rate() {
        let fx = FxForward { spot_rate: 1.25, domestic_rate: 0.03, foreign_rate: 0.01, t: 1.0, notional_domestic: 1_000_000.0 };
        let f = fx.forward_rate();
        // S * e^(0.02) ≈ 1.27528
        assert!((f - 1.25 * (0.02_f64).exp()).abs() < 1e-6);
    }

    #[test]
    fn test_commodity_forward() {
        let cf = CommodityForward { spot: 50.0, storage_cost: 0.02, convenience_yield: 0.01, r: 0.05, t: 1.0 };
        let f = cf.fair_forward();
        assert!((f - 50.0 * (0.06_f64).exp()).abs() < 0.01);
    }

    #[test]
    fn test_forward_curve_contango() {
        let curve = ForwardCurve { pillars: vec![(0.5, 100.0), (1.0, 102.0), (2.0, 105.0)] };
        assert!(curve.contango());
        assert!(!curve.backwardation());
    }

    #[test]
    fn test_forward_curve_interpolate() {
        let curve = ForwardCurve { pillars: vec![(1.0, 100.0), (2.0, 110.0)] };
        let mid = curve.interpolate(1.5);
        // Should be between 100 and 110
        assert!(mid > 100.0 && mid < 110.0);
    }

    #[test]
    fn test_from_spot_rates() {
        let curve = ForwardCurve::from_spot_rates(100.0, &[(1.0, 0.05), (2.0, 0.05)]);
        assert_eq!(curve.pillars.len(), 2);
        assert!((curve.pillars[0].1 - 100.0 * (0.05_f64).exp()).abs() < 0.01);
    }
}
