//! Mortgage calculations: amortization, prepayment, and refinancing analysis.

/// Defines the terms of a mortgage loan.
#[derive(Debug, Clone, PartialEq)]
pub struct MortgageTerms {
    /// Original loan principal (purchase price minus down payment).
    pub principal: f64,
    /// Annual interest rate as a decimal (e.g. 0.065 for 6.5%).
    pub annual_rate: f64,
    /// Total loan term in months.
    pub term_months: u32,
    /// Down payment amount (not included in the principal — caller should subtract).
    pub down_payment: f64,
}

/// A single row in an amortization schedule.
#[derive(Debug, Clone, PartialEq)]
pub struct AmortizationRow {
    /// Month number (1-indexed).
    pub month: u32,
    /// Total payment made this month.
    pub payment: f64,
    /// Portion of payment applied to principal.
    pub principal: f64,
    /// Portion of payment applied to interest.
    pub interest: f64,
    /// Remaining loan balance after this payment.
    pub balance: f64,
}

/// Stateless calculator for mortgage-related computations.
pub struct MortgageCalculator;

impl MortgageCalculator {
    /// Computes the fixed monthly payment using the standard annuity formula:
    ///
    /// `payment = P * r * (1+r)^n / ((1+r)^n - 1)`
    ///
    /// where `r = annual_rate / 12` and `n = term_months`.
    /// Returns 0 if the annual rate is zero (interest-free loan).
    pub fn monthly_payment(terms: &MortgageTerms) -> f64 {
        let r = terms.annual_rate / 12.0;
        let n = terms.term_months as f64;
        if r == 0.0 {
            return terms.principal / n;
        }
        let factor = (1.0 + r).powf(n);
        terms.principal * r * factor / (factor - 1.0)
    }

    /// Generates the full amortization schedule as a `Vec<AmortizationRow>`.
    pub fn amortization_schedule(terms: &MortgageTerms) -> Vec<AmortizationRow> {
        let payment = Self::monthly_payment(terms);
        let r = terms.annual_rate / 12.0;
        let mut balance = terms.principal;
        let mut schedule = Vec::with_capacity(terms.term_months as usize);

        for month in 1..=terms.term_months {
            let interest = balance * r;
            let principal_paid = (payment - interest).max(0.0);
            // On the last month, pay off the remaining balance exactly.
            let principal_paid = if month == terms.term_months {
                balance
            } else {
                principal_paid.min(balance)
            };
            let actual_payment = if month == terms.term_months {
                principal_paid + interest
            } else {
                payment
            };
            balance -= principal_paid;
            schedule.push(AmortizationRow {
                month,
                payment: actual_payment,
                principal: principal_paid,
                interest,
                balance: balance.max(0.0),
            });
        }
        schedule
    }

    /// Returns the total interest paid over the life of the loan.
    pub fn total_interest(terms: &MortgageTerms) -> f64 {
        let schedule = Self::amortization_schedule(terms);
        schedule.iter().map(|r| r.interest).sum()
    }

    /// Calculates the savings from making an additional `extra_payment` each month.
    ///
    /// Returns `(months_saved, interest_saved)`.
    pub fn prepayment_savings(terms: &MortgageTerms, extra_payment: f64) -> (u32, f64) {
        let base_interest = Self::total_interest(terms);
        let base_months = terms.term_months;

        // Simulate with the extra payment.
        let payment = Self::monthly_payment(terms) + extra_payment;
        let r = terms.annual_rate / 12.0;
        let mut balance = terms.principal;
        let mut months_paid: u32 = 0;
        let mut total_interest_paid = 0.0;

        while balance > 0.0 && months_paid < base_months * 2 {
            let interest = balance * r;
            let principal_paid = (payment - interest).min(balance).max(0.0);
            total_interest_paid += interest;
            balance -= principal_paid;
            months_paid += 1;
            if balance <= 1e-6 {
                break;
            }
        }

        let months_saved = base_months.saturating_sub(months_paid);
        let interest_saved = (base_interest - total_interest_paid).max(0.0);
        (months_saved, interest_saved)
    }

