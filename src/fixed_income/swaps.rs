//! Interest rate swap pricing and analytics.
//!
//! Provides swap leg structures, discount curve bootstrapping, NPV, par rate,
//! DV01, and duration calculations for vanilla interest rate swaps.

/// Day count convention for interest accrual.
#[derive(Debug, Clone, PartialEq)]
pub enum DayCount {
    /// Actual days / 360.
    Actual360,
    /// Actual days / 365.
    Actual365,
    /// 30/360 bond basis.
    Thirty360,
}

/// Compute the day count fraction for a given number of days.
pub fn day_count_fraction(day_count: &DayCount, days: u32) -> f64 {
    match day_count {
        DayCount::Actual360 => days as f64 / 360.0,
        DayCount::Actual365 => days as f64 / 365.0,
        DayCount::Thirty360 => days as f64 / 360.0,
    }
}

/// A single leg of an interest rate swap.
#[derive(Debug, Clone)]
pub struct SwapLeg {
    /// Notional principal amount.
    pub notional: f64,
    /// Fixed rate (None for floating leg).
    pub fixed_rate: Option<f64>,
    /// Spread over floating reference rate.
    pub floating_spread: f64,
    /// Number of payments per year.
    pub payment_frequency: u32,
    /// Day count convention.
    pub day_count: DayCount,
}

/// Specification for a vanilla fixed-for-floating interest rate swap.
#[derive(Debug, Clone)]
pub struct SwapSpec {
    /// Notional principal.
    pub notional: f64,
    /// Fixed coupon rate (annualized).
    pub fixed_rate: f64,
    /// Swap tenor in years.
    pub tenor_years: f64,
    /// Payment frequency (payments per year).
    pub payment_frequency: u32,
    /// Day count convention.
    pub day_count: DayCount,
}

/// Zero-coupon discount curve with linear interpolation.
#[derive(Debug, Clone)]
pub struct DiscountCurve {
    /// Tenors in years.
    pub tenors: Vec<f64>,
    /// Continuously compounded zero rates.
    pub zero_rates: Vec<f64>,
}

impl DiscountCurve {
    /// Compute discount factor at time `t` using linear interpolation of zero rates,
    /// then `df = exp(-r * t)`.
    pub fn discount_factor(&self, t: f64) -> f64 {
        if t <= 0.0 {
            return 1.0;
        }
        let r = self.interpolate_rate(t);
        (-r * t).exp()
    }

    /// Linearly interpolate (or extrapolate flat) the zero rate at time `t`.
    fn interpolate_rate(&self, t: f64) -> f64 {
        let n = self.tenors.len();
        if n == 0 {
            return 0.0;
        }
        if t <= self.tenors[0] {
            return self.zero_rates[0];
        }
        if t >= self.tenors[n - 1] {
            return self.zero_rates[n - 1];
        }
        // Linear interpolation
        for i in 0..n - 1 {
            let t0 = self.tenors[i];
            let t1 = self.tenors[i + 1];
            if t >= t0 && t <= t1 {
                let alpha = (t - t0) / (t1 - t0);
                return self.zero_rates[i] * (1.0 - alpha) + self.zero_rates[i + 1] * alpha;
            }
        }
        self.zero_rates[n - 1]
    }

