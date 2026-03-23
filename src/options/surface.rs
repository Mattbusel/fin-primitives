//! # Module: options::surface
//!
//! ## Responsibility
//! Volatility surface: a grid of implied vols over (strike, expiry) space,
//! with bilinear interpolation, ATM vol, term structure, and smile extraction.

/// A single point on the volatility surface.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct VolPoint {
    /// Strike price.
    pub strike: f64,
    /// Time to expiry in years.
    pub expiry: f64,
    /// Implied volatility at this (strike, expiry).
    pub implied_vol: f64,
}

/// Volatility smile at a fixed expiry: a set of (strike, implied_vol) pairs.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VolSmile {
    /// Time to expiry (years) for this smile.
    pub expiry: f64,
    /// (strike, implied_vol) pairs sorted by strike ascending.
    pub points: Vec<(f64, f64)>,
}

/// A volatility surface built from a collection of `VolPoint`s.
///
/// Internally stores a sorted grid of unique strikes and expiries, with
/// bilinear interpolation for queries inside the grid.
#[derive(Debug, Clone)]
pub struct VolSurface {
    /// Unique strikes, sorted ascending.
    strikes: Vec<f64>,
    /// Unique expiries, sorted ascending.
    expiries: Vec<f64>,
    /// Grid[i_expiry][i_strike] = implied_vol.
    grid: Vec<Vec<f64>>,
}

impl VolSurface {
    /// Build a `VolSurface` from a collection of `VolPoint`s.
    ///
    /// Duplicate (strike, expiry) pairs are averaged. Gaps in the grid are
    /// filled with the nearest known value (nearest-neighbour fallback).
    pub fn from_points(points: Vec<VolPoint>) -> Self {
        // Collect unique strikes and expiries
        let mut strike_set: Vec<f64> = points.iter().map(|p| p.strike).collect();
        strike_set.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        strike_set.dedup_by(|a, b| (*a - *b).abs() < 1e-12);

        let mut expiry_set: Vec<f64> = points.iter().map(|p| p.expiry).collect();
        expiry_set.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        expiry_set.dedup_by(|a, b| (*a - *b).abs() < 1e-12);

        let n_exp = expiry_set.len();
        let n_str = strike_set.len();

        // Accumulators for averaging duplicate entries
        let mut sum_grid = vec![vec![0.0_f64; n_str]; n_exp];
        let mut cnt_grid = vec![vec![0_u32; n_str]; n_exp];

        for p in &points {
            let i_exp = expiry_set
                .iter()
                .position(|&e| (e - p.expiry).abs() < 1e-12)
                .unwrap_or(0);
            let i_str = strike_set
                .iter()
                .position(|&s| (s - p.strike).abs() < 1e-12)
                .unwrap_or(0);
            sum_grid[i_exp][i_str] += p.implied_vol;
            cnt_grid[i_exp][i_str] += 1;
        }

        // Average, filling zeros with NaN for gap detection
        let mut grid = vec![vec![f64::NAN; n_str]; n_exp];
        for i_exp in 0..n_exp {
            for i_str in 0..n_str {
                let c = cnt_grid[i_exp][i_str];
                if c > 0 {
                    grid[i_exp][i_str] = sum_grid[i_exp][i_str] / f64::from(c);
                }
            }
        }

        // Fill NaN gaps with nearest known value (simple sweep)
        Self::fill_gaps(&mut grid, n_exp, n_str);

        Self { strikes: strike_set, expiries: expiry_set, grid }
    }