    /// Calculates the number of months until refinancing breaks even.
    ///
    /// Returns `None` if the new loan never saves enough to cover closing costs.
    ///
    /// # Parameters
    /// - `old_terms`: Original loan terms.
    /// - `new_terms`: Refinanced loan terms (principal should reflect current balance).
    /// - `closing_costs`: Up-front costs of refinancing.
    /// - `months_elapsed`: Number of months already paid on the old loan.
    pub fn refinance_breakeven(
        old_terms: &MortgageTerms,
        new_terms: &MortgageTerms,
        closing_costs: f64,
        months_elapsed: u32,
    ) -> Option<u32> {
        let old_payment = Self::monthly_payment(old_terms);
        let new_payment = Self::monthly_payment(new_terms);
        let monthly_savings = old_payment - new_payment;

        if monthly_savings <= 0.0 {
            return None;
        }

        // Determine the remaining balance on the old loan at months_elapsed.
        let old_schedule = Self::amortization_schedule(old_terms);
        let remaining_balance = if months_elapsed == 0 {
            old_terms.principal
        } else {
            let idx = (months_elapsed as usize).min(old_schedule.len()) - 1;
            old_schedule[idx].balance
        };

        // The new loan's principal should cover the remaining balance.
        // Total cost to compare: closing costs vs monthly savings.
        let _ = remaining_balance; // informational; caller sets new_terms.principal

        let months_to_breakeven = (closing_costs / monthly_savings).ceil() as u32;

        // Ensure the breakeven is within the new loan's term.
        if months_to_breakeven <= new_terms.term_months {
            Some(months_to_breakeven)
        } else {
            None
        }
    }

    /// Calculates the loan-to-value (LTV) ratio.
    ///
    /// `ltv = principal / current_value`
    pub fn ltv_ratio(terms: &MortgageTerms, current_value: f64) -> f64 {
        if current_value == 0.0 {
            return f64::INFINITY;
        }
        terms.principal / current_value
    }
}

// ── MBS / Advanced Mortgage Analytics ────────────────────────────────────────

/// Mortgage product type.
#[derive(Debug, Clone, PartialEq)]
pub enum MortgageType {
    /// Fixed-rate mortgage.
    Fixed,
    /// Adjustable-rate mortgage.
    AdjustableRate {
        /// Initial interest rate (annual, decimal).
        initial_rate: f64,
        /// Maximum rate change per adjustment period.
        adjustment_cap: f64,
        /// Maximum lifetime rate increase over initial.
        lifetime_cap: f64,
    },
    /// Interest-only period followed by full amortization.
    InterestOnly {
        /// Number of months in the interest-only period.
        io_period_months: u32,
    },
    /// Balloon payment at a specific month.
    BalloonPayment {
        /// Month at which the balloon payment is due.
        balloon_at_month: u32,
    },
}

/// A single mortgage loan with full analytics.
#[derive(Debug, Clone)]
pub struct MortgageLoan {
    /// Original principal balance.
    pub principal: f64,
    /// Annual interest rate as a decimal.
    pub annual_rate: f64,
    /// Total term in months.
    pub term_months: u32,
    /// Mortgage product type.
    pub mortgage_type: MortgageType,
    /// Unix timestamp of origination.
    pub origination_date: u64,
}

impl MortgageLoan {
    /// Standard amortization monthly payment: P*r*(1+r)^n / ((1+r)^n - 1).
    pub fn monthly_payment(&self) -> f64 {
        let r = self.annual_rate / 12.0;
        let n = self.term_months as f64;
        if r == 0.0 {
            return self.principal / n;
        }
        let factor = (1.0 + r).powf(n);
        self.principal * r * factor / (factor - 1.0)
    }

    /// Full amortization schedule.
    pub fn amortize(&self) -> Vec<AmortizationRow> {
        let payment = self.monthly_payment();
        let r = self.annual_rate / 12.0;
        let mut balance = self.principal;
        let mut schedule = Vec::with_capacity(self.term_months as usize);

        for month in 1..=self.term_months {
            let interest = balance * r;
            let principal_paid = if month == self.term_months {
                balance
            } else {
                (payment - interest).max(0.0).min(balance)
            };
            let actual_payment = if month == self.term_months {
                principal_paid + interest
            } else {
                payment
            };
            balance -= principal_paid;
            schedule.push(AmortizationRow {
                month,
                payment: actual_payment,
                interest,
                principal: principal_paid,
                balance: balance.max(0.0),
            });
        }
        schedule
    }

    /// Remaining balance after `after_months` payments.
    pub fn remaining_balance(&self, after_months: u32) -> f64 {
        let schedule = self.amortize();
        let idx = (after_months as usize).min(schedule.len());
        if idx == 0 {
            return self.principal;
        }
        schedule[idx - 1].balance
    }

    /// Total interest paid over the loan lifetime.
    pub fn total_interest(&self) -> f64 {
        self.amortize().iter().map(|r| r.interest).sum()
    }

    /// Loan-to-value ratio.
    pub fn ltv(&self, property_value: f64) -> f64 {
        if property_value == 0.0 {
            return f64::INFINITY;
        }
        self.principal / property_value
    }
}

/// Prepayment model for MBS analytics.
#[derive(Debug, Clone)]
pub enum PrepaymentModel {
    /// Constant prepayment rate (annual, e.g. 0.08 = 8% CPR).
    CPR(f64),
    /// PSA standard prepayment benchmark.
    PSA {
        /// PSA speed multiplier (1.0 = 100% PSA).
        psa_factor: f64,
    },
    /// Actual historical prepayment rates (monthly SMM).
    ActualHistory(Vec<f64>),
}

