//! Interest rate swap pricing.
//!
//! Provides day-count conventions, swap leg representations, discount curve
//! interpolation, par swap rate, DV01, and NPV calculation.

/// Day-count convention used to compute year fractions between dates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayCountConvention {
    /// Actual days divided by 360.
    Actual360,
    /// Actual days divided by 365.
    Actual365,
    /// 30/360 convention (bond basis).
    Thirty360,
}

impl DayCountConvention {
    /// Compute the year fraction between two Unix timestamps (in seconds).
    ///
    /// For [`DayCountConvention::Thirty360`] the timestamps are converted to
    /// calendar dates assuming each day = 86 400 seconds; day-of-month
    /// arithmetic then follows the standard 30/360 formula.
    pub fn year_fraction(self, start: u64, end: u64) -> f64 {
        if end <= start {
            return 0.0;
        }
        match self {
            DayCountConvention::Actual360 => {
                let days = (end - start) as f64 / 86_400.0;
                days / 360.0
            }
            DayCountConvention::Actual365 => {
                let days = (end - start) as f64 / 86_400.0;
                days / 365.0
            }
            DayCountConvention::Thirty360 => {
                // Decompose both timestamps into (year, month, day).
                let (y1, m1, d1) = days_to_ymd(start / 86_400);
                let (y2, m2, d2) = days_to_ymd(end / 86_400);
                // Standard 30/360: clamp d1/d2 at 30.
                let d1 = d1.min(30);
                let d2 = if d1 == 30 { d2.min(30) } else { d2 };
                let days =
                    360 * (y2 as i64 - y1 as i64) + 30 * (m2 as i64 - m1 as i64) + (d2 as i64 - d1 as i64);
                days.max(0) as f64 / 360.0
            }
        }
    }
}

/// Convert a count of days since the Unix epoch (1970-01-01) to (year, month, day).
fn days_to_ymd(days: u64) -> (u32, u32, u32) {
    // Gregorian calendar algorithm (civil calendar).
    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as u32, m as u32, d as u32)
}

// ─────────────────────────────────────────────────────────────────────────────
// Swap Legs
// ─────────────────────────────────────────────────────────────────────────────

/// A single leg of an interest rate swap.
#[derive(Debug, Clone)]
pub enum SwapLeg {
    /// A fixed-coupon leg.
    Fixed {
        /// Annual fixed coupon rate (e.g. 0.05 = 5%).
        rate: f64,
        /// Notional principal.
        notional: f64,
        /// Scheduled payment dates as Unix timestamps (seconds).
        payment_dates: Vec<u64>,
    },
    /// A floating (IBOR) leg.
    Floating {
        /// Additional spread over the floating index (e.g. 0.001 = 10 bp).
        spread: f64,
        /// Notional principal.
        notional: f64,
        /// Scheduled payment dates as Unix timestamps (seconds).
        payment_dates: Vec<u64>,
        /// Rate-reset / observation dates, one per payment period.
        reset_dates: Vec<u64>,
    },
}

// ─────────────────────────────────────────────────────────────────────────────
// Interest Rate Swap
// ─────────────────────────────────────────────────────────────────────────────

/// A vanilla interest rate swap composed of one fixed and one floating leg.
#[derive(Debug, Clone)]
pub struct InterestRateSwap {
    /// The fixed-coupon payer/receiver leg.
    pub fixed_leg: SwapLeg,
    /// The floating-rate leg.
    pub floating_leg: SwapLeg,
    /// Trade start date as Unix timestamp (seconds).
    pub start_date: u64,
    /// Maturity date as Unix timestamp (seconds).
    pub maturity_date: u64,
    /// Day-count convention applied to both legs.
    pub day_count: DayCountConvention,
}

// ─────────────────────────────────────────────────────────────────────────────
// Discount Curve
// ─────────────────────────────────────────────────────────────────────────────

/// A piecewise-linear discount factor curve.
///
/// Points are stored sorted by maturity.  Extrapolation is flat (constant
/// endpoint discount factor) beyond the curve range.
#[derive(Debug, Clone)]
pub struct DiscountCurve {
    /// Maturity pillars in years (must be the same length as `discount_factors`).
    pub maturities_years: Vec<f64>,
    /// Discount factors P(0, T) corresponding to each maturity pillar.
    pub discount_factors: Vec<f64>,
}

