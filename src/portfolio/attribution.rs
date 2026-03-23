//! Brinson-Hood-Beebower (BHB) performance attribution.
//!
//! Decomposes active return into allocation effect, selection effect, and
//! interaction effect across segments (sectors, asset classes, regions, etc.).
//!
//! ## Models
//!
//! | Type | Formula |
//! |------|---------|
//! | Allocation | (wp - wb) * rb |
//! | Selection  | wb * (rp - rb) |
//! | Interaction| (wp - wb) * (rp - rb) |
//! | BF Allocation | (wp - wb) * (rb - RB) |
//!
//! where `wp` = portfolio weight, `wb` = benchmark weight,
//! `rp` = portfolio segment return, `rb` = benchmark segment return,
//! `RB` = total benchmark return.

// ─── Segment ─────────────────────────────────────────────────────────────────

/// A single attribution segment (e.g. sector, region, asset class).
#[derive(Debug, Clone)]
pub struct Segment {
    /// Descriptive name of the segment.
    pub name: String,
    /// Portfolio weight in this segment (range \[0, 1\]).
    pub portfolio_weight: f64,
    /// Benchmark weight in this segment (range \[0, 1\]).
    pub benchmark_weight: f64,
    /// Portfolio return within this segment.
    pub portfolio_return: f64,
    /// Benchmark return within this segment.
    pub benchmark_return: f64,
}

impl Segment {
    /// Construct a new segment.
    pub fn new(
        name: impl Into<String>,
        portfolio_weight: f64,
        benchmark_weight: f64,
        portfolio_return: f64,
        benchmark_return: f64,
    ) -> Self {
        Self {
            name: name.into(),
            portfolio_weight,
            benchmark_weight,
            portfolio_return,
            benchmark_return,
        }
    }

    /// BHB allocation effect: (wp - wb) * rb.
    pub fn allocation_effect(&self) -> f64 {
        (self.portfolio_weight - self.benchmark_weight) * self.benchmark_return
    }

    /// BHB selection effect: wb * (rp - rb).
    pub fn selection_effect(&self) -> f64 {
        self.benchmark_weight * (self.portfolio_return - self.benchmark_return)
    }

    /// BHB interaction effect: (wp - wb) * (rp - rb).
    pub fn interaction_effect(&self) -> f64 {
        (self.portfolio_weight - self.benchmark_weight)
            * (self.portfolio_return - self.benchmark_return)
    }

    /// Brinson-Fachler allocation effect: (wp - wb) * (rb - RB).
    ///
    /// This variant removes the benchmark-return level effect, making
    /// allocation skill more clearly attributable to over/underweighting
    /// segments relative to the total benchmark.
    pub fn bf_allocation_effect(&self, benchmark_total_return: f64) -> f64 {
        (self.portfolio_weight - self.benchmark_weight)
            * (self.benchmark_return - benchmark_total_return)
    }
}

// ─── BHBAttribution ──────────────────────────────────────────────────────────

/// Result of a Brinson-Hood-Beebower attribution analysis.
#[derive(Debug, Clone)]
pub struct BHBAttribution {
    /// Input segments, one per row in the attribution table.
    pub segments: Vec<Segment>,
    /// Sum of allocation effects across all segments.
    pub total_allocation: f64,
    /// Sum of selection effects across all segments.
    pub total_selection: f64,
    /// Sum of interaction effects across all segments.
    pub total_interaction: f64,
    /// Total active return: portfolio total return minus benchmark total return.
    pub total_active_return: f64,
}

impl BHBAttribution {
    /// Verify the attribution identity:
    /// total_allocation + total_selection + total_interaction ≈ total_active_return.
    ///
    /// Returns the residual (should be near zero).
    pub fn residual(&self) -> f64 {
        (self.total_allocation + self.total_selection + self.total_interaction)
            - self.total_active_return
    }
}

// ─── BHBAttributor ───────────────────────────────────────────────────────────

/// Computes Brinson-Hood-Beebower attribution from a set of segments.
pub struct BHBAttributor {
    segments: Vec<Segment>,
}

impl BHBAttributor {
    /// Create a new attributor with the provided segments.
    pub fn new(segments: Vec<Segment>) -> Self {
        Self { segments }
    }

    /// Run the BHB attribution decomposition.
    pub fn compute(&self) -> BHBAttribution {
        let total_allocation: f64 = self.segments.iter().map(|s| s.allocation_effect()).sum();
        let total_selection: f64 = self.segments.iter().map(|s| s.selection_effect()).sum();
        let total_interaction: f64 = self.segments.iter().map(|s| s.interaction_effect()).sum();

        // Total portfolio return = Σ(wp * rp); total benchmark = Σ(wb * rb)
        let portfolio_total: f64 = self
            .segments
            .iter()
            .map(|s| s.portfolio_weight * s.portfolio_return)
            .sum();
        let benchmark_total: f64 = self
            .segments
            .iter()
            .map(|s| s.benchmark_weight * s.benchmark_return)
            .sum();
        let total_active_return = portfolio_total - benchmark_total;

        BHBAttribution {
            segments: self.segments.clone(),
            total_allocation,
            total_selection,
            total_interaction,
            total_active_return,
        }
    }