/// Compute the single monthly mortality (SMM) rate for a given model and month.
pub fn monthly_smm(model: &PrepaymentModel, month: u32) -> f64 {
    match model {
        PrepaymentModel::CPR(cpr) => 1.0 - (1.0 - cpr).powf(1.0 / 12.0),
        PrepaymentModel::PSA { psa_factor } => {
            // PSA: ramps from 0.2% CPR at month 1 to 6% CPR at month 30, flat thereafter
            let cpr = if month <= 30 {
                0.06 * psa_factor * (month as f64 / 30.0)
            } else {
                0.06 * psa_factor
            };
            1.0 - (1.0 - cpr).powf(1.0 / 12.0)
        }
        PrepaymentModel::ActualHistory(rates) => {
            let idx = (month as usize).saturating_sub(1);
            if idx < rates.len() {
                rates[idx]
            } else {
                0.0
            }
        }
    }
}

/// A pool of mortgage loans for MBS analytics.
#[derive(Debug, Clone)]
pub struct MbsPool {
    /// Constituent mortgage loans.
    pub loans: Vec<MortgageLoan>,
    /// Aggregate pool balance.
    pub pool_balance: f64,
    /// Weighted average coupon (annual rate).
    pub wac: f64,
    /// Weighted average maturity (months).
    pub wam: f64,
}

impl MbsPool {
    /// Construct an MBS pool, computing WAC and WAM from loans.
    pub fn new(loans: Vec<MortgageLoan>) -> Self {
        let pool_balance: f64 = loans.iter().map(|l| l.principal).sum();
        let wac = if pool_balance == 0.0 {
            0.0
        } else {
            loans.iter().map(|l| l.annual_rate * l.principal).sum::<f64>() / pool_balance
        };
        let wam = if pool_balance == 0.0 {
            0.0
        } else {
            loans.iter().map(|l| l.term_months as f64 * l.principal).sum::<f64>() / pool_balance
        };
        Self { loans, pool_balance, wac, wam }
    }

    /// Monthly cash flows: (scheduled_principal, interest, prepayment).
    pub fn cash_flows(&self, prepayment: &PrepaymentModel) -> Vec<(f64, f64, f64)> {
        let max_months = self.loans.iter().map(|l| l.term_months).max().unwrap_or(0);
        let mut result = Vec::with_capacity(max_months as usize);

        // Aggregate by summing per-loan cash flows with prepayment
        for month in 1..=max_months {
            let mut total_principal = 0.0;
            let mut total_interest = 0.0;
            let mut total_prepayment = 0.0;

            for loan in &self.loans {
                if month > loan.term_months {
                    continue;
                }
                let r = loan.annual_rate / 12.0;
                // Approximate remaining balance at start of month
                let bal = loan.remaining_balance(month - 1);
                if bal <= 0.0 {
                    continue;
                }
                let interest = bal * r;
                let payment = loan.monthly_payment();
                let sched_principal = (payment - interest).max(0.0).min(bal);
                let smm = monthly_smm(prepayment, month);
                let prepay_amt = (bal - sched_principal) * smm;
                total_interest += interest;
                total_principal += sched_principal;
                total_prepayment += prepay_amt;
            }
            result.push((total_principal, total_interest, total_prepayment));
        }
        result
    }

    /// Price of the MBS pool as PV of cash flows discounted at `discount_rate`.
    pub fn price(&self, discount_rate: f64, prepayment: &PrepaymentModel) -> f64 {
        let cfs = self.cash_flows(prepayment);
        let r = discount_rate / 12.0;
        cfs.iter().enumerate().map(|(i, (p, interest, pp))| {
            let cf = p + interest + pp;
            let t = (i + 1) as f64;
            cf / (1.0 + r).powf(t)
        }).sum()
    }

    /// Effective duration via parallel shift of ±1bp.
    pub fn duration(&self, discount_rate: f64, prepayment: &PrepaymentModel) -> f64 {
        let shift = 0.0001;
        let p_up = self.price(discount_rate + shift, prepayment);
        let p_dn = self.price(discount_rate - shift, prepayment);
        let p0 = self.price(discount_rate, prepayment);
        if p0 == 0.0 {
            return 0.0;
        }
        (p_dn - p_up) / (2.0 * shift * p0)
    }