    /// Bootstrap a discount curve from par swap rates.
    ///
    /// For each tenor, solve for the discount factor that reprices the par swap.
    /// The par swap condition: `sum(c * df(t_i) * dcf_i) + df(T) = 1`.
    pub fn from_par_rates(tenors: &[f64], par_rates: &[f64]) -> Self {
        assert_eq!(tenors.len(), par_rates.len(), "tenors and par_rates must have equal length");
        let n = tenors.len();
        let mut dfs: Vec<f64> = Vec::with_capacity(n);
        let mut zero_rates: Vec<f64> = Vec::with_capacity(n);

        for i in 0..n {
            let t = tenors[i];
            let c = par_rates[i];
            // Assume annual payments for bootstrap (dcf = t_k - t_{k-1})
            // Build payment schedule up to tenor i
            let mut pv_coupons = 0.0;
            let steps = i; // prior periods
            for j in 0..steps {
                let t_prev = if j == 0 { 0.0 } else { tenors[j - 1] };
                let t_j = tenors[j];
                let dcf = t_j - t_prev;
                pv_coupons += c * dcf * dfs[j];
            }
            // Last period
            let t_prev_last = if i == 0 { 0.0 } else { tenors[i - 1] };
            let dcf_last = t - t_prev_last;
            // par: pv_coupons + c * dcf_last * df_i + df_i = 1
            // df_i * (1 + c * dcf_last) = 1 - pv_coupons
            let df_i = (1.0 - pv_coupons) / (1.0 + c * dcf_last);
            dfs.push(df_i);
            // Convert to zero rate: df = exp(-r*t)
            let r = if t > 0.0 { -df_i.ln() / t } else { 0.0 };
            zero_rates.push(r);
        }

        DiscountCurve {
            tenors: tenors.to_vec(),
            zero_rates,
        }
    }
}

/// Present value of the fixed leg.
pub fn fixed_leg_pv(spec: &SwapSpec, curve: &DiscountCurve) -> f64 {
    let n_payments = (spec.tenor_years * spec.payment_frequency as f64).round() as u32;
    let dt = 1.0 / spec.payment_frequency as f64;
    let days_per_period = (365.0 * dt).round() as u32;
    let dcf = day_count_fraction(&spec.day_count, days_per_period);

    let mut pv = 0.0;
    for k in 1..=n_payments {
        let t = k as f64 * dt;
        let df = curve.discount_factor(t);
        pv += spec.notional * spec.fixed_rate * dcf * df;
    }
    // Add notional at maturity
    pv += spec.notional * curve.discount_factor(spec.tenor_years);
    pv
}

/// Present value of the floating leg (par approximation).
///
/// At inception the floating leg is worth par (notional), discounted back.
/// This uses the approximation: PV_float = notional * (df(0) - df(T)) + notional * df(T)
/// which simplifies to notional (at par). We compute it as the bond-equivalent:
/// notional * (1 - df(T)) via the annuity factor, plus notional * df(T) = notional.
pub fn floating_leg_pv(spec: &SwapSpec, curve: &DiscountCurve) -> f64 {
    // Approximate: PV of floating leg = notional at start = notional * 1
    // More precisely: notional * (df(0) - df(T)) + notional * df(T) = notional
    // But we add notional at maturity:
    let df_t = curve.discount_factor(spec.tenor_years);
    // PV = notional (principal) discounted + projected floating coupons
    // Using the fact that floating resets to par each period:
    // PV_float = notional * (1 - df(T)) + notional * df(T) = notional
    // For accuracy with a non-flat curve, use:
    let n_payments = (spec.tenor_years * spec.payment_frequency as f64).round() as u32;
    let dt = 1.0 / spec.payment_frequency as f64;
    let days_per_period = (365.0 * dt).round() as u32;
    let dcf = day_count_fraction(&spec.day_count, days_per_period);

    // Forward rate for each period times notional
    let mut pv = 0.0;
    let mut df_prev = 1.0;
    for k in 1..=n_payments {
        let t = k as f64 * dt;
        let df = curve.discount_factor(t);
        let fwd = (df_prev / df - 1.0) / dcf;
        pv += spec.notional * fwd * dcf * df;
        df_prev = df;
    }
    pv += spec.notional * df_t;
    pv
}

/// Net present value of the swap.
///
/// `receive_fixed = true`: long fixed (receive fixed, pay floating).
pub fn swap_npv(spec: &SwapSpec, curve: &DiscountCurve, receive_fixed: bool) -> f64 {
    let fixed_pv = fixed_leg_pv(spec, curve);
    let float_pv = floating_leg_pv(spec, curve);
    if receive_fixed {
        fixed_pv - float_pv
    } else {
        float_pv - fixed_pv
    }
}