    /// Bilinear interpolation for (strike, expiry).
    ///
    /// Returns `None` if the query is outside the grid boundaries.
    pub fn interpolate(&self, strike: f64, expiry: f64) -> Option<f64> {
        if self.strikes.is_empty() || self.expiries.is_empty() {
            return None;
        }
        // Check bounds
        if strike < *self.strikes.first()? || strike > *self.strikes.last()? {
            return None;
        }
        if expiry < *self.expiries.first()? || expiry > *self.expiries.last()? {
            return None;
        }

        let (i0, i1, t_s) = bracket(&self.strikes, strike);
        let (j0, j1, t_e) = bracket(&self.expiries, expiry);

        // Bilinear interpolation
        let v00 = self.grid[j0][i0];
        let v10 = self.grid[j0][i1];
        let v01 = self.grid[j1][i0];
        let v11 = self.grid[j1][i1];

        if v00.is_nan() || v10.is_nan() || v01.is_nan() || v11.is_nan() {
            return None;
        }

        let v = (1.0 - t_e) * ((1.0 - t_s) * v00 + t_s * v10)
            + t_e * ((1.0 - t_s) * v01 + t_s * v11);
        Some(v)
    }

    /// Implied vol at-the-money (spot = strike) for a given expiry.
    ///
    /// Uses interpolation across the strike axis at the nearest available
    /// expiry (or interpolates between expiries). Returns `None` outside grid.
    ///
    /// For a pure ATM query the surface must contain strikes that bracket
    /// the spot level; this implementation takes ATM as the midpoint of the
    /// strike range at the given expiry as a proxy when no spot is provided.
    /// Use `interpolate` with `strike = spot` for a proper ATM vol lookup.
    pub fn atm_vol(&self, expiry: f64) -> Option<f64> {
        if self.strikes.is_empty() || self.expiries.is_empty() {
            return None;
        }
        // Use the middle strike as ATM proxy
        let mid_idx = self.strikes.len() / 2;
        let atm_strike = self.strikes[mid_idx];
        self.interpolate(atm_strike, expiry)
    }

    /// Returns (expiry, atm_vol) pairs for all grid expiries, sorted by expiry.
    pub fn term_structure(&self) -> Vec<(f64, f64)> {
        self.expiries
            .iter()
            .enumerate()
            .filter_map(|(j, &exp)| {
                let mid = self.strikes.len() / 2;
                let vol = self.grid[j][mid];
                if vol.is_nan() { None } else { Some((exp, vol)) }
            })
            .collect()
    }

    /// Returns a `VolSmile` at the given expiry (interpolated between grid expiries).
    ///
    /// Returns `None` if the expiry is outside the grid.
    pub fn smile(&self, expiry: f64) -> Option<VolSmile> {
        if expiry < *self.expiries.first()? || expiry > *self.expiries.last()? {
            return None;
        }
        let pts: Vec<(f64, f64)> = self
            .strikes
            .iter()
            .filter_map(|&k| self.interpolate(k, expiry).map(|v| (k, v)))
            .collect();
        if pts.is_empty() {
            return None;
        }
        Some(VolSmile { expiry, points: pts })
    }

    // Fill NaN cells with the nearest non-NaN value via a simple forward/backward pass.
    fn fill_gaps(grid: &mut Vec<Vec<f64>>, n_exp: usize, n_str: usize) {
        // Forward pass over strikes for each expiry row
        for row in grid.iter_mut().take(n_exp) {
            let mut last = f64::NAN;
            for j in 0..n_str {
                if !row[j].is_nan() {
                    last = row[j];
                } else if !last.is_nan() {
                    row[j] = last;
                }
            }
            // Backward pass
            let mut last = f64::NAN;
            for j in (0..n_str).rev() {
                if !row[j].is_nan() {
                    last = row[j];
                } else if !last.is_nan() {
                    row[j] = last;
                }
            }
        }
        // Forward pass over expiries for each strike column
        for i in 0..n_str {
            let mut last = f64::NAN;
            for j in 0..n_exp {
                if !grid[j][i].is_nan() {
                    last = grid[j][i];
                } else if !last.is_nan() {
                    grid[j][i] = last;
                }
            }
            let mut last = f64::NAN;
            for j in (0..n_exp).rev() {
                if !grid[j][i].is_nan() {
                    last = grid[j][i];
                } else if !last.is_nan() {
                    grid[j][i] = last;
                }
            }
        }
    }
}

