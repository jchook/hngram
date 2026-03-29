//! TOML configuration for the ingestion pipeline.
//!
//! Config file structure is namespaced by subcommand:
//!
//! ```toml
//! [process]
//! output = "parquet"
//! max_entries = 20000000
//! producer_count = 2
//!
//! [process.prune.2]
//! min_global = 500
//! min_bucket = 10
//! ```
//!
//! Precedence: CLI arg > TOML file > hardcoded default.

use anyhow::Context;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use tokenizer::counter::PruningConfig;

// ============================================================================
// TOML serde types
// ============================================================================

/// Top-level config file, namespaced by subcommand.
#[derive(Debug, Default, Deserialize)]
pub struct IngestionConfig {
    pub process: Option<ProcessSection>,
}

/// `[process]` section.
#[derive(Debug, Default, Deserialize)]
pub struct ProcessSection {
    pub start: Option<String>,
    pub end: Option<String>,
    pub output: Option<String>,
    pub max_entries: Option<usize>,
    pub producer_count: Option<usize>,
    pub merge_shards: Option<usize>,
    pub prune: Option<HashMap<String, PruneThreshold>>,
}

/// `[process.prune.N]` — per-n-gram-order thresholds.
#[derive(Debug, Deserialize)]
pub struct PruneThreshold {
    pub min_global: Option<u64>,
    pub min_bucket: Option<u32>,
}

// ============================================================================
// Loading
// ============================================================================

/// Parse an ingestion TOML config file.
pub fn load(path: &Path) -> anyhow::Result<IngestionConfig> {
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file {}", path.display()))?;
    let config: IngestionConfig = toml::from_str(&contents)
        .with_context(|| format!("Failed to parse config file {}", path.display()))?;
    Ok(config)
}

// ============================================================================
// Resolved config values
// ============================================================================

/// Build a `PruningConfig` from the `[process.prune]` section + env var overlay.
pub fn resolve_pruning(prune: &Option<HashMap<String, PruneThreshold>>) -> anyhow::Result<PruningConfig> {
    let mut config = PruningConfig::default();
    if let Some(prune) = prune {
        for (key, thresh) in prune {
            let n: u8 = key.parse()
                .with_context(|| format!("Invalid n-gram order '{}' in [process.prune]", key))?;
            config.set_threshold(n, thresh.min_global, thresh.min_bucket);
        }
    }
    // Env vars always overlay
    config.apply_env();
    tracing::info!(
        "Pruning thresholds — 1gram: global={}, bucket={} | 2gram: global={}, bucket={} | 3gram: global={}, bucket={}",
        config.min_global_count(1), config.min_bucket_count(1),
        config.min_global_count(2), config.min_bucket_count(2),
        config.min_global_count(3), config.min_bucket_count(3),
    );
    Ok(config)
}
