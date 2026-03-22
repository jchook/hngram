//! ClickHouse integration for HN N-gram
//!
//! Provides Rust types and functions that mirror the ClickHouse schema (RFC-003).
//! Used by both ingestion (writes) and API (reads).

use clickhouse::{Client, Row};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use thiserror::Error;
use time::{Date, OffsetDateTime};

/// Re-export tokenizer version for consumers
pub use tokenizer::TOKENIZER_VERSION;

/// Re-export time types for consumers
pub use time;

/// Database and table names
pub const DATABASE: &str = "hn_ngram";
pub const TABLE_NGRAM_COUNTS: &str = "ngram_counts";
pub const TABLE_BUCKET_TOTALS: &str = "bucket_totals";
pub const TABLE_NGRAM_VOCABULARY: &str = "ngram_vocabulary";

#[derive(Error, Debug)]
pub enum ClickHouseError {
    #[error("ClickHouse error: {0}")]
    Client(#[from] clickhouse::error::Error),

    #[error("Invalid n-gram order: {0} (must be 1, 2, or 3)")]
    InvalidNgramOrder(u8),

    #[error("Empty ngram list")]
    EmptyNgramList,
}

pub type Result<T> = std::result::Result<T, ClickHouseError>;

// ============================================================================
// Row Types (match schema exactly)
// ============================================================================

/// Row in `ngram_counts` table
#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct NgramCountRow {
    /// Tokenizer version as string (e.g., "1" for version 1)
    /// Using String allows future semantic versioning if needed
    pub tokenizer_version: String,
    pub n: u8,
    pub ngram: String,
    #[serde(with = "clickhouse::serde::time::date")]
    pub bucket: Date,
    pub count: u32,
}

/// Row in `bucket_totals` table
#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct BucketTotalRow {
    pub tokenizer_version: String,
    pub n: u8,
    #[serde(with = "clickhouse::serde::time::date")]
    pub bucket: Date,
    pub total_count: u64,
}

/// Row in `ngram_vocabulary` table
#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct NgramVocabularyRow {
    pub tokenizer_version: String,
    pub n: u8,
    pub ngram: String,
    pub global_count: u64,
    #[serde(with = "clickhouse::serde::time::datetime")]
    pub admitted_at: OffsetDateTime,
}

// ============================================================================
// Query Result Types
// ============================================================================

/// Result from the main n-gram query (RFC-003 Section 7)
#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct NgramQueryResult {
    #[serde(with = "clickhouse::serde::time::date")]
    pub bucket: Date,
    pub ngram: String,
    pub count: u32,
    pub total_count: u64,
}

/// Aggregated result with derived granularity (RFC-003 Section 8)
#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct AggregatedQueryResult {
    #[serde(with = "clickhouse::serde::time::date")]
    pub bucket: Date,
    pub ngram: String,
    pub sum_count: u64,
    pub sum_total: u64,
}

/// Result from batch vocabulary check
#[derive(Debug, Clone, Row, Serialize, Deserialize)]
struct VocabularyCheckRow {
    n: u8,
    ngram: String,
}

// ============================================================================
// Client Wrapper
// ============================================================================

/// ClickHouse client wrapper with HN N-gram specific operations
#[derive(Clone)]
pub struct HnClickHouse {
    client: Client,
    tokenizer_version: String,
}

impl HnClickHouse {
    /// Create a new client from environment variables or defaults
    pub fn from_env() -> Self {
        let host = std::env::var("CLICKHOUSE_HOST").unwrap_or_else(|_| "localhost".into());

        let port: u16 = match std::env::var("CLICKHOUSE_PORT") {
            Ok(p) => p.parse().unwrap_or_else(|_| {
                eprintln!(
                    "Warning: CLICKHOUSE_PORT '{}' is not a valid port, using default 8123",
                    p
                );
                8123
            }),
            Err(_) => 8123,
        };

        let user = std::env::var("CLICKHOUSE_USER").unwrap_or_else(|_| "default".into());
        let password = std::env::var("CLICKHOUSE_PASSWORD").unwrap_or_default();
        let database = std::env::var("CLICKHOUSE_DATABASE").unwrap_or_else(|_| DATABASE.into());

        let url = format!("http://{}:{}", host, port);

        let client = Client::default()
            .with_url(url)
            .with_user(user)
            .with_password(password)
            .with_database(database);

        Self {
            client,
            tokenizer_version: TOKENIZER_VERSION.to_string(),
        }
    }