/// Returns (lower_idx, upper_idx, fraction) for bilinear interpolation.
fn bracket(sorted: &[f64], x: f64) -> (usize, usize, f64) {
    let n = sorted.len();
    if n == 1 {
        return (0, 0, 0.0);
    }
    // Binary search for insertion point
    let pos = sorted.partition_point(|&v| v <= x);
    if pos == 0 {
        return (0, 0, 0.0);
    }
    if pos >= n {
        return (n - 1, n - 1, 0.0);
    }
    let lo = pos - 1;
    let hi = pos;
    let span = sorted[hi] - sorted[lo];
    let t = if span.abs() < 1e-15 { 0.0 } else { (x - sorted[lo]) / span };
    (lo, hi, t)
}

// ─── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_surface(vol: f64) -> VolSurface {
        let strikes = [80.0, 90.0, 100.0, 110.0, 120.0];
        let expiries = [0.25, 0.5, 1.0, 2.0];
        let points: Vec<VolPoint> = strikes
            .iter()
            .flat_map(|&k| {
                expiries.iter().map(move |&e| VolPoint {
                    strike: k,
                    expiry: e,
                    implied_vol: vol,
                })
            })
            .collect();
        VolSurface::from_points(points)
    }

    fn skewed_surface() -> VolSurface {
        // vol = 0.20 + 0.05 * (100 - K)/100 + 0.10 * T
        let strikes = [80.0, 90.0, 100.0, 110.0, 120.0];
        let expiries = [0.25, 0.5, 1.0, 2.0];
        let points: Vec<VolPoint> = strikes
            .iter()
            .flat_map(|&k| {
                expiries.iter().map(move |&e| VolPoint {
                    strike: k,
                    expiry: e,
                    implied_vol: 0.20 + 0.05 * (100.0 - k) / 100.0 + 0.10 * e,
                })
            })
            .collect();
        VolSurface::from_points(points)
    }

    #[test]
    fn flat_surface_interpolate_on_grid() {
        let surf = flat_surface(0.20);
        let v = surf.interpolate(100.0, 1.0).unwrap();
        assert!((v - 0.20).abs() < 1e-10, "flat surface on-grid: {v}");
    }

    #[test]
    fn flat_surface_interpolate_between_grid() {
        let surf = flat_surface(0.20);
        // Midpoint between strikes and expiries should still return 0.20
        let v = surf.interpolate(95.0, 0.75).unwrap();
        assert!((v - 0.20).abs() < 1e-10, "flat surface off-grid: {v}");
    }

    #[test]
    fn interpolate_outside_returns_none_high_strike() {
        let surf = flat_surface(0.20);
        assert!(surf.interpolate(200.0, 1.0).is_none());
    }

    #[test]
    fn interpolate_outside_returns_none_low_strike() {
        let surf = flat_surface(0.20);
        assert!(surf.interpolate(10.0, 1.0).is_none());
    }

    #[test]
    fn interpolate_outside_returns_none_high_expiry() {
        let surf = flat_surface(0.20);
        assert!(surf.interpolate(100.0, 5.0).is_none());
    }

    #[test]
    fn interpolate_outside_returns_none_low_expiry() {
        let surf = flat_surface(0.20);
        assert!(surf.interpolate(100.0, 0.01).is_none());
    }

    #[test]
    fn skewed_surface_on_grid_point() {
        let surf = skewed_surface();
        // strike=100, expiry=1.0 → vol = 0.20 + 0 + 0.10 = 0.30
        let v = surf.interpolate(100.0, 1.0).unwrap();
        assert!((v - 0.30).abs() < 1e-10, "skewed on-grid: {v}");
    }

    #[test]
    fn skewed_surface_bilinear_accuracy() {
        let surf = skewed_surface();
        // Midpoint of (90, 0.5) and (100, 1.0): should be ~average
        let v00 = 0.20 + 0.05 * (100.0 - 90.0) / 100.0 + 0.10 * 0.5; // 0.255
        let v10 = 0.20 + 0.05 * (100.0 - 100.0) / 100.0 + 0.10 * 0.5; // 0.250
        let v01 = 0.20 + 0.05 * (100.0 - 90.0) / 100.0 + 0.10 * 1.0; // 0.305
        let v11 = 0.20 + 0.05 * (100.0 - 100.0) / 100.0 + 0.10 * 1.0; // 0.300
        let expected = 0.25 * (v00 + v10 + v01 + v11); // bilinear at t=0.5, s=0.5
        let v = surf.interpolate(95.0, 0.75).unwrap();
        assert!((v - expected).abs() < 0.005, "bilinear: {v:.4} vs {expected:.4}");
    }

    #[test]
    fn atm_vol_on_grid_expiry() {
        let surf = flat_surface(0.20);
        let v = surf.atm_vol(1.0).unwrap();
        assert!((v - 0.20).abs() < 1e-10);
    }

    #[test]
    fn atm_vol_off_grid_expiry() {
        let surf = flat_surface(0.25);
        let v = surf.atm_vol(0.75).unwrap();
        assert!((v - 0.25).abs() < 1e-10);
    }

    #[test]
    fn term_structure_sorted() {
        let surf = flat_surface(0.20);
        let ts = surf.term_structure();
        assert!(!ts.is_empty());
        for w in ts.windows(2) {
            assert!(w[0].0 < w[1].0, "term structure not sorted");
        }
    }

    #[test]
    fn term_structure_flat() {
        let surf = flat_surface(0.20);
        for (_, vol) in surf.term_structure() {
            assert!((vol - 0.20).abs() < 1e-10);
        }
    }

    #[test]
    fn smile_on_grid_expiry() {
        let surf = flat_surface(0.20);
        let smile = surf.smile(1.0).unwrap();
        assert_eq!(smile.expiry, 1.0);
        assert!(!smile.points.is_empty());
        for (_, v) in &smile.points {
            assert!((v - 0.20).abs() < 1e-10);
        }
    }

    #[test]
    fn smile_outside_returns_none() {
        let surf = flat_surface(0.20);
        assert!(surf.smile(10.0).is_none());
    }

    #[test]
    fn smile_strikes_sorted() {
        let surf = skewed_surface();
        let smile = surf.smile(0.5).unwrap();
        for w in smile.points.windows(2) {
            assert!(w[0].0 <= w[1].0, "smile strikes not sorted");
        }
    }

    #[test]
    fn from_points_single_point() {
        let pts = vec![VolPoint { strike: 100.0, expiry: 1.0, implied_vol: 0.20 }];
        let surf = VolSurface::from_points(pts);
        // Should be able to query the exact point
        let v = surf.interpolate(100.0, 1.0).unwrap();
        assert!((v - 0.20).abs() < 1e-10);
    }

    #[test]
    fn from_points_duplicate_averaged() {
        let pts = vec![
            VolPoint { strike: 100.0, expiry: 1.0, implied_vol: 0.20 },
            VolPoint { strike: 100.0, expiry: 1.0, implied_vol: 0.30 },
        ];
        let surf = VolSurface::from_points(pts);
        let v = surf.interpolate(100.0, 1.0).unwrap();
        assert!((v - 0.25).abs() < 1e-10, "duplicates should average: {v}");
    }

    #[test]
    fn smile_vol_decreases_with_strike_for_skewed() {
        // In skewed_surface, lower strike → higher vol
        let surf = skewed_surface();
        let smile = surf.smile(1.0).unwrap();
        for w in smile.points.windows(2) {
            assert!(w[0].1 >= w[1].1, "vol should decrease with strike: {} < {}", w[0].1, w[1].1);
        }
    }
}
