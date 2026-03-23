//! # Module: correlation::stats
//!
//! Rolling and pairwise correlation measures:
//! - Pearson correlation
//! - Spearman rank correlation (with average-rank tie handling)
//! - Kendall tau-b (O(n²) concordant/discordant counting)
//! - `SymbolCorrelationMatrix`: full Pearson matrix for N symbols
//! - `RollingCorrelation`: rolling-window pairwise correlations via `VecDeque`

use std::collections::HashMap;
use std::collections::VecDeque;

// ─── Pearson ─────────────────────────────────────────────────────────────────

/// Compute the Pearson product-moment correlation between two equal-length slices.
///
/// Returns `None` when:
/// - Either slice has fewer than 2 elements.
/// - Either series has zero (or near-zero) variance.
/// - The slices have different lengths.
pub fn pearson_correlation(x: &[f64], y: &[f64]) -> Option<f64> {
    if x.len() != y.len() || x.len() < 2 {
        return None;
    }
    let n = x.len() as f64;
    let sum_x: f64 = x.iter().sum();
    let sum_y: f64 = y.iter().sum();
    let sum_xy: f64 = x.iter().zip(y.iter()).map(|(a, b)| a * b).sum();
    let sum_x2: f64 = x.iter().map(|a| a * a).sum();
    let sum_y2: f64 = y.iter().map(|b| b * b).sum();

    let num = n * sum_xy - sum_x * sum_y;
    let den_sq = (n * sum_x2 - sum_x * sum_x) * (n * sum_y2 - sum_y * sum_y);
    if den_sq <= 0.0 {
        return None;
    }
    Some((num / den_sq.sqrt()).clamp(-1.0, 1.0))
}

// ─── Spearman ────────────────────────────────────────────────────────────────

/// Compute Spearman rank correlation using the rank transformation.
///
/// Ties are broken by average rank.
/// Returns `None` under the same conditions as [`pearson_correlation`].
pub fn spearman_correlation(x: &[f64], y: &[f64]) -> Option<f64> {
    if x.len() != y.len() || x.len() < 2 {
        return None;
    }
    let rx = average_ranks(x);
    let ry = average_ranks(y);
    pearson_correlation(&rx, &ry)
}

/// Assign average ranks to a slice, handling ties with average rank.
fn average_ranks(data: &[f64]) -> Vec<f64> {
    let n = data.len();
    // Create (value, original_index) pairs sorted by value
    let mut indexed: Vec<(f64, usize)> = data.iter().copied().enumerate().map(|(i, v)| (v, i)).collect();
    indexed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut ranks = vec![0.0_f64; n];
    let mut i = 0;
    while i < n {
        // Find the run of equal values
        let mut j = i + 1;
        while j < n && (indexed[j].0 - indexed[i].0).abs() < f64::EPSILON {
            j += 1;
        }
        // Average rank for positions i..j (1-indexed ranks)
        let avg_rank = (i + j + 1) as f64 / 2.0; // = ((i+1) + j) / 2 as 1-indexed
        for k in i..j {
            ranks[indexed[k].1] = avg_rank;
        }
        i = j;
    }
    ranks
}

// ─── Kendall tau-b ───────────────────────────────────────────────────────────

/// Compute Kendall tau-b correlation coefficient.
///
/// O(n²) concordant/discordant pair counting with full tie correction.
/// Returns `None` if either slice has fewer than 2 elements or slices differ in length.
pub fn kendall_tau(x: &[f64], y: &[f64]) -> Option<f64> {
    if x.len() != y.len() || x.len() < 2 {
        return None;
    }
    let n = x.len();
    let mut concordant: i64 = 0;
    let mut discordant: i64 = 0;
    let mut ties_x: i64 = 0;
    let mut ties_y: i64 = 0;
    let mut ties_xy: i64 = 0;

    for i in 0..n {
        for j in (i + 1)..n {
            let dx = x[i] - x[j];
            let dy = y[i] - y[j];
            let prod = dx * dy;
            let x_tied = dx.abs() < f64::EPSILON;
            let y_tied = dy.abs() < f64::EPSILON;

            if x_tied && y_tied {
                ties_xy += 1;
            } else if x_tied {
                ties_x += 1;
            } else if y_tied {
                ties_y += 1;
            } else if prod > 0.0 {
                concordant += 1;
            } else {
                discordant += 1;
            }
        }
    }

    let total_pairs = (n as i64 * (n as i64 - 1)) / 2;
    let n0 = total_pairs;
    let n1 = n0 - ties_x - ties_xy;
    let n2 = n0 - ties_y - ties_xy;

    let denom = (n1 as f64 * n2 as f64).sqrt();
    if denom == 0.0 {
        return None;
    }

    let tau = (concordant - discordant) as f64 / denom;
    Some(tau.clamp(-1.0, 1.0))
}

