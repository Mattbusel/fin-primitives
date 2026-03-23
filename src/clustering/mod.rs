//! Asset clustering using k-means on return correlations.
//!
//! Provides [`KMeans`] for general k-means clustering and [`CorrelationClusterer`]
//! which builds an NxN Pearson correlation matrix from asset return series and
//! clusters assets using the correlation rows as feature vectors.

/// A named asset return series.
#[derive(Debug, Clone)]
pub struct AssetReturns {
    /// Ticker symbol or asset identifier.
    pub symbol: String,
    /// Daily (or periodic) return observations.
    pub returns: Vec<f64>,
}

/// Cluster assignment for a single asset.
#[derive(Debug, Clone)]
pub struct ClusterAssignment {
    /// Asset identifier.
    pub symbol: String,
    /// Zero-based cluster index.
    pub cluster_id: usize,
    /// Euclidean distance from the asset's feature vector to its centroid.
    pub distance_to_centroid: f64,
}

/// Output of a k-means run.
#[derive(Debug, Clone)]
pub struct KMeansResult {
    /// Per-point cluster assignments.
    pub assignments: Vec<ClusterAssignment>,
    /// Final centroid positions (`k` vectors, each of length `d`).
    pub centroids: Vec<Vec<f64>>,
    /// Total within-cluster sum of squared distances (inertia).
    pub inertia: f64,
    /// Number of iterations performed before convergence or reaching `max_iter`.
    pub iterations: u32,
}

/// Stateless k-means clustering over arbitrary feature vectors.
pub struct KMeans;