    /// Compute the Brinson-Fachler allocation effect for each segment.
    ///
    /// Returns a vec of `(segment_name, bf_allocation_effect)`.
    pub fn brinson_fachler_allocations(&self) -> Vec<(String, f64)> {
        let benchmark_total: f64 = self
            .segments
            .iter()
            .map(|s| s.benchmark_weight * s.benchmark_return)
            .sum();
        self.segments
            .iter()
            .map(|s| (s.name.clone(), s.bf_allocation_effect(benchmark_total)))
            .collect()
    }

    /// Per-segment summary: `(name, allocation, selection, interaction)`.
    pub fn sector_summary(&self) -> Vec<(String, f64, f64, f64)> {
        self.segments
            .iter()
            .map(|s| {
                (
                    s.name.clone(),
                    s.allocation_effect(),
                    s.selection_effect(),
                    s.interaction_effect(),
                )
            })
            .collect()
    }
}

/// Brinson-Fachler allocation effect for a single segment given the total
/// benchmark return.
///
/// `(wp - wb) * (rb - benchmark_total_return)`
pub fn bf_allocation_effect(segment: &Segment, benchmark_total_return: f64) -> f64 {
    segment.bf_allocation_effect(benchmark_total_return)
}

// ─── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(wp: f64, wb: f64, rp: f64, rb: f64) -> Segment {
        Segment::new("test", wp, wb, rp, rb)
    }

    // ── single-segment identity ──────────────────────────────────────────────

    #[test]
    fn single_segment_zero_active_when_equal() {
        // If portfolio == benchmark in every way, active return = 0.
        let segments = vec![seg(0.5, 0.5, 0.10, 0.10), seg(0.5, 0.5, 0.05, 0.05)];
        let attr = BHBAttributor::new(segments).compute();
        assert!(attr.total_active_return.abs() < 1e-12, "zero active return expected");
        assert!(attr.total_allocation.abs() < 1e-12);
        assert!(attr.total_selection.abs() < 1e-12);
        assert!(attr.total_interaction.abs() < 1e-12);
    }

    #[test]
    fn allocation_effect_correct() {
        // Overweight a better-returning segment: allocation effect should be positive
        // wp=0.6, wb=0.4, rb=0.10 → allocation = (0.6-0.4)*0.10 = 0.02
        let s = seg(0.6, 0.4, 0.15, 0.10);
        assert!((s.allocation_effect() - 0.02).abs() < 1e-12);
    }

    #[test]
    fn selection_effect_correct() {
        // wb=0.5, rp=0.15, rb=0.10 → selection = 0.5*(0.15-0.10) = 0.025
        let s = seg(0.5, 0.5, 0.15, 0.10);
        assert!((s.selection_effect() - 0.025).abs() < 1e-12);
    }

    #[test]
    fn interaction_effect_correct() {
        // (wp-wb)*(rp-rb) = (0.6-0.4)*(0.15-0.10) = 0.2*0.05 = 0.01
        let s = seg(0.6, 0.4, 0.15, 0.10);
        assert!((s.interaction_effect() - 0.01).abs() < 1e-12);
    }

    // ── multi-segment: attribution identity ──────────────────────────────────

    #[test]
    fn multi_segment_attribution_identity() {
        // BHB identity: Σ(alloc + sel + interact) ≈ total_active_return
        let segments = vec![
            Segment::new("Tech",    0.30, 0.25, 0.20, 0.15),
            Segment::new("Energy",  0.20, 0.25, 0.08, 0.10),
            Segment::new("Finance", 0.50, 0.50, 0.12, 0.11),
        ];
        let attr = BHBAttributor::new(segments).compute();
        let residual = attr.residual();
        assert!(
            residual.abs() < 1e-10,
            "BHB identity violated: residual={residual:.2e}"
        );
    }

    #[test]
    fn sector_summary_length_matches() {
        let segments = vec![
            Segment::new("A", 0.5, 0.4, 0.10, 0.08),
            Segment::new("B", 0.5, 0.6, 0.06, 0.07),
        ];
        let attributor = BHBAttributor::new(segments);
        let summary = attributor.sector_summary();
        assert_eq!(summary.len(), 2);
    }

    // ── Brinson-Fachler vs BHB ───────────────────────────────────────────────

    #[test]
    fn bf_differs_from_bhb_when_benchmark_return_nonzero() {
        // BF removes the benchmark-level effect, so results differ when total benchmark != 0
        let segments = vec![
            Segment::new("A", 0.6, 0.5, 0.15, 0.12),
            Segment::new("B", 0.4, 0.5, 0.08, 0.06),
        ];
        let benchmark_total: f64 = segments
            .iter()
            .map(|s| s.benchmark_weight * s.benchmark_return)
            .sum();

        let bhb_alloc: f64 = segments.iter().map(|s| s.allocation_effect()).sum();
        let bf_alloc: f64 = segments
            .iter()
            .map(|s| s.bf_allocation_effect(benchmark_total))
            .sum();

        // BF allocation ≠ BHB allocation when benchmark total return ≠ 0
        assert!((bhb_alloc - bf_alloc).abs() > 1e-10 || benchmark_total.abs() < 1e-12,
            "BF and BHB should differ when benchmark return is non-zero");
    }

    #[test]
    fn bf_allocation_via_free_function() {
        let s = Segment::new("X", 0.6, 0.4, 0.10, 0.08);
        let rb_total = 0.05;
        let expected = (0.6 - 0.4) * (0.08 - 0.05); // 0.006
        assert!((bf_allocation_effect(&s, rb_total) - expected).abs() < 1e-12);
    }
}