// ─── Symbol-based correlation matrix ─────────────────────────────────────────

/// Full Pearson correlation matrix for a fixed set of symbols.
///
/// Constructed from complete return histories; not updated incrementally.
/// For rolling / streaming use, see [`RollingCorrelation`].
#[derive(Debug, Clone)]
pub struct SymbolCorrelationMatrix {
    /// Symbol labels in order.
    pub symbols: Vec<String>,
    /// n×n correlation matrix (row-major).
    pub matrix: Vec<Vec<f64>>,
    /// Dimension (number of symbols).
    pub n: usize,
}

impl SymbolCorrelationMatrix {
    /// Build the Pearson correlation matrix from full return series.
    ///
    /// `symbols` and `returns` must have the same length; all return slices must also
    /// have the same length (the minimum across series is used).
    pub fn from_returns(symbols: Vec<String>, returns: Vec<Vec<f64>>) -> Self {
        let n = symbols.len();
        let mut matrix = vec![vec![1.0_f64; n]; n];

        for i in 0..n {
            for j in (i + 1)..n {
                let corr = pearson_correlation(&returns[i], &returns[j]).unwrap_or(0.0);
                matrix[i][j] = corr;
                matrix[j][i] = corr;
            }
        }

        Self { symbols, matrix, n }
    }

    /// Get the correlation between symbols at indices `i` and `j`.
    pub fn get(&self, i: usize, j: usize) -> f64 {
        self.matrix[i][j]
    }

    /// Render the correlation matrix as a plain-text ASCII table.
    pub fn to_table(&self) -> String {
        // Determine column width
        let col_w = self.symbols.iter().map(|s| s.len()).max().unwrap_or(6).max(6);
        let fmt = |v: f64| format!("{:>width$.4}", v, width = col_w);
        let pad = |s: &str| format!("{:>width$}", s, width = col_w);

        let mut out = String::new();
        // Header row
        out.push_str(&" ".repeat(col_w + 1));
        for sym in &self.symbols {
            out.push(' ');
            out.push_str(&pad(sym));
        }
        out.push('\n');

        for (i, sym) in self.symbols.iter().enumerate() {
            out.push_str(&pad(sym));
            for j in 0..self.n {
                out.push(' ');
                out.push_str(&fmt(self.matrix[i][j]));
            }
            out.push('\n');
        }
        out
    }

    /// Returns all symbol pairs where `|correlation| > threshold`.
    ///
    /// Each entry is `(symbol_a, symbol_b, correlation)` for `i < j`.
    pub fn highly_correlated(&self, threshold: f64) -> Vec<(String, String, f64)> {
        let mut result = Vec::new();
        for i in 0..self.n {
            for j in (i + 1)..self.n {
                let c = self.matrix[i][j];
                if c.abs() > threshold {
                    result.push((self.symbols[i].clone(), self.symbols[j].clone(), c));
                }
            }
        }
        result
    }

    /// Compute eigenvalues of the correlation matrix using the Jacobi sweep algorithm.
    ///
    /// Returns eigenvalues in descending order.
    /// The Jacobi method iteratively zeroes off-diagonal elements via plane rotations.
    pub fn eigenvalues(&self) -> Vec<f64> {
        if self.n == 0 {
            return vec![];
        }
        jacobi_eigenvalues(&self.matrix, self.n)
    }
}