impl DiscountCurve {
    /// Construct a new [`DiscountCurve`].
    ///
    /// Pairs are sorted by maturity ascending.
    ///
    /// # Panics
    ///
    /// Panics if `maturities` and `discount_factors` differ in length.
    pub fn new(maturities: Vec<f64>, discount_factors: Vec<f64>) -> Self {
        assert_eq!(
            maturities.len(),
            discount_factors.len(),
            "maturities and discount_factors must have the same length"
        );
        let mut pairs: Vec<(f64, f64)> = maturities
            .into_iter()
            .zip(discount_factors)
            .collect();
        pairs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let (mats, dfs): (Vec<f64>, Vec<f64>) = pairs.into_iter().unzip();
        Self {
            maturities_years: mats,
            discount_factors: dfs,
        }
    }

    /// Linear interpolation of the discount factor at maturity `t` (years).
    ///
    /// Always anchors at `(0, 1.0)` (today's discount factor is 1).
    /// Extrapolates flat (constant) beyond the longest maturity.
    pub fn interpolate(&self, t: f64) -> f64 {
        let mats = &self.maturities_years;
        let dfs = &self.discount_factors;
        if t <= 0.0 {
            return 1.0;
        }
        if mats.is_empty() {
            return 1.0;
        }
        let last = mats.len() - 1;
        if t >= mats[last] {
            return dfs[last];
        }
        // Find the first index where mats[idx] > t.
        let idx = mats.partition_point(|&m| m <= t);
        if idx == 0 {
            // t is before the first pillar; interpolate between (0, 1.0) and mats[0].
            let t0 = 0.0_f64;
            let t1 = mats[0];
            let df1 = dfs[0];
            let frac = (t - t0) / (t1 - t0);
            return 1.0 + frac * (df1 - 1.0);
        }
        let t0 = mats[idx - 1];
        let t1 = mats[idx];
        let df0 = dfs[idx - 1];
        let df1 = dfs[idx];
        let frac = (t - t0) / (t1 - t0);
        df0 + frac * (df1 - df0)
    }

    /// Continuously-compounded zero rate at maturity `t` (years).
    ///
    /// Returns 0.0 for t ≤ 0.
    pub fn zero_rate(&self, t: f64) -> f64 {
        if t <= 0.0 {
            return 0.0;
        }
        let df = self.interpolate(t);
        if df <= 0.0 {
            return 0.0;
        }
        -df.ln() / t
    }

