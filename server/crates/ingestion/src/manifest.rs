//! Manifest file for idempotency tracking (RFC-004 §10).
//!
//! Tracks which files have been processed in each phase to allow
//! restartable pipeline execution.

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tokenizer::TOKENIZER_VERSION;

#[derive(Serialize, Deserialize, Default)]
pub struct Manifest {
    pub tokenizer_version: String,
    phase1_completed: Vec<String>,
    phase2_completed: Vec<String>,
    pub vocabulary_built: bool,
    #[serde(default)]
    ingest_completed: Vec<String>,

    /// Runtime lookup sets (not serialized).
    #[serde(skip)]
    phase1_set: HashSet<String>,
    #[serde(skip)]
    phase2_set: HashSet<String>,
    #[serde(skip)]
    ingest_set: HashSet<String>,
}

impl Manifest {
    fn manifest_path(data_dir: &Path) -> PathBuf {
        data_dir.join("manifest.json")
    }

    /// Load manifest from disk, or create a new one if it doesn't exist.
    pub fn load(data_dir: &Path) -> anyhow::Result<Self> {
        let path = Self::manifest_path(data_dir);
        if !path.exists() {
            return Ok(Self {
                tokenizer_version: TOKENIZER_VERSION.to_string(),
                ..Default::default()
            });
        }
        let data = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read manifest at {}", path.display()))?;
        let mut manifest: Self = serde_json::from_str(&data)
            .with_context(|| format!("Failed to parse manifest at {}", path.display()))?;
        // Build lookup sets from the serialized vecs
        manifest.phase1_set = manifest.phase1_completed.iter().cloned().collect();
        manifest.phase2_set = manifest.phase2_completed.iter().cloned().collect();
        manifest.ingest_set = manifest.ingest_completed.iter().cloned().collect();
        Ok(manifest)
    }

    /// Save manifest to disk atomically (write tmp, rename).
    pub fn save(&self, data_dir: &Path) -> anyhow::Result<()> {
        let path = Self::manifest_path(data_dir);
        let tmp_path = path.with_extension("json.tmp");
        let data = serde_json::to_string_pretty(self)?;
        std::fs::create_dir_all(data_dir)?;
        std::fs::write(&tmp_path, data)
            .with_context(|| format!("Failed to write {}", tmp_path.display()))?;
        std::fs::rename(&tmp_path, &path)
            .with_context(|| format!("Failed to rename to {}", path.display()))?;
        Ok(())
    }

    /// Validate that the manifest's tokenizer version matches the current one.
    /// Returns an error if they differ (requires full rebuild).
    pub fn validate_version(&self) -> anyhow::Result<()> {
        let current = TOKENIZER_VERSION.to_string();
        if !self.tokenizer_version.is_empty() && self.tokenizer_version != current {
            bail!(
                "Manifest tokenizer version '{}' does not match current '{}'. \
                 A tokenizer change invalidates all data. \
                 Delete the manifest file to start a fresh rebuild.",
                self.tokenizer_version,
                current
            );
        }
        Ok(())
    }

    pub fn is_phase1_done(&self, path: &str) -> bool {
        self.phase1_set.contains(path)
    }

    pub fn mark_phase1_done(&mut self, path: &str, data_dir: &Path) -> anyhow::Result<()> {
        if self.phase1_set.insert(path.to_string()) {
            self.phase1_completed.push(path.to_string());
            self.save(data_dir)?;
        }
        Ok(())
    }

    pub fn is_phase2_done(&self, path: &str) -> bool {
        self.phase2_set.contains(path)
    }

    pub fn mark_phase2_done(&mut self, path: &str, data_dir: &Path) -> anyhow::Result<()> {
        if self.phase2_set.insert(path.to_string()) {
            self.phase2_completed.push(path.to_string());
            self.save(data_dir)?;
        }
        Ok(())
    }

    pub fn mark_vocabulary_built(&mut self, data_dir: &Path) -> anyhow::Result<()> {
        self.vocabulary_built = true;
        self.save(data_dir)
    }

    pub fn phase1_count(&self) -> usize {
        self.phase1_completed.len()
    }

    pub fn phase2_count(&self) -> usize {
        self.phase2_completed.len()
    }

    pub fn is_ingest_done(&self, path: &str) -> bool {
        self.ingest_set.contains(path)
    }

    pub fn mark_ingest_done(&mut self, path: &str, data_dir: &Path) -> anyhow::Result<()> {
        if self.ingest_set.insert(path.to_string()) {
            self.ingest_completed.push(path.to_string());
            self.save(data_dir)?;
        }
        Ok(())
    }

    pub fn ingest_count(&self) -> usize {
        self.ingest_completed.len()
    }
}