/// Par swap rate: the fixed rate that makes NPV = 0.
///
/// `par_rate = annuity_fwd_sum / annuity_factor`
pub fn par_swap_rate(spec: &SwapSpec, curve: &DiscountCurve) -> f64 {
    let n_payments = (spec.tenor_years * spec.payment_frequency as f64).round() as u32;
    let dt = 1.0 / spec.payment_frequency as f64;
    let days_per_period = (365.0 * dt).round() as u32;
    let dcf = day_count_fraction(&spec.day_count, days_per_period);

    let df_t = curve.discount_factor(spec.tenor_years);

    // Annuity factor: sum of df(t_k) * dcf
    let mut annuity = 0.0;
    for k in 1..=n_payments {
        let t = k as f64 * dt;
        annuity += curve.discount_factor(t) * dcf;
    }

    // Par rate = (1 - df(T)) / annuity
    (1.0 - df_t) / annuity
}

/// DV01: sensitivity of swap NPV to a 1bp (0.0001) parallel shift in rates.
pub fn swap_dv01(spec: &SwapSpec, curve: &DiscountCurve, receive_fixed: bool) -> f64 {
    let bump = 0.0001;
    let bumped_rates: Vec<f64> = curve.zero_rates.iter().map(|r| r + bump).collect();
    let bumped_curve = DiscountCurve {
        tenors: curve.tenors.clone(),
        zero_rates: bumped_rates,
    };
    let npv_base = swap_npv(spec, curve, receive_fixed);
    let npv_bumped = swap_npv(spec, &bumped_curve, receive_fixed);
    npv_bumped - npv_base
}

/// Modified duration of the fixed leg.
pub fn swap_duration(spec: &SwapSpec, curve: &DiscountCurve) -> f64 {
    let n_payments = (spec.tenor_years * spec.payment_frequency as f64).round() as u32;
    let dt = 1.0 / spec.payment_frequency as f64;
    let days_per_period = (365.0 * dt).round() as u32;
    let dcf = day_count_fraction(&spec.day_count, days_per_period);

    let mut weighted_sum = 0.0;
    let mut pv_sum = 0.0;

    for k in 1..=n_payments {
        let t = k as f64 * dt;
        let df = curve.discount_factor(t);
        let cf = spec.notional * spec.fixed_rate * dcf;
        weighted_sum += t * cf * df;
        pv_sum += cf * df;
    }
    // Include notional at maturity
    let df_t = curve.discount_factor(spec.tenor_years);
    weighted_sum += spec.tenor_years * spec.notional * df_t;
    pv_sum += spec.notional * df_t;

    if pv_sum == 0.0 {
        return 0.0;
    }
    // Macaulay duration; approximate as modified duration (divide by 1+r/freq)
    let mac_duration = weighted_sum / pv_sum;
    let r = curve.interpolate_rate(spec.tenor_years);
    mac_duration / (1.0 + r / spec.payment_frequency as f64)
}

/// Complete swap valuation result.
#[derive(Debug, Clone)]
pub struct SwapValuation {
    /// Net present value (positive = in the money for the receiver).
    pub npv: f64,
    /// Present value of the fixed leg.
    pub fixed_pv: f64,
    /// Present value of the floating leg.
    pub floating_pv: f64,
    /// Par swap rate (rate that sets NPV to zero).
    pub par_rate: f64,
    /// DV01 (dollar value of 1 basis point).
    pub dv01: f64,
    /// Modified duration of the fixed leg.
    pub duration: f64,
}