    /// Create from explicit parameters
    pub fn new(url: &str, user: &str, password: &str, database: &str) -> Self {
        let client = Client::default()
            .with_url(url)
            .with_user(user)
            .with_password(password)
            .with_database(database);

        Self {
            client,
            tokenizer_version: TOKENIZER_VERSION.to_string(),
        }
    }

    /// Get the tokenizer version string
    pub fn tokenizer_version(&self) -> &str {
        &self.tokenizer_version
    }

    // ========================================================================
    // Insert Operations (for ingestion)
    // ========================================================================

    /// Insert n-gram counts in batch
    pub async fn insert_ngram_counts(&self, rows: &[NgramCountRow]) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }

        let mut insert = self.client.insert(TABLE_NGRAM_COUNTS)?;
        for row in rows {
            insert.write(row).await?;
        }
        insert.end().await?;
        Ok(())
    }

    /// Insert bucket totals in batch
    pub async fn insert_bucket_totals(&self, rows: &[BucketTotalRow]) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }

        let mut insert = self.client.insert(TABLE_BUCKET_TOTALS)?;
        for row in rows {
            insert.write(row).await?;
        }
        insert.end().await?;
        Ok(())
    }

    /// Insert vocabulary entries in batch
    pub async fn insert_vocabulary(&self, rows: &[NgramVocabularyRow]) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }

        let mut insert = self.client.insert(TABLE_NGRAM_VOCABULARY)?;
        for row in rows {
            insert.write(row).await?;
        }
        insert.end().await?;
        Ok(())
    }

    // ========================================================================
    // Query Operations (for API)
    // ========================================================================

    /// Query n-gram frequencies (RFC-003 Section 7)
    ///
    /// Returns raw counts and totals - caller computes relative frequency.
    pub async fn query_ngrams(
        &self,
        n: u8,
        ngrams: &[String],
        start: Date,
        end: Date,
    ) -> Result<Vec<NgramQueryResult>> {
        if n < 1 || n > 3 {
            return Err(ClickHouseError::InvalidNgramOrder(n));
        }
        if ngrams.is_empty() {
            return Err(ClickHouseError::EmptyNgramList);
        }

        // Use parameterized query with array binding to prevent SQL injection
        let query = format!(
            r#"
            SELECT
                c.bucket,
                c.ngram,
                c.count,
                t.total_count
            FROM {TABLE_NGRAM_COUNTS} c
            JOIN {TABLE_BUCKET_TOTALS} t
                ON c.bucket = t.bucket
               AND c.n = t.n
               AND c.tokenizer_version = t.tokenizer_version
            WHERE
                c.tokenizer_version = ?
                AND c.n = ?
                AND c.ngram IN ?
                AND c.bucket BETWEEN ? AND ?
            ORDER BY c.bucket
            "#
        );

        let results = self
            .client
            .query(&query)
            .bind(&self.tokenizer_version)
            .bind(n)
            .bind(ngrams)
            .bind(start)
            .bind(end)
            .fetch_all::<NgramQueryResult>()
            .await?;

        Ok(results)
    }

    /// Query with aggregation to week/month/year (RFC-003 Section 8)
    pub async fn query_ngrams_aggregated(
        &self,
        n: u8,
        ngrams: &[String],
        start: Date,
        end: Date,
        granularity: Granularity,
    ) -> Result<Vec<AggregatedQueryResult>> {
        if n < 1 || n > 3 {
            return Err(ClickHouseError::InvalidNgramOrder(n));
        }
        if ngrams.is_empty() {
            return Err(ClickHouseError::EmptyNgramList);
        }

        let bucket_fn = match granularity {
            Granularity::Day => "toDate(c.bucket)",
            Granularity::Week => "toStartOfWeek(c.bucket)",
            Granularity::Month => "toStartOfMonth(c.bucket)",
            Granularity::Year => "toStartOfYear(c.bucket)",
        };

        // Use parameterized query with array binding to prevent SQL injection
        let query = format!(
            r#"
            SELECT
                {bucket_fn} AS bucket,
                c.ngram,
                sum(c.count) AS sum_count,
                sum(t.total_count) AS sum_total
            FROM {TABLE_NGRAM_COUNTS} c
            JOIN {TABLE_BUCKET_TOTALS} t
                ON c.bucket = t.bucket
               AND c.n = t.n
               AND c.tokenizer_version = t.tokenizer_version
            WHERE
                c.tokenizer_version = ?
                AND c.n = ?
                AND c.ngram IN ?
                AND c.bucket BETWEEN ? AND ?
            GROUP BY bucket, c.ngram
            ORDER BY bucket
            "#
        );

        let results = self
            .client
            .query(&query)
            .bind(&self.tokenizer_version)
            .bind(n)
            .bind(ngrams)
            .bind(start)
            .bind(end)
            .fetch_all::<AggregatedQueryResult>()
            .await?;

        Ok(results)
    }

    /// Check if an n-gram exists in vocabulary
    pub async fn is_in_vocabulary(&self, n: u8, ngram: &str) -> Result<bool> {
        let query = format!(
            r#"
            SELECT count() > 0
            FROM {TABLE_NGRAM_VOCABULARY}
            WHERE tokenizer_version = ? AND n = ? AND ngram = ?
            "#
        );

        let exists: u8 = self
            .client
            .query(&query)
            .bind(&self.tokenizer_version)
            .bind(n)
            .bind(ngram)
            .fetch_one()
            .await?;

        Ok(exists > 0)
    }

    /// Check vocabulary status for multiple n-grams in a single query
    ///
    /// Returns a Vec<bool> in the same order as the input, indicating
    /// whether each (n, ngram) pair exists in the vocabulary.
    pub async fn check_vocabulary(&self, ngrams: &[(u8, String)]) -> Result<Vec<bool>> {
        if ngrams.is_empty() {
            return Ok(vec![]);
        }

        // Group by n to minimize queries (one per distinct n value)
        let mut by_n: std::collections::HashMap<u8, Vec<&str>> = std::collections::HashMap::new();
        for (n, ngram) in ngrams {
            by_n.entry(*n).or_default().push(ngram.as_str());
        }

        // Fetch all existing (n, ngram) pairs
        let mut existing: HashSet<(u8, String)> = HashSet::new();

        for (n, ngram_list) in by_n {
            let query = format!(
                r#"
                SELECT n, ngram
                FROM {TABLE_NGRAM_VOCABULARY}
                WHERE tokenizer_version = ? AND n = ? AND ngram IN ?
                "#
            );

            let rows: Vec<VocabularyCheckRow> = self
                .client
                .query(&query)
                .bind(&self.tokenizer_version)
                .bind(n)
                .bind(&ngram_list)
                .fetch_all()
                .await?;

            for row in rows {
                existing.insert((row.n, row.ngram));
            }
        }

        // Return results in input order
        Ok(ngrams
            .iter()
            .map(|(n, ngram)| existing.contains(&(*n, ngram.clone())))
            .collect())
    }
}

// ============================================================================
// Supporting Types
// ============================================================================

/// Time granularity for aggregated queries
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Granularity {
    Day,
    Week,
    Month,
    Year,
}

impl Granularity {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "day" | "daily" => Some(Self::Day),
            "week" | "weekly" => Some(Self::Week),
            "month" | "monthly" => Some(Self::Month),
            "year" | "yearly" => Some(Self::Year),
            _ => None,
        }
    }
}

impl Default for Granularity {
    fn default() -> Self {
        Self::Month
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn granularity_from_str() {
        assert_eq!(Granularity::from_str("day"), Some(Granularity::Day));
        assert_eq!(Granularity::from_str("weekly"), Some(Granularity::Week));
        assert_eq!(Granularity::from_str("Month"), Some(Granularity::Month));
        assert_eq!(Granularity::from_str("YEAR"), Some(Granularity::Year));
        assert_eq!(Granularity::from_str("invalid"), None);
    }

    #[test]
    fn tokenizer_version_is_string() {
        // Tokenizer version is u8 internally but converted to string for schema
        // This allows future migration to semantic versioning if needed
        let version = TOKENIZER_VERSION.to_string();
        assert_eq!(version, "1");
    }
}