/// Jacobi eigenvalue algorithm for symmetric matrices.
///
/// Performs up to `max_sweeps * n*(n-1)/2` rotations, converging off-diagonal elements
/// to near zero. Returns eigenvalues in descending order.
fn jacobi_eigenvalues(matrix: &[Vec<f64>], n: usize) -> Vec<f64> {
    // Copy into a flat mutable buffer
    let mut a: Vec<f64> = matrix.iter().flat_map(|row| row.iter().copied()).collect();
    let idx = |i: usize, j: usize| i * n + j;

    let max_sweeps = 100;
    let tol = 1e-10_f64;

    for _ in 0..max_sweeps {
        // Find max off-diagonal element
        let mut max_val = 0.0_f64;
        for i in 0..n {
            for j in (i + 1)..n {
                let v = a[idx(i, j)].abs();
                if v > max_val {
                    max_val = v;
                }
            }
        }
        if max_val < tol {
            break;
        }

        // One Jacobi sweep over all off-diagonal pairs
        for p in 0..n {
            for q in (p + 1)..n {
                let apq = a[idx(p, q)];
                if apq.abs() < tol {
                    continue;
                }
                let app = a[idx(p, p)];
                let aqq = a[idx(q, q)];
                let theta = 0.5 * (aqq - app) / apq;
                let t = if theta >= 0.0 {
                    1.0 / (theta + (1.0 + theta * theta).sqrt())
                } else {
                    -1.0 / (-theta + (1.0 + theta * theta).sqrt())
                };
                let c = 1.0 / (1.0 + t * t).sqrt();
                let s = t * c;

                // Update diagonal
                a[idx(p, p)] = app - t * apq;
                a[idx(q, q)] = aqq + t * apq;
                a[idx(p, q)] = 0.0;
                a[idx(q, p)] = 0.0;

                // Update off-diagonal rows/columns
                for r in 0..n {
                    if r == p || r == q {
                        continue;
                    }
                    let arp = a[idx(r, p)];
                    let arq = a[idx(r, q)];
                    a[idx(r, p)] = c * arp - s * arq;
                    a[idx(p, r)] = a[idx(r, p)];
                    a[idx(r, q)] = s * arp + c * arq;
                    a[idx(q, r)] = a[idx(r, q)];
                }
            }
        }
    }

    // Diagonal entries are eigenvalues
    let mut eigs: Vec<f64> = (0..n).map(|i| a[idx(i, i)]).collect();
    eigs.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    eigs
}

// ─── Rolling correlation ──────────────────────────────────────────────────────

/// Rolling window Pearson correlation matrix, updated tick-by-tick.
///
/// Each series is stored in a fixed-size `VecDeque`; once all series have
/// accumulated `window` values, `compute_matrix` and `pairwise` become available.
pub struct RollingCorrelation {
    /// Rolling window size.
    window: usize,
    /// Per-symbol deques.
    series: HashMap<String, VecDeque<f64>>,
}

impl RollingCorrelation {
    /// Create a new rolling correlation tracker with the given window size.
    pub fn new(window: usize) -> Self {
        Self { window, series: HashMap::new() }
    }

    /// Push a new value for the given symbol.
    ///
    /// If the symbol is not yet tracked it is initialised automatically.
    /// Once the deque reaches `window` length, the oldest value is evicted.
    pub fn push(&mut self, symbol: &str, value: f64) {
        let dq = self.series.entry(symbol.to_string()).or_insert_with(|| VecDeque::with_capacity(self.window));
        if dq.len() >= self.window {
            dq.pop_front();
        }
        dq.push_back(value);
    }

    /// Returns `true` when every tracked series has accumulated at least `window` values.
    pub fn is_ready(&self) -> bool {
        !self.series.is_empty() && self.series.values().all(|dq| dq.len() >= self.window)
    }

    /// Compute the full Pearson correlation matrix over all tracked symbols.
    ///
    /// Returns `None` if any series has fewer than `window` values.
    pub fn compute_matrix(&self) -> Option<SymbolCorrelationMatrix> {
        if !self.is_ready() {
            return None;
        }
        let mut symbols: Vec<String> = self.series.keys().cloned().collect();
        symbols.sort();
        let returns: Vec<Vec<f64>> = symbols
            .iter()
            .map(|s| self.series[s].iter().copied().collect())
            .collect();
        Some(SymbolCorrelationMatrix::from_returns(symbols, returns))
    }