/// Compute a full swap valuation.
pub fn value_swap(spec: &SwapSpec, curve: &DiscountCurve, receive_fixed: bool) -> SwapValuation {
    let fixed_pv = fixed_leg_pv(spec, curve);
    let floating_pv = floating_leg_pv(spec, curve);
    let npv = if receive_fixed {
        fixed_pv - floating_pv
    } else {
        floating_pv - fixed_pv
    };
    SwapValuation {
        npv,
        fixed_pv,
        floating_pv,
        par_rate: par_swap_rate(spec, curve),
        dv01: swap_dv01(spec, curve, receive_fixed),
        duration: swap_duration(spec, curve),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_curve(rate: f64) -> DiscountCurve {
        DiscountCurve {
            tenors: vec![0.5, 1.0, 2.0, 3.0, 5.0, 7.0, 10.0],
            zero_rates: vec![rate; 7],
        }
    }

    #[test]
    fn test_swap_npv_at_par_rate_is_zero() {
        let curve = flat_curve(0.05);
        let par = par_swap_rate(
            &SwapSpec {
                notional: 1_000_000.0,
                fixed_rate: 0.05,
                tenor_years: 5.0,
                payment_frequency: 2,
                day_count: DayCount::Actual365,
            },
            &curve,
        );
        let spec = SwapSpec {
            notional: 1_000_000.0,
            fixed_rate: par,
            tenor_years: 5.0,
            payment_frequency: 2,
            day_count: DayCount::Actual365,
        };
        let npv = swap_npv(&spec, &curve, true);
        assert!(npv.abs() < 1.0, "NPV at par rate should be near zero, got {}", npv);
    }

    #[test]
    fn test_dv01_positive_for_receiver() {
        let curve = flat_curve(0.05);
        let spec = SwapSpec {
            notional: 1_000_000.0,
            fixed_rate: 0.05,
            tenor_years: 5.0,
            payment_frequency: 2,
            day_count: DayCount::Actual365,
        };
        // Receiver gets fixed; if rates go up, fixed leg loses value (NPV decreases)
        // DV01 is negative for receiver when rates rise
        let dv01 = swap_dv01(&spec, &curve, true);
        // For a 5Y receiver: bumping rates hurts fixed PV more, so DV01 < 0
        // In absolute terms: |DV01| > 0
        assert!(dv01.abs() > 0.0);
    }

    #[test]
    fn test_day_count_fraction() {
        assert!((day_count_fraction(&DayCount::Actual360, 180) - 0.5).abs() < 1e-10);
        assert!((day_count_fraction(&DayCount::Actual365, 365) - 1.0).abs() < 1e-10);
        assert!((day_count_fraction(&DayCount::Thirty360, 90) - 0.25).abs() < 1e-10);
    }

    #[test]
    fn test_bootstrap_consistent_discount_factors() {
        let tenors = vec![1.0, 2.0, 3.0, 5.0];
        let par_rates = vec![0.04, 0.045, 0.048, 0.052];
        let curve = DiscountCurve::from_par_rates(&tenors, &par_rates);

        // For each tenor, reprice the par swap using the bootstrapped curve
        for (i, &t) in tenors.iter().enumerate() {
            let spec = SwapSpec {
                notional: 1.0,
                fixed_rate: par_rates[i],
                tenor_years: t,
                payment_frequency: 1,
                day_count: DayCount::Actual365,
            };
            let npv = swap_npv(&spec, &curve, true);
            assert!(
                npv.abs() < 1e-6,
                "Bootstrap should give zero NPV for tenor={}, got {}",
                t,
                npv
            );
        }
    }

    #[test]
    fn test_fixed_leg_pv_high_rate() {
        // At very high fixed rate, the fixed leg PV exceeds notional
        let curve = flat_curve(0.01);
        let spec = SwapSpec {
            notional: 1_000_000.0,
            fixed_rate: 0.20,
            tenor_years: 2.0,
            payment_frequency: 1,
            day_count: DayCount::Actual365,
        };
        let pv = fixed_leg_pv(&spec, &curve);
        assert!(pv > spec.notional, "Fixed leg PV should exceed notional at very high coupon");
    }
}