impl KMeans {
    /// Fit k-means to `data` (slice of feature vectors, all same length).
    ///
    /// Uses a Linear Congruential Generator seeded with `seed` for reproducible
    /// centroid initialisation.  Runs at most `max_iter` assignment/update cycles.
    ///
    /// # Panics
    /// Panics if `data` is empty, `k` is zero, or `k` exceeds `data.len()`.
    pub fn fit(data: &[Vec<f64>], k: usize, max_iter: u32, seed: u64) -> KMeansResult {
        assert!(!data.is_empty(), "data must be non-empty");
        assert!(k > 0, "k must be > 0");
        assert!(k <= data.len(), "k must not exceed number of data points");

        let n = data.len();
        let d = data[0].len();

        // --- Initialise centroids via LCG ---
        let mut lcg = seed;
        let mut centroid_indices: Vec<usize> = Vec::with_capacity(k);
        while centroid_indices.len() < k {
            lcg = lcg.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            let idx = (lcg >> 33) as usize % n;
            if !centroid_indices.contains(&idx) {
                centroid_indices.push(idx);
            }
        }
        let mut centroids: Vec<Vec<f64>> = centroid_indices.iter().map(|&i| data[i].clone()).collect();

        let mut assignments = vec![0usize; n];
        let mut iterations = 0u32;

        for _iter in 0..max_iter {
            iterations += 1;
            let mut changed = false;

            // Assignment step
            for (i, point) in data.iter().enumerate() {
                let best = (0..k)
                    .min_by(|&a, &b| {
                        euclidean_sq(point, &centroids[a])
                            .partial_cmp(&euclidean_sq(point, &centroids[b]))
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .unwrap_or(0);
                if assignments[i] != best {
                    assignments[i] = best;
                    changed = true;
                }
            }

            // Update step
            let mut sums = vec![vec![0.0f64; d]; k];
            let mut counts = vec![0usize; k];
            for (i, point) in data.iter().enumerate() {
                let c = assignments[i];
                counts[c] += 1;
                for (j, &v) in point.iter().enumerate() {
                    sums[c][j] += v;
                }
            }
            for c in 0..k {
                if counts[c] > 0 {
                    for j in 0..d {
                        centroids[c][j] = sums[c][j] / counts[c] as f64;
                    }
                }
            }

            if !changed {
                break;
            }
        }

        // Build result
        let inertia: f64 = data
            .iter()
            .zip(assignments.iter())
            .map(|(p, &c)| euclidean_sq(p, &centroids[c]))
            .sum();

        // We need symbol names — placeholder; CorrelationClusterer will supply them.
        let cluster_assignments: Vec<ClusterAssignment> = data
            .iter()
            .zip(assignments.iter())
            .enumerate()
            .map(|(i, (p, &c))| ClusterAssignment {
                symbol: i.to_string(),
                cluster_id: c,
                distance_to_centroid: euclidean_sq(p, &centroids[c]).sqrt(),
            })
            .collect();

        KMeansResult {
            assignments: cluster_assignments,
            centroids,
            inertia,
            iterations,
        }
    }
}

// ---------------------------------------------------------------------------
// Correlation-based clusterer
// ---------------------------------------------------------------------------

/// Clusters assets by their Pearson correlation profile.
///
/// Builds an N×N Pearson correlation matrix from a set of asset return series,
/// then uses each asset's row of the correlation matrix as its feature vector
/// for k-means clustering.
pub struct CorrelationClusterer {
    symbols: Vec<String>,
    /// NxN correlation matrix stored row-major.
    corr_matrix: Vec<Vec<f64>>,
    last_result: Option<KMeansResult>,
}

impl CorrelationClusterer {
    /// Build a [`CorrelationClusterer`] from a slice of asset return series.
    ///
    /// All series must have the same length; shorter series are ignored (not
    /// panicked) but correlation between mismatched-length pairs is set to `0.0`.
    pub fn from_returns(assets: &[AssetReturns]) -> Self {
        let n = assets.len();
        let mut corr_matrix = vec![vec![0.0f64; n]; n];

        for i in 0..n {
            corr_matrix[i][i] = 1.0;
            for j in (i + 1)..n {
                let c = pearson_correlation(&assets[i].returns, &assets[j].returns);
                corr_matrix[i][j] = c;
                corr_matrix[j][i] = c;
            }
        }

        let symbols = assets.iter().map(|a| a.symbol.clone()).collect();
        Self { symbols, corr_matrix, last_result: None }
    }

    /// Cluster assets into `k` groups using k-means on their correlation rows.
    pub fn cluster(&mut self, k: usize, seed: u64) -> KMeansResult {
        let mut result = KMeans::fit(&self.corr_matrix, k, 300, seed);

        // Replace numeric placeholder symbols with actual asset symbols.
        for (assign, sym) in result.assignments.iter_mut().zip(self.symbols.iter()) {
            assign.symbol = sym.clone();
        }

        self.last_result = Some(result.clone());
        result
    }

    /// Returns `(symbol, cluster_id)` pairs from the most recent [`cluster`] call.
    ///
    /// Returns an empty `Vec` if [`cluster`] has not been called yet.
    pub fn cluster_labels(&self) -> Vec<(String, usize)> {
        match &self.last_result {
            Some(r) => r
                .assignments
                .iter()
                .map(|a| (a.symbol.clone(), a.cluster_id))
                .collect(),
            None => Vec::new(),
        }
    }

    /// Compute the mean silhouette coefficient for a given [`KMeansResult`].
    ///
    /// Silhouette score ∈ [-1, 1]; higher is better.  Returns `0.0` if there
    /// is only one cluster or the result has fewer than 2 assignments.
    pub fn silhouette_score(&self, result: &KMeansResult) -> f64 {
        let n = result.assignments.len();
        if n < 2 {
            return 0.0;
        }
        let k = result.centroids.len();
        if k == 1 {
            return 0.0;
        }

        // Map symbol → row index in corr_matrix
        let mut sil_sum = 0.0f64;
        let mut count = 0usize;

        for (i, assign) in result.assignments.iter().enumerate() {
            // Find index of this symbol in self.symbols
            let row_idx = match self.symbols.iter().position(|s| s == &assign.symbol) {
                Some(idx) => idx,
                None => continue,
            };
            let feat = &self.corr_matrix[row_idx];
            let my_cluster = assign.cluster_id;

            // a(i): mean distance to same-cluster points
            let same_cluster_pts: Vec<usize> = result
                .assignments
                .iter()
                .enumerate()
                .filter(|(j, a)| *j != i && a.cluster_id == my_cluster)
                .map(|(j, _)| j)
                .collect();

            let a = if same_cluster_pts.is_empty() {
                0.0
            } else {
                let sum: f64 = same_cluster_pts
                    .iter()
                    .map(|&j| {
                        let other_sym = &result.assignments[j].symbol;
                        let other_idx = self
                            .symbols
                            .iter()
                            .position(|s| s == other_sym)
                            .unwrap_or(0);
                        euclidean_sq(feat, &self.corr_matrix[other_idx]).sqrt()
                    })
                    .sum();
                sum / same_cluster_pts.len() as f64
            };

            // b(i): min mean distance to any other cluster
            let mut b = f64::INFINITY;
            for c in 0..k {
                if c == my_cluster {
                    continue;
                }
                let other_pts: Vec<usize> = result
                    .assignments
                    .iter()
                    .enumerate()
                    .filter(|(_, a)| a.cluster_id == c)
                    .map(|(j, _)| j)
                    .collect();
                if other_pts.is_empty() {
                    continue;
                }
                let sum: f64 = other_pts
                    .iter()
                    .map(|&j| {
                        let other_sym = &result.assignments[j].symbol;
                        let other_idx = self
                            .symbols
                            .iter()
                            .position(|s| s == other_sym)
                            .unwrap_or(0);
                        euclidean_sq(feat, &self.corr_matrix[other_idx]).sqrt()
                    })
                    .sum();
                let mean_dist = sum / other_pts.len() as f64;
                if mean_dist < b {
                    b = mean_dist;
                }
            }

            let denom = a.max(b);
            let s_i = if denom == 0.0 { 0.0 } else { (b - a) / denom };
            sil_sum += s_i;
            count += 1;
        }

        if count == 0 { 0.0 } else { sil_sum / count as f64 }
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn euclidean_sq(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b.iter()).map(|(&x, &y)| (x - y).powi(2)).sum()
}

fn pearson_correlation(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len().min(b.len());
    if n < 2 {
        return 0.0;
    }
    let mean_a = a[..n].iter().sum::<f64>() / n as f64;
    let mean_b = b[..n].iter().sum::<f64>() / n as f64;
    let mut cov = 0.0f64;
    let mut var_a = 0.0f64;
    let mut var_b = 0.0f64;
    for i in 0..n {
        let da = a[i] - mean_a;
        let db = b[i] - mean_b;
        cov += da * db;
        var_a += da * da;
        var_b += db * db;
    }
    let denom = (var_a * var_b).sqrt();
    if denom == 0.0 { 0.0 } else { cov / denom }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_asset(symbol: &str, returns: Vec<f64>) -> AssetReturns {
        AssetReturns { symbol: symbol.to_string(), returns }
    }

    // ---- KMeans ----

    #[test]
    fn test_kmeans_two_clear_clusters() {
        // Points near 0 and points near 10 should split cleanly
        let data: Vec<Vec<f64>> = vec![
            vec![0.1], vec![0.2], vec![0.0],
            vec![10.0], vec![10.1], vec![9.9],
        ];
        let result = KMeans::fit(&data, 2, 100, 42);
        assert_eq!(result.centroids.len(), 2);
        assert_eq!(result.assignments.len(), 6);
        // Verify the two clusters exist
        let c0 = result.assignments[0].cluster_id;
        let c3 = result.assignments[3].cluster_id;
        assert_ne!(c0, c3, "near-0 and near-10 must be in different clusters");
    }

    #[test]
    fn test_kmeans_k_equals_n() {
        let data: Vec<Vec<f64>> = vec![vec![1.0], vec![2.0], vec![3.0]];
        let result = KMeans::fit(&data, 3, 10, 7);
        assert_eq!(result.centroids.len(), 3);
        assert_eq!(result.assignments.len(), 3);
    }

    #[test]
    fn test_kmeans_inertia_non_negative() {
        let data: Vec<Vec<f64>> = vec![vec![1.0, 2.0], vec![3.0, 4.0], vec![5.0, 6.0]];
        let result = KMeans::fit(&data, 2, 50, 1);
        assert!(result.inertia >= 0.0);
    }

    #[test]
    fn test_kmeans_iterations_bounded() {
        let data: Vec<Vec<f64>> = (0..20).map(|i| vec![i as f64]).collect();
        let result = KMeans::fit(&data, 3, 5, 99);
        assert!(result.iterations <= 5);
    }

    // ---- pearson_correlation ----

    #[test]
    fn test_pearson_perfect_positive() {
        let a = vec![1.0, 2.0, 3.0, 4.0];
        let b = vec![2.0, 4.0, 6.0, 8.0];
        let c = pearson_correlation(&a, &b);
        assert!((c - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_pearson_perfect_negative() {
        let a = vec![1.0, 2.0, 3.0, 4.0];
        let b = vec![4.0, 3.0, 2.0, 1.0];
        let c = pearson_correlation(&a, &b);
        assert!((c + 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_pearson_too_short() {
        assert_eq!(pearson_correlation(&[1.0], &[1.0]), 0.0);
        assert_eq!(pearson_correlation(&[], &[]), 0.0);
    }

    // ---- CorrelationClusterer ----

    #[test]
    fn test_correlation_clusterer_diagonal_ones() {
        let assets = vec![
            make_asset("A", vec![0.01, -0.01, 0.02, -0.02]),
            make_asset("B", vec![0.02, -0.02, 0.04, -0.04]),
            make_asset("C", vec![-0.01, 0.01, -0.02, 0.02]),
        ];
        let clusterer = CorrelationClusterer::from_returns(&assets);
        // Diagonal must be 1.0
        for i in 0..3 {
            assert!((clusterer.corr_matrix[i][i] - 1.0).abs() < 1e-10);
        }
        // A and B are perfectly positively correlated
        assert!((clusterer.corr_matrix[0][1] - 1.0).abs() < 1e-6);
        // A and C are perfectly negatively correlated
        assert!((clusterer.corr_matrix[0][2] + 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_correlation_clusterer_cluster_labels() {
        let assets = vec![
            make_asset("A", vec![0.01, 0.02, 0.03]),
            make_asset("B", vec![0.01, 0.02, 0.03]),
            make_asset("C", vec![-0.01, -0.02, -0.03]),
        ];
        let mut clusterer = CorrelationClusterer::from_returns(&assets);
        let _result = clusterer.cluster(2, 42);
        let labels = clusterer.cluster_labels();
        assert_eq!(labels.len(), 3);
        let symbols: Vec<&str> = labels.iter().map(|(s, _)| s.as_str()).collect();
        assert!(symbols.contains(&"A"));
        assert!(symbols.contains(&"B"));
        assert!(symbols.contains(&"C"));
    }

    #[test]
    fn test_cluster_labels_before_cluster_call() {
        let assets = vec![make_asset("X", vec![0.01, 0.02])];
        let clusterer = CorrelationClusterer::from_returns(&assets);
        assert!(clusterer.cluster_labels().is_empty());
    }

    #[test]
    fn test_silhouette_score_range() {
        let assets = vec![
            make_asset("A", vec![0.01, 0.02, 0.03, 0.04]),
            make_asset("B", vec![0.01, 0.02, 0.03, 0.04]),
            make_asset("C", vec![-0.03, -0.02, -0.01, 0.00]),
            make_asset("D", vec![-0.03, -0.02, -0.01, 0.00]),
        ];
        let mut clusterer = CorrelationClusterer::from_returns(&assets);
        let result = clusterer.cluster(2, 5);
        let score = clusterer.silhouette_score(&result);
        assert!(score >= -1.0 && score <= 1.0, "Silhouette must be in [-1, 1]");
    }

    #[test]
    fn test_silhouette_single_cluster() {
        let assets = vec![
            make_asset("A", vec![0.01, 0.02]),
            make_asset("B", vec![0.01, 0.02]),
        ];
        let mut clusterer = CorrelationClusterer::from_returns(&assets);
        let result = clusterer.cluster(1, 1);
        assert_eq!(clusterer.silhouette_score(&result), 0.0);
    }
}