    /// Convexity via parallel shift of ±1bp.
    pub fn convexity(&self, discount_rate: f64, prepayment: &PrepaymentModel) -> f64 {
        let shift = 0.0001;
        let p_up = self.price(discount_rate + shift, prepayment);
        let p_dn = self.price(discount_rate - shift, prepayment);
        let p0 = self.price(discount_rate, prepayment);
        if p0 == 0.0 {
            return 0.0;
        }
        (p_up + p_dn - 2.0 * p0) / (shift * shift * p0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn standard_terms() -> MortgageTerms {
        MortgageTerms {
            principal: 300_000.0,
            annual_rate: 0.06,
            term_months: 360,
            down_payment: 60_000.0,
        }
    }

    #[test]
    fn test_monthly_payment() {
        let terms = standard_terms();
        let payment = MortgageCalculator::monthly_payment(&terms);
        // Known result for $300k, 6%, 30yr: ~$1798.65
        assert!((payment - 1798.65).abs() < 0.5, "payment={payment}");
    }

    #[test]
    fn test_monthly_payment_zero_rate() {
        let terms = MortgageTerms {
            principal: 120_000.0,
            annual_rate: 0.0,
            term_months: 120,
            down_payment: 0.0,
        };
        let payment = MortgageCalculator::monthly_payment(&terms);
        assert!((payment - 1000.0).abs() < 1e-6);
    }

    #[test]
    fn test_amortization_schedule_length() {
        let terms = standard_terms();
        let schedule = MortgageCalculator::amortization_schedule(&terms);
        assert_eq!(schedule.len(), 360);
    }

    #[test]
    fn test_amortization_schedule_final_balance() {
        let terms = standard_terms();
        let schedule = MortgageCalculator::amortization_schedule(&terms);
        let last = schedule.last().unwrap();
        assert!(last.balance < 1.0, "final balance should be ~0, got {}", last.balance);
    }

    #[test]
    fn test_amortization_schedule_month_numbers() {
        let terms = MortgageTerms {
            principal: 100_000.0,
            annual_rate: 0.05,
            term_months: 12,
            down_payment: 0.0,
        };
        let schedule = MortgageCalculator::amortization_schedule(&terms);
        for (i, row) in schedule.iter().enumerate() {
            assert_eq!(row.month, (i + 1) as u32);
        }
    }

    #[test]
    fn test_total_interest_positive() {
        let terms = standard_terms();
        let interest = MortgageCalculator::total_interest(&terms);
        assert!(interest > 0.0, "total interest should be positive");
        // For $300k / 6% / 30yr total interest ≈ $347k
        assert!(interest > 300_000.0, "total interest should exceed principal for long term");
    }

    #[test]
    fn test_prepayment_savings_reduces_term() {
        let terms = standard_terms();
        let (months_saved, interest_saved) =
            MortgageCalculator::prepayment_savings(&terms, 200.0);
        assert!(months_saved > 0, "extra payment should save months");
        assert!(interest_saved > 0.0, "extra payment should save interest");
    }

    #[test]
    fn test_prepayment_savings_large_extra() {
        let terms = MortgageTerms {
            principal: 100_000.0,
            annual_rate: 0.06,
            term_months: 120,
            down_payment: 0.0,
        };
        let (months_saved, interest_saved) =
            MortgageCalculator::prepayment_savings(&terms, 5000.0);
        assert!(months_saved > 0);
        assert!(interest_saved > 0.0);
    }

    #[test]
    fn test_refinance_breakeven_lower_rate() {
        let old = MortgageTerms {
            principal: 300_000.0,
            annual_rate: 0.07,
            term_months: 360,
            down_payment: 0.0,
        };
        let new = MortgageTerms {
            principal: 295_000.0,
            annual_rate: 0.055,
            term_months: 360,
            down_payment: 0.0,
        };
        let result = MortgageCalculator::refinance_breakeven(&old, &new, 5_000.0, 24);
        assert!(result.is_some(), "should find a breakeven");
        let months = result.unwrap();
        assert!(months > 0 && months <= 360);
    }

    #[test]
    fn test_refinance_breakeven_higher_rate_none() {
        let old = MortgageTerms {
            principal: 300_000.0,
            annual_rate: 0.05,
            term_months: 360,
            down_payment: 0.0,
        };
        let new = MortgageTerms {
            principal: 300_000.0,
            annual_rate: 0.07,
            term_months: 360,
            down_payment: 0.0,
        };
        let result = MortgageCalculator::refinance_breakeven(&old, &new, 5_000.0, 0);
        assert!(result.is_none(), "higher rate should give no breakeven");
    }

    #[test]
    fn test_ltv_ratio() {
        let terms = MortgageTerms {
            principal: 240_000.0,
            annual_rate: 0.06,
            term_months: 360,
            down_payment: 60_000.0,
        };
        let ltv = MortgageCalculator::ltv_ratio(&terms, 300_000.0);
        assert!((ltv - 0.8).abs() < 1e-9);
    }

    #[test]
    fn test_ltv_ratio_zero_value() {
        let terms = standard_terms();
        let ltv = MortgageCalculator::ltv_ratio(&terms, 0.0);
        assert!(ltv.is_infinite());
    }
}
