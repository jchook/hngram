//! N-gram counting and aggregation (RFC-002)
//!
//! Provides data structures and logic for:
//! - Counting n-gram occurrences per bucket
//! - Computing denominators for normalization
//! - Pruning by global and per-bucket thresholds

use std::collections::HashMap;

/// Configuration for n-gram pruning thresholds
#[derive(Debug, Clone)]
pub struct PruningConfig {
    /// Minimum global count for bigrams to be admitted (default: 20)
    pub min_bigram_global: u64,
    /// Minimum global count for trigrams to be admitted (default: 10)
    pub min_trigram_global: u64,
    /// Minimum per-bucket count for bigrams (default: 3)
    pub min_bigram_bucket: u32,
    /// Minimum per-bucket count for trigrams (default: 5)
    pub min_trigram_bucket: u32,
}

impl Default for PruningConfig {
    fn default() -> Self {
        Self {
            min_bigram_global: 20,
            min_trigram_global: 10,
            min_bigram_bucket: 3,
            min_trigram_bucket: 5,
        }
    }
}

impl PruningConfig {
    /// Get the minimum global count threshold for a given n
    pub fn min_global_count(&self, n: u8) -> u64 {
        match n {
            1 => 0, // No pruning for unigrams
            2 => self.min_bigram_global,
            3 => self.min_trigram_global,
            _ => u64::MAX, // Reject n > 3
        }
    }

    /// Get the minimum per-bucket count threshold for a given n
    pub fn min_bucket_count(&self, n: u8) -> u32 {
        match n {
            1 => 1, // Unigrams always kept if count >= 1
            2 => self.min_bigram_bucket,
            3 => self.min_trigram_bucket,
            _ => u32::MAX,
        }
    }
}

/// Key for n-gram counts: (bucket, n, ngram)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NgramKey {
    pub bucket: String, // YYYY-MM-DD format
    pub n: u8,
    pub ngram: String,
}

/// Key for bucket totals: (bucket, n)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BucketKey {
    pub bucket: String,
    pub n: u8,
}

/// Aggregated n-gram counts for a processing batch
#[derive(Debug, Default)]
pub struct NgramCounter {
    /// Counts per (bucket, n, ngram)
    counts: HashMap<NgramKey, u32>,
    /// Total n-grams per (bucket, n) for denominators
    totals: HashMap<BucketKey, u64>,
}

impl NgramCounter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Process a single comment's tokens for a given bucket
    pub fn process_comment(&mut self, bucket: &str, tokens: &[String]) {
        let k = tokens.len();
        if k == 0 {
            return;
        }

        // Generate and count 1-grams, 2-grams, 3-grams
        for n in 1..=3u8 {
            let n_usize = n as usize;
            if k < n_usize {
                continue;
            }

            let ngram_count = k - n_usize + 1;

            // Update total for this bucket/n
            let bucket_key = BucketKey {
                bucket: bucket.to_string(),
                n,
            };
            *self.totals.entry(bucket_key).or_insert(0) += ngram_count as u64;

            // Count each n-gram
            for window in tokens.windows(n_usize) {
                let ngram = window.join(" ");
                let key = NgramKey {
                    bucket: bucket.to_string(),
                    n,
                    ngram,
                };
                *self.counts.entry(key).or_insert(0) += 1;
            }
        }
    }

    /// Merge another counter into this one (for parallel processing)
    pub fn merge(&mut self, other: NgramCounter) {
        for (key, count) in other.counts {
            *self.counts.entry(key).or_insert(0) += count;
        }
        for (key, total) in other.totals {
            *self.totals.entry(key).or_insert(0) += total;
        }
    }

    /// Get the raw counts (before pruning)
    pub fn counts(&self) -> &HashMap<NgramKey, u32> {
        &self.counts
    }

    /// Get the bucket totals (denominators)
    pub fn totals(&self) -> &HashMap<BucketKey, u64> {
        &self.totals
    }

    /// Compute global counts across all buckets for vocabulary admission
    pub fn global_counts(&self) -> HashMap<(u8, String), u64> {
        let mut global: HashMap<(u8, String), u64> = HashMap::new();
        for (key, &count) in &self.counts {
            *global.entry((key.n, key.ngram.clone())).or_insert(0) += count as u64;
        }
        global
    }

    /// Apply per-bucket pruning and return filtered counts
    /// Note: This does NOT affect totals (denominators must remain unpruned)
    pub fn prune_bucket_counts(&self, config: &PruningConfig) -> HashMap<NgramKey, u32> {
        self.counts
            .iter()
            .filter(|(key, &count)| count >= config.min_bucket_count(key.n))
            .map(|(k, &v)| (k.clone(), v))
            .collect()
    }

    /// Filter counts to only admitted vocabulary
    pub fn filter_to_vocabulary<S: ::std::hash::BuildHasher>(
        &self,
        admitted: &HashMap<(u8, String), (), S>,
        config: &PruningConfig,
    ) -> HashMap<NgramKey, u32> {
        self.counts
            .iter()
            .filter(|(key, &count)| {
                // Unigrams are always admitted
                if key.n == 1 {
                    return count >= config.min_bucket_count(1);
                }
                // Check vocabulary admission and bucket threshold
                admitted.contains_key(&(key.n, key.ngram.clone()))
                    && count >= config.min_bucket_count(key.n)
            })
            .map(|(k, &v)| (k.clone(), v))
            .collect()
    }
}