    /// Compute the rolling Pearson correlation for a specific pair.
    ///
    /// Returns `None` if either symbol is not tracked or has fewer than `window` values.
    pub fn pairwise(&self, sym_a: &str, sym_b: &str) -> Option<f64> {
        let a = self.series.get(sym_a)?;
        let b = self.series.get(sym_b)?;
        if a.len() < self.window || b.len() < self.window {
            return None;
        }
        let va: Vec<f64> = a.iter().copied().collect();
        let vb: Vec<f64> = b.iter().copied().collect();
        pearson_correlation(&va, &vb)
    }
}

// ─── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pearson_perfect_positive() {
        let x = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let y = vec![2.0, 4.0, 6.0, 8.0, 10.0];
        let r = pearson_correlation(&x, &y).unwrap();
        assert!((r - 1.0).abs() < 1e-9, "r={r}");
    }

    #[test]
    fn test_pearson_perfect_negative() {
        let x = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let y = vec![10.0, 8.0, 6.0, 4.0, 2.0];
        let r = pearson_correlation(&x, &y).unwrap();
        assert!((r + 1.0).abs() < 1e-9, "r={r}");
    }

    #[test]
    fn test_pearson_insufficient_data() {
        assert!(pearson_correlation(&[1.0], &[1.0]).is_none());
        assert!(pearson_correlation(&[], &[]).is_none());
    }

    #[test]
    fn test_pearson_zero_variance() {
        let x = vec![5.0, 5.0, 5.0];
        let y = vec![1.0, 2.0, 3.0];
        assert!(pearson_correlation(&x, &y).is_none());
    }

    #[test]
    fn test_spearman_perfect_positive() {
        let x = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let y = vec![10.0, 20.0, 30.0, 40.0, 50.0];
        let r = spearman_correlation(&x, &y).unwrap();
        assert!((r - 1.0).abs() < 1e-9, "r={r}");
    }

    #[test]
    fn test_spearman_anti_correlation() {
        let x = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let y = vec![5.0, 4.0, 3.0, 2.0, 1.0];
        let r = spearman_correlation(&x, &y).unwrap();
        assert!((r + 1.0).abs() < 1e-9, "r={r}");
    }

    #[test]
    fn test_spearman_rank_transform_with_ties() {
        // With ties: ranks of [1,1,2] should be [1.5, 1.5, 3]
        let ranks = average_ranks(&[1.0, 1.0, 2.0]);
        assert!((ranks[0] - 1.5).abs() < 1e-9);
        assert!((ranks[1] - 1.5).abs() < 1e-9);
        assert!((ranks[2] - 3.0).abs() < 1e-9);
    }

    #[test]
    fn test_kendall_perfect_concordant() {
        let x = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let y = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let tau = kendall_tau(&x, &y).unwrap();
        assert!((tau - 1.0).abs() < 1e-9, "tau={tau}");
    }

    #[test]
    fn test_kendall_perfect_discordant() {
        let x = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let y = vec![5.0, 4.0, 3.0, 2.0, 1.0];
        let tau = kendall_tau(&x, &y).unwrap();
        assert!((tau + 1.0).abs() < 1e-9, "tau={tau}");
    }

    #[test]
    fn test_symbol_correlation_matrix_from_returns() {
        let symbols = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let returns = vec![
            vec![1.0, 2.0, 3.0, 4.0, 5.0],
            vec![2.0, 4.0, 6.0, 8.0, 10.0], // perfectly correlated with A
            vec![5.0, 4.0, 3.0, 2.0, 1.0],  // anti-correlated with A
        ];
        let mat = SymbolCorrelationMatrix::from_returns(symbols, returns);
        assert!((mat.get(0, 1) - 1.0).abs() < 1e-9);
        assert!((mat.get(0, 2) + 1.0).abs() < 1e-9);
        assert_eq!(mat.get(0, 0), 1.0);
    }

    #[test]
    fn test_highly_correlated_filter() {
        let symbols = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let returns = vec![
            vec![1.0, 2.0, 3.0, 4.0, 5.0],
            vec![2.0, 4.0, 6.0, 8.0, 10.0],
            vec![5.0, 4.0, 3.0, 2.0, 1.0],
        ];
        let mat = SymbolCorrelationMatrix::from_returns(symbols, returns);
        let high = mat.highly_correlated(0.9);
        // All three pairs exceed |0.9| (|r|=1.0)
        assert_eq!(high.len(), 3);
    }

    #[test]
    fn test_highly_correlated_excludes_below_threshold() {
        let symbols = vec!["A".to_string(), "B".to_string()];
        let returns = vec![
            vec![1.0, 2.0, 3.0, 4.0, 5.0],
            vec![1.0, 1.5, 1.0, 1.5, 1.0], // low correlation
        ];
        let mat = SymbolCorrelationMatrix::from_returns(symbols, returns);
        let high = mat.highly_correlated(0.99);
        assert!(high.is_empty());
    }

    #[test]
    fn test_to_table_contains_symbols() {
        let symbols = vec!["BTC".to_string(), "ETH".to_string()];
        let returns = vec![
            vec![1.0, 2.0, 3.0],
            vec![1.0, 2.0, 3.0],
        ];
        let mat = SymbolCorrelationMatrix::from_returns(symbols, returns);
        let table = mat.to_table();
        assert!(table.contains("BTC"));
        assert!(table.contains("ETH"));
    }

    #[test]
    fn test_rolling_correlation_not_ready_until_window() {
        let mut rc = RollingCorrelation::new(5);
        for i in 0..4 {
            rc.push("A", i as f64);
            rc.push("B", i as f64 * 2.0);
        }
        assert!(!rc.is_ready());
        assert!(rc.compute_matrix().is_none());
        assert!(rc.pairwise("A", "B").is_none());
    }

    #[test]
    fn test_rolling_correlation_ready_after_window() {
        let mut rc = RollingCorrelation::new(5);
        for i in 0..5 {
            rc.push("A", i as f64);
            rc.push("B", i as f64 * 2.0);
        }
        assert!(rc.is_ready());
        let r = rc.pairwise("A", "B").unwrap();
        assert!((r - 1.0).abs() < 1e-9, "r={r}");
    }

    #[test]
    fn test_rolling_window_evicts_old_values() {
        let mut rc = RollingCorrelation::new(3);
        // Push 5 values; only last 3 count
        for i in 0..5 {
            rc.push("A", i as f64);
        }
        let dq = &rc.series["A"];
        assert_eq!(dq.len(), 3);
        assert_eq!(dq[0], 2.0);
        assert_eq!(dq[2], 4.0);
    }

    #[test]
    fn test_rolling_correlation_matrix() {
        let mut rc = RollingCorrelation::new(5);
        for i in 0..5 {
            let v = i as f64;
            rc.push("X", v);
            rc.push("Y", -v);
        }
        let mat = rc.compute_matrix().unwrap();
        // X and Y are anti-correlated
        let x_idx = mat.symbols.iter().position(|s| s == "X").unwrap();
        let y_idx = mat.symbols.iter().position(|s| s == "Y").unwrap();
        assert!((mat.get(x_idx, y_idx) + 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_eigenvalues_length() {
        let symbols = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let returns = vec![
            vec![1.0, 2.0, 3.0, 4.0, 5.0],
            vec![2.0, 4.0, 6.0, 8.0, 10.0],
            vec![5.0, 4.0, 3.0, 2.0, 1.0],
        ];
        let mat = SymbolCorrelationMatrix::from_returns(symbols, returns);
        let eigs = mat.eigenvalues();
        assert_eq!(eigs.len(), 3);
    }

    #[test]
    fn test_eigenvalues_descending() {
        let symbols = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let returns = vec![
            vec![1.0, 2.0, 3.0, 4.0, 5.0],
            vec![5.0, 3.0, 1.0, 4.0, 2.0],
            vec![2.0, 5.0, 1.0, 3.0, 4.0],
        ];
        let mat = SymbolCorrelationMatrix::from_returns(symbols, returns);
        let eigs = mat.eigenvalues();
        for i in 0..eigs.len() - 1 {
            assert!(eigs[i] >= eigs[i + 1] - 1e-9, "eigs not descending: {:?}", eigs);
        }
    }
}