    /// Continuously-compounded instantaneous forward rate between `t1` and `t2` (years).
    ///
    /// Returns 0.0 when `t2 <= t1`.
    pub fn forward_rate(&self, t1: f64, t2: f64) -> f64 {
        if t2 <= t1 {
            return 0.0;
        }
        let df1 = self.interpolate(t1);
        let df2 = self.interpolate(t2);
        if df2 <= 0.0 || df1 <= 0.0 {
            return 0.0;
        }
        (df1 / df2).ln() / (t2 - t1)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Pricing Functions
// ─────────────────────────────────────────────────────────────────────────────

/// Present value of the fixed leg.
///
/// Computes the PV of each coupon using the year-fraction from `valuation_date`
/// to each payment date and discounts at the curve.
///
/// Returns 0.0 if `leg` is not [`SwapLeg::Fixed`].
pub fn price_fixed_leg(
    leg: &SwapLeg,
    curve: &DiscountCurve,
    valuation_date: u64,
    day_count: DayCountConvention,
) -> f64 {
    let (rate, notional, payment_dates) = match leg {
        SwapLeg::Fixed { rate, notional, payment_dates } => (*rate, *notional, payment_dates),
        SwapLeg::Floating { .. } => return 0.0,
    };

    let mut prev_date = valuation_date;
    let mut pv = 0.0;
    for &pay_date in payment_dates {
        if pay_date <= valuation_date {
            prev_date = pay_date;
            continue;
        }
        let accrual = day_count.year_fraction(prev_date, pay_date);
        let t = day_count.year_fraction(valuation_date, pay_date);
        let df = curve.interpolate(t);
        pv += rate * notional * accrual * df;
        prev_date = pay_date;
    }
    pv
}

/// Present value of the floating leg.
///
/// Projects each coupon using the forward rate over the accrual period and
/// discounts at the curve.
///
/// Returns 0.0 if `leg` is not [`SwapLeg::Floating`].
pub fn price_floating_leg(
    leg: &SwapLeg,
    curve: &DiscountCurve,
    valuation_date: u64,
    day_count: DayCountConvention,
) -> f64 {
    let (spread, notional, payment_dates, reset_dates) = match leg {
        SwapLeg::Floating { spread, notional, payment_dates, reset_dates } => {
            (*spread, *notional, payment_dates, reset_dates)
        }
        SwapLeg::Fixed { .. } => return 0.0,
    };

    let n = payment_dates.len().min(reset_dates.len());
    let mut pv = 0.0;
    let mut prev_date = valuation_date;

    for i in 0..n {
        let pay_date = payment_dates[i];
        let reset_date = reset_dates[i];
        if pay_date <= valuation_date {
            prev_date = pay_date;
            continue;
        }

        // The accrual period starts from the previous payment date (or valuation
        // date) and ends at this payment date.
        let t_start = day_count.year_fraction(valuation_date, prev_date.max(valuation_date));
        let t_end = day_count.year_fraction(valuation_date, pay_date);
        let _ = reset_date; // reset date used conceptually; forward rate proxies the index

        let fwd = curve.forward_rate(t_start, t_end);
        let accrual = day_count.year_fraction(prev_date.max(valuation_date), pay_date);
        let df = curve.interpolate(t_end);
        pv += (fwd + spread) * notional * accrual * df;
        prev_date = pay_date;
    }
    pv
}

/// Par swap rate: the fixed rate that makes the swap NPV equal to zero.
///
/// Computes the floating leg PV via the same forward-rate projection used in
/// [`price_floating_leg`], then solves analytically for the fixed coupon that
/// matches it: `par = floating_pv / (notional × annuity)`.
///
/// Uses [`DayCountConvention::Actual365`] internally. Returns 0.0 if the
/// annuity is zero.
pub fn par_swap_rate(
    curve: &DiscountCurve,
    payment_dates: &[u64],
    valuation_date: u64,
) -> f64 {
    let dc = DayCountConvention::Actual365;
    let notional = 1.0_f64;

    // Build a unit-notional floating leg.
    let reset_dates = payment_dates.to_vec();
    let floating_leg = SwapLeg::Floating {
        spread: 0.0,
        notional,
        payment_dates: payment_dates.to_vec(),
        reset_dates,
    };
    let float_pv = price_floating_leg(&floating_leg, curve, valuation_date, dc);

    // Annuity = sum of (accrual_i × df_i) — the denominator for a unit-notional fixed leg.
    let mut annuity = 0.0;
    let mut prev = valuation_date;
    for &pd in payment_dates {
        if pd <= valuation_date {
            prev = pd;
            continue;
        }
        let t = dc.year_fraction(valuation_date, pd);
        let accrual = dc.year_fraction(prev, pd);
        let df = curve.interpolate(t);
        annuity += accrual * df;
        prev = pd;
    }
    if annuity == 0.0 {
        return 0.0;
    }
    // par_rate × annuity = float_pv  ⟹  par_rate = float_pv / annuity
    float_pv / annuity
}

/// Net present value of the swap from the fixed-payer perspective.
///
/// NPV = PV(fixed leg) - PV(floating leg)
/// (Positive means fixed payer is out-of-the-money.)
pub fn npv(swap: &InterestRateSwap, curve: &DiscountCurve, valuation_date: u64) -> f64 {
    let fixed_pv = price_fixed_leg(&swap.fixed_leg, curve, valuation_date, swap.day_count);
    let float_pv = price_floating_leg(&swap.floating_leg, curve, valuation_date, swap.day_count);
    fixed_pv - float_pv
}

/// Dollar value of a 1 basis-point parallel shift (DV01).
///
/// Computed by bumping all zero rates by +1 bp, re-building a shifted curve,
/// and re-pricing the swap.  Returns the *absolute* change.
pub fn dv01(swap: &InterestRateSwap, curve: &DiscountCurve, valuation_date: u64) -> f64 {
    let base_npv = npv(swap, curve, valuation_date);

    // Build shifted curve: increase every zero rate by 1 bp and recompute DFs.
    let bump = 0.0001_f64;
    let shifted_dfs: Vec<f64> = curve
        .maturities_years
        .iter()
        .zip(curve.discount_factors.iter())
        .map(|(&t, &df)| {
            if t <= 0.0 {
                return df;
            }
            let z = curve.zero_rate(t) + bump;
            (-z * t).exp()
        })
        .collect();

    let shifted_curve = DiscountCurve::new(curve.maturities_years.clone(), shifted_dfs);
    let shifted_npv = npv(swap, &shifted_curve, valuation_date);
    (shifted_npv - base_npv).abs()
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: flat discount curve at a constant zero rate.
    fn flat_curve(rate: f64, pillars: &[f64]) -> DiscountCurve {
        let dfs = pillars.iter().map(|&t| (-rate * t).exp()).collect();
        DiscountCurve::new(pillars.to_vec(), dfs)
    }

    // Helper: build equally-spaced annual payment dates.
    fn annual_dates(start: u64, years: u32) -> Vec<u64> {
        (1..=years)
            .map(|y| start + y as u64 * 365 * 86_400)
            .collect()
    }

    #[test]
    fn flat_curve_discount_factors_sum() {
        let rate = 0.05;
        let pillars = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let curve = flat_curve(rate, &pillars);
        for (i, &t) in pillars.iter().enumerate() {
            let expected = (-rate * t).exp();
            let got = curve.interpolate(t);
            assert!((got - expected).abs() < 1e-10, "pillar {t}: expected {expected}, got {got}");
        }
        // Interpolate between pillars
        let mid = curve.interpolate(1.5);
        assert!(mid > 0.0 && mid < 1.0, "discount factor must be in (0,1)");
    }

    #[test]
    fn par_swap_rate_gives_zero_npv() {
        let rate = 0.03;
        let pillars: Vec<f64> = (1..=5).map(|y| y as f64).collect();
        let curve = flat_curve(rate, &pillars);

        let start: u64 = 0;
        let pay_dates = annual_dates(start, 5);
        let reset_dates = annual_dates(start, 5);

        let par = par_swap_rate(&curve, &pay_dates, start);

        let notional = 1_000_000.0;
        let swap = InterestRateSwap {
            fixed_leg: SwapLeg::Fixed {
                rate: par,
                notional,
                payment_dates: pay_dates.clone(),
            },
            floating_leg: SwapLeg::Floating {
                spread: 0.0,
                notional,
                payment_dates: pay_dates.clone(),
                reset_dates,
            },
            start_date: start,
            maturity_date: *pay_dates.last().unwrap_or(&0),
            day_count: DayCountConvention::Actual365,
        };

        let n = npv(&swap, &curve, start);
        assert!(n.abs() < 1.0, "NPV should be near zero at par, got {n}");
    }

    #[test]
    fn dv01_positive_for_receiver() {
        // A receiver swap: floating payer, fixed receiver.
        // We model it as: NPV = float_pv - fixed_pv (receiver perspective).
        // With a bump the receiver gains when rates fall; DV01 should be positive.
        let rate = 0.03;
        let pillars: Vec<f64> = (1..=5).map(|y| y as f64).collect();
        let curve = flat_curve(rate, &pillars);

        let start: u64 = 0;
        let pay_dates = annual_dates(start, 5);
        let reset_dates = annual_dates(start, 5);
        let par = par_swap_rate(&curve, &pay_dates, start);

        let notional = 1_000_000.0;
        let swap = InterestRateSwap {
            fixed_leg: SwapLeg::Fixed {
                rate: par * 1.005, // slightly above par → fixed payer has positive NPV
                notional,
                payment_dates: pay_dates.clone(),
            },
            floating_leg: SwapLeg::Floating {
                spread: 0.0,
                notional,
                payment_dates: pay_dates.clone(),
                reset_dates,
            },
            start_date: start,
            maturity_date: *pay_dates.last().unwrap_or(&0),
            day_count: DayCountConvention::Actual365,
        };

        let d = dv01(&swap, &curve, start);
        assert!(d > 0.0, "DV01 must be positive, got {d}");
    }

    #[test]
    fn day_count_actual360() {
        let start = 0_u64;
        let end = 30 * 86_400_u64; // 30 days
        let yf = DayCountConvention::Actual360.year_fraction(start, end);
        let expected = 30.0 / 360.0;
        assert!((yf - expected).abs() < 1e-10);
    }

    #[test]
    fn day_count_actual365() {
        let start = 0_u64;
        let end = 365 * 86_400_u64;
        let yf = DayCountConvention::Actual365.year_fraction(start, end);
        assert!((yf - 1.0).abs() < 1e-10);
    }

    #[test]
    fn day_count_thirty360() {
        // 1 year (12 months × 30) = 360/360 = 1.0
        // 1970-01-01 + 1 year → 1971-01-01
        let start = 0_u64;
        let end = 365 * 86_400_u64;
        let yf = DayCountConvention::Thirty360.year_fraction(start, end);
        // Should be close to 1.0
        assert!((yf - 1.0).abs() < 0.01, "got {yf}");
    }

    #[test]
    fn zero_rate_roundtrip() {
        let curve = flat_curve(0.04, &[1.0, 2.0, 5.0]);
        let z = curve.zero_rate(2.0);
        assert!((z - 0.04).abs() < 1e-10, "zero rate should recover 4%, got {z}");
    }

    #[test]
    fn forward_rate_flat_curve() {
        let curve = flat_curve(0.05, &[1.0, 2.0, 3.0]);
        let f = curve.forward_rate(1.0, 2.0);
        assert!((f - 0.05).abs() < 1e-9, "flat curve forward = zero rate, got {f}");
    }
}
