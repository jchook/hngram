//! Phase 1: Vocabulary build — partial counts, merge, admit (RFC-004 §11 pass 1).

use crate::manifest::Manifest;
use crate::months::YearMonth;
use crate::parquet;
use anyhow::Context;
use hn_clickhouse::{HnClickHouse, NgramVocabularyRow};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use tokenizer::counter::{build_vocabulary, PruningConfig};
use tokenizer::TOKENIZER_VERSION;

/// Directory for partial count files.
fn partial_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("partial")
}

/// Path for a partial count file.
fn partial_path(data_dir: &Path, ym: &YearMonth) -> PathBuf {
    partial_dir(data_dir).join(format!("{}.counts", ym))
}

/// Path for the vocabulary file.
fn vocabulary_path(data_dir: &Path) -> PathBuf {
    data_dir.join("vocabulary.json")
}

/// Write partial global counts to a TSV file: n\tngram\tcount\n
fn write_partial_counts(
    path: &Path,
    counts: &HashMap<(u8, String), u64>,
) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("counts.tmp");
    let mut file = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
    for ((n, ngram), count) in counts {
        writeln!(file, "{}\t{}\t{}", n, ngram, count)?;
    }
    file.flush()?;
    drop(file);
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Read and merge all partial count files into global counts.
fn merge_partial_counts(
    data_dir: &Path,
    months: &[YearMonth],
) -> anyhow::Result<HashMap<(u8, String), u64>> {
    let mut global: HashMap<(u8, String), u64> = HashMap::new();

    for ym in months {
        let path = partial_path(data_dir, ym);
        if !path.exists() {
            continue;
        }

        let file = std::io::BufReader::new(
            std::fs::File::open(&path)
                .with_context(|| format!("Failed to open {}", path.display()))?,
        );

        for line in file.lines() {
            let line = line?;
            let parts: Vec<&str> = line.splitn(3, '\t').collect();
            if parts.len() != 3 {
                continue;
            }
            let n: u8 = parts[0].parse().unwrap_or(0);
            let ngram = parts[1].to_string();
            let count: u64 = parts[2].parse().unwrap_or(0);
            if n >= 1 && n <= 3 && count > 0 {
                *global.entry((n, ngram)).or_insert(0) += count;
            }
        }
    }

    Ok(global)
}