/// Build admitted vocabulary from global counts
pub fn build_vocabulary(
    global_counts: &HashMap<(u8, String), u64>,
    config: &PruningConfig,
) -> HashMap<(u8, String), ()> {
    global_counts
        .iter()
        .filter(|((n, _ngram), &count)| count >= config.min_global_count(*n))
        .map(|(key, _)| (key.clone(), ()))
        .collect()
}

/// Denominator contribution from a token sequence
#[derive(Debug, Clone, Copy, Default)]
pub struct DenominatorContribution {
    pub unigrams: u64,
    pub bigrams: u64,
    pub trigrams: u64,
}

impl DenominatorContribution {
    /// Calculate contributions for a token sequence of length k
    pub fn from_token_count(k: usize) -> Self {
        Self {
            unigrams: k as u64,
            bigrams: k.saturating_sub(1) as u64,
            trigrams: k.saturating_sub(2) as u64,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pruning_config_defaults() {
        let config = PruningConfig::default();
        assert_eq!(config.min_bigram_global, 20);
        assert_eq!(config.min_trigram_global, 10);
        assert_eq!(config.min_bigram_bucket, 3);
        assert_eq!(config.min_trigram_bucket, 5);
    }

    #[test]
    fn test_min_global_count() {
        let config = PruningConfig::default();
        assert_eq!(config.min_global_count(1), 0);
        assert_eq!(config.min_global_count(2), 20);
        assert_eq!(config.min_global_count(3), 10);
    }

    #[test]
    fn test_min_bucket_count() {
        let config = PruningConfig::default();
        assert_eq!(config.min_bucket_count(1), 1);
        assert_eq!(config.min_bucket_count(2), 3);
        assert_eq!(config.min_bucket_count(3), 5);
    }

    // RFC-002 Example A
    #[test]
    fn rfc_example_a() {
        let tokens: Vec<String> = vec!["rust", "is", "fast"]
            .into_iter()
            .map(String::from)
            .collect();

        let mut counter = NgramCounter::new();
        counter.process_comment("2024-01-01", &tokens);

        // Check counts
        let counts = counter.counts();

        // 1-grams
        assert_eq!(
            counts.get(&NgramKey {
                bucket: "2024-01-01".to_string(),
                n: 1,
                ngram: "rust".to_string()
            }),
            Some(&1)
        );
        assert_eq!(
            counts.get(&NgramKey {
                bucket: "2024-01-01".to_string(),
                n: 1,
                ngram: "is".to_string()
            }),
            Some(&1)
        );
        assert_eq!(
            counts.get(&NgramKey {
                bucket: "2024-01-01".to_string(),
                n: 1,
                ngram: "fast".to_string()
            }),
            Some(&1)
        );

        // 2-grams
        assert_eq!(
            counts.get(&NgramKey {
                bucket: "2024-01-01".to_string(),
                n: 2,
                ngram: "rust is".to_string()
            }),
            Some(&1)
        );
        assert_eq!(
            counts.get(&NgramKey {
                bucket: "2024-01-01".to_string(),
                n: 2,
                ngram: "is fast".to_string()
            }),
            Some(&1)
        );

        // 3-grams
        assert_eq!(
            counts.get(&NgramKey {
                bucket: "2024-01-01".to_string(),
                n: 3,
                ngram: "rust is fast".to_string()
            }),
            Some(&1)
        );

        // Check totals (denominators)
        let totals = counter.totals();
        assert_eq!(
            totals.get(&BucketKey {
                bucket: "2024-01-01".to_string(),
                n: 1
            }),
            Some(&3)
        );
        assert_eq!(
            totals.get(&BucketKey {
                bucket: "2024-01-01".to_string(),
                n: 2
            }),
            Some(&2)
        );
        assert_eq!(
            totals.get(&BucketKey {
                bucket: "2024-01-01".to_string(),
                n: 3
            }),
            Some(&1)
        );
    }

    // RFC-002 Example B
    #[test]
    fn rfc_example_b() {
        let tokens: Vec<String> = vec!["ai"].into_iter().map(String::from).collect();

        let mut counter = NgramCounter::new();
        counter.process_comment("2024-01-01", &tokens);

        let totals = counter.totals();
        assert_eq!(
            totals.get(&BucketKey {
                bucket: "2024-01-01".to_string(),
                n: 1
            }),
            Some(&1)
        );
        // No bigrams or trigrams from single token
        assert_eq!(
            totals.get(&BucketKey {
                bucket: "2024-01-01".to_string(),
                n: 2
            }),
            None
        );
        assert_eq!(
            totals.get(&BucketKey {
                bucket: "2024-01-01".to_string(),
                n: 3
            }),
            None
        );
    }

    // RFC-002 Example C
    #[test]
    fn rfc_example_c() {
        let tokens: Vec<String> = vec![];

        let mut counter = NgramCounter::new();
        counter.process_comment("2024-01-01", &tokens);

        assert!(counter.counts().is_empty());
        assert!(counter.totals().is_empty());
    }

    #[test]
    fn test_denominator_contribution() {
        // 4 tokens: 4 unigrams, 3 bigrams, 2 trigrams
        let contrib = DenominatorContribution::from_token_count(4);
        assert_eq!(contrib.unigrams, 4);
        assert_eq!(contrib.bigrams, 3);
        assert_eq!(contrib.trigrams, 2);

        // 2 tokens: 2 unigrams, 1 bigram, 0 trigrams
        let contrib = DenominatorContribution::from_token_count(2);
        assert_eq!(contrib.unigrams, 2);
        assert_eq!(contrib.bigrams, 1);
        assert_eq!(contrib.trigrams, 0);

        // 1 token: 1 unigram, 0 bigrams, 0 trigrams
        let contrib = DenominatorContribution::from_token_count(1);
        assert_eq!(contrib.unigrams, 1);
        assert_eq!(contrib.bigrams, 0);
        assert_eq!(contrib.trigrams, 0);
    }

    #[test]
    fn test_merge_counters() {
        let tokens1: Vec<String> = vec!["rust", "is", "fast"]
            .into_iter()
            .map(String::from)
            .collect();
        let tokens2: Vec<String> = vec!["rust", "is", "safe"]
            .into_iter()
            .map(String::from)
            .collect();

        let mut counter1 = NgramCounter::new();
        counter1.process_comment("2024-01-01", &tokens1);

        let mut counter2 = NgramCounter::new();
        counter2.process_comment("2024-01-01", &tokens2);

        counter1.merge(counter2);

        let counts = counter1.counts();

        // "rust" should appear twice
        assert_eq!(
            counts.get(&NgramKey {
                bucket: "2024-01-01".to_string(),
                n: 1,
                ngram: "rust".to_string()
            }),
            Some(&2)
        );

        // "rust is" should appear twice
        assert_eq!(
            counts.get(&NgramKey {
                bucket: "2024-01-01".to_string(),
                n: 2,
                ngram: "rust is".to_string()
            }),
            Some(&2)
        );

        // Totals should be summed
        let totals = counter1.totals();
        assert_eq!(
            totals.get(&BucketKey {
                bucket: "2024-01-01".to_string(),
                n: 1
            }),
            Some(&6)
        ); // 3 + 3
    }

    #[test]
    fn test_global_counts() {
        let mut counter = NgramCounter::new();

        // Same n-gram in different buckets
        let tokens: Vec<String> = vec!["machine", "learning"]
            .into_iter()
            .map(String::from)
            .collect();

        counter.process_comment("2024-01-01", &tokens);
        counter.process_comment("2024-01-02", &tokens);
        counter.process_comment("2024-01-03", &tokens);

        let global = counter.global_counts();

        // "machine learning" should have global count of 3
        assert_eq!(global.get(&(2, "machine learning".to_string())), Some(&3));
    }

    #[test]
    fn test_vocabulary_building() {
        let mut global_counts: HashMap<(u8, String), u64> = HashMap::new();

        // Unigrams (always admitted)
        global_counts.insert((1, "rust".to_string()), 5);

        // Bigrams
        global_counts.insert((2, "machine learning".to_string()), 25); // Above threshold
        global_counts.insert((2, "rare phrase".to_string()), 5); // Below threshold

        // Trigrams
        global_counts.insert((3, "large language model".to_string()), 15); // Above threshold
        global_counts.insert((3, "very rare trigram".to_string()), 3); // Below threshold

        let config = PruningConfig::default();
        let vocab = build_vocabulary(&global_counts, &config);

        // Check admissions
        assert!(vocab.contains_key(&(1, "rust".to_string())));
        assert!(vocab.contains_key(&(2, "machine learning".to_string())));
        assert!(!vocab.contains_key(&(2, "rare phrase".to_string())));
        assert!(vocab.contains_key(&(3, "large language model".to_string())));
        assert!(!vocab.contains_key(&(3, "very rare trigram".to_string())));
    }

    #[test]
    fn test_bucket_pruning() {
        let mut counter = NgramCounter::new();

        // Add counts that should be pruned vs kept
        let bucket = "2024-01-01";

        // Unigram with count 1 (kept)
        for _ in 0..1 {
            counter.process_comment(
                bucket,
                &["rare".to_string()]
                    .into_iter()
                    .collect::<Vec<_>>(),
            );
        }

        // Bigram with count 2 (pruned, threshold is 3)
        for _ in 0..2 {
            counter.process_comment(
                bucket,
                &["low", "count"].into_iter().map(String::from).collect::<Vec<_>>(),
            );
        }

        // Bigram with count 5 (kept)
        for _ in 0..5 {
            counter.process_comment(
                bucket,
                &["high", "count"].into_iter().map(String::from).collect::<Vec<_>>(),
            );
        }

        let config = PruningConfig::default();
        let pruned = counter.prune_bucket_counts(&config);

        // Unigram should be kept
        assert!(pruned.contains_key(&NgramKey {
            bucket: bucket.to_string(),
            n: 1,
            ngram: "rare".to_string()
        }));

        // Low count bigram should be pruned
        assert!(!pruned.contains_key(&NgramKey {
            bucket: bucket.to_string(),
            n: 2,
            ngram: "low count".to_string()
        }));

        // High count bigram should be kept
        assert!(pruned.contains_key(&NgramKey {
            bucket: bucket.to_string(),
            n: 2,
            ngram: "high count".to_string()
        }));
    }

    #[test]
    fn test_occurrence_counting() {
        // Per RFC-002: If a comment contains "ai" five times, that contributes five unigram occurrences
        let tokens: Vec<String> = vec!["ai", "ai", "ai", "ai", "ai"]
            .into_iter()
            .map(String::from)
            .collect();

        let mut counter = NgramCounter::new();
        counter.process_comment("2024-01-01", &tokens);

        let counts = counter.counts();
        assert_eq!(
            counts.get(&NgramKey {
                bucket: "2024-01-01".to_string(),
                n: 1,
                ngram: "ai".to_string()
            }),
            Some(&5)
        );

        // "ai ai" appears 4 times
        assert_eq!(
            counts.get(&NgramKey {
                bucket: "2024-01-01".to_string(),
                n: 2,
                ngram: "ai ai".to_string()
            }),
            Some(&4)
        );
    }
}