/// Run phase 1: build vocabulary from global n-gram counts.
pub async fn build_vocabulary_phase(
    data_dir: &Path,
    months: &[YearMonth],
    manifest: &mut Manifest,
    ch: &HnClickHouse,
) -> anyhow::Result<()> {
    if manifest.vocabulary_built {
        tracing::info!("Vocabulary already built — skipping (delete manifest to rebuild)");
        return Ok(());
    }

    let total = months.len();
    let config = PruningConfig::from_env();

    // Step 1: Process each file to produce partial count files
    // Collect pending months (skip already-done and missing files)
    let pending: Vec<(usize, &YearMonth)> = months
        .iter()
        .enumerate()
        .filter(|(_, ym)| {
            let rel_path = ym.file_path();
            if manifest.is_phase1_done(&rel_path) {
                tracing::debug!("Skipping phase 1 for {} (already done)", rel_path);
                return false;
            }
            let local_path = data_dir.join(&rel_path);
            if !local_path.exists() {
                tracing::warn!("File not found: {} — skipping", local_path.display());
                return false;
            }
            true
        })
        .collect();

    let pending_total = pending.len();
    tracing::info!("{} of {} months need processing", pending_total, total);

    // Process months concurrently with bounded parallelism
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4),
    ));

    let mut handles = Vec::with_capacity(pending_total);

    for (i, ym) in pending {
        let rel_path = ym.file_path();
        let local_path = data_dir.join(&rel_path);
        let partial = partial_path(data_dir, ym);
        let sem = semaphore.clone();

        let handle = tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();
            tracing::info!("Phase 1: {} ({}/{})", rel_path, i + 1, pending_total);
            let start = std::time::Instant::now();

            let path = local_path.clone();
            let comments = tokio::task::spawn_blocking(move || parquet::read_comments(&path))
                .await??;

            let comment_count = comments.len();

            let counter =
                tokio::task::spawn_blocking(move || parquet::process_comments_parallel(&comments))
                    .await?;

            let global_counts = counter.global_counts();
            write_partial_counts(&partial, &global_counts)?;

            let elapsed = start.elapsed();
            tracing::info!(
                "  {} — Comments: {} | Unique n-grams: {} | Elapsed: {:.1}s",
                rel_path,
                comment_count,
                global_counts.len(),
                elapsed.as_secs_f64()
            );

            Ok::<String, anyhow::Error>(rel_path)
        });

        handles.push(handle);
    }

    // Collect results and update manifest
    for handle in handles {
        let rel_path = handle.await??;
        manifest.mark_phase1_done(&rel_path, data_dir)?;
    }

    // Step 2: Merge all partial counts
    tracing::info!("Merging partial counts...");
    let merge_start = std::time::Instant::now();
    let global_counts = merge_partial_counts(data_dir, months)?;

    let total_unigrams = global_counts
        .iter()
        .filter(|((n, _), _)| *n == 1)
        .count();
    let total_bigram_candidates = global_counts
        .iter()
        .filter(|((n, _), _)| *n == 2)
        .count();
    let total_trigram_candidates = global_counts
        .iter()
        .filter(|((n, _), _)| *n == 3)
        .count();

    tracing::info!(
        "  Global counts: {} unigrams, {} bigram candidates, {} trigram candidates",
        total_unigrams,
        total_bigram_candidates,
        total_trigram_candidates,
    );

    // Step 3: Build vocabulary
    let vocabulary = build_vocabulary(&global_counts, &config);

    let admitted_bigrams = vocabulary.iter().filter(|((n, _), _)| *n == 2).count();
    let admitted_trigrams = vocabulary.iter().filter(|((n, _), _)| *n == 3).count();

    tracing::info!(
        "  Admitted: {} bigrams (of {}), {} trigrams (of {})",
        admitted_bigrams,
        total_bigram_candidates,
        admitted_trigrams,
        total_trigram_candidates,
    );
    tracing::info!("  Merge elapsed: {:.1}s", merge_start.elapsed().as_secs_f64());

    // Step 4: Write vocabulary to JSON
    let vocab_entries: Vec<(u8, String, u64)> = vocabulary
        .keys()
        .map(|(n, ngram)| {
            let count = global_counts.get(&(*n, ngram.clone())).copied().unwrap_or(0);
            (*n, ngram.clone(), count)
        })
        .collect();

    let vocab_json = serde_json::to_string_pretty(&vocab_entries)?;
    std::fs::write(vocabulary_path(data_dir), &vocab_json)?;
    tracing::info!(
        "Vocabulary written to {}",
        vocabulary_path(data_dir).display()
    );

    // Step 5: Insert vocabulary into ClickHouse
    let now = time::OffsetDateTime::now_utc();
    let rows: Vec<NgramVocabularyRow> = vocab_entries
        .iter()
        .map(|(n, ngram, global_count)| NgramVocabularyRow {
            tokenizer_version: TOKENIZER_VERSION.to_string(),
            n: *n,
            ngram: ngram.clone(),
            global_count: *global_count,
            admitted_at: now,
        })
        .collect();

    if !rows.is_empty() {
        tracing::info!("Inserting {} vocabulary rows into ClickHouse...", rows.len());
        ch.insert_vocabulary(&rows).await?;
    }

    manifest.mark_vocabulary_built(data_dir)?;
    tracing::info!("Phase 1 complete — vocabulary built");

    Ok(())
}

/// Load admitted vocabulary from JSON file.
pub fn load_vocabulary(data_dir: &Path) -> anyhow::Result<HashMap<(u8, String), ()>> {
    let path = vocabulary_path(data_dir);
    let data = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read vocabulary at {}", path.display()))?;
    let entries: Vec<(u8, String, u64)> = serde_json::from_str(&data)?;
    Ok(entries.into_iter().map(|(n, ngram, _)| ((n, ngram), ())).collect())
}
