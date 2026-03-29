//! Ingestion processing: tokenize, count, prune, and output (RFC-004 §12).
//!
//! Two output modes:
//! - ClickHouse: incremental, watermark-based, direct DB insertion
//! - Parquet: full corpus from scratch, writes ClickHouse-compatible Parquet files

use crate::months::{parse_bucket_date, YearMonth};
use crate::parquet;
use crate::vocabulary;
use anyhow::Context;
use hn_clickhouse::{
    BucketTotalRow, GlobalCountRow, HnClickHouse, IngestionLogRow, NgramCountRow,
    NgramVocabularyRow,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokenizer::counter::{build_vocabulary, BucketKey, NgramKey, PruningConfig};
use tokenizer::TOKENIZER_VERSION;

/// Output mode for the process command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    ClickHouse,
    Parquet,
}

/// Configuration for the process command.
pub struct ProcessConfig {
    /// Flush threshold: flush when globals.len() + counts.len() exceeds this.
    /// This is a threshold, not a hard cap — accumulators retain capacity after flush.
    pub max_entries: usize,
    /// Number of concurrent file-processing workers.
    pub producer_count: usize,
    /// Optional path to TOML config file for pruning thresholds.
    pub config_path: Option<PathBuf>,
}

impl Default for ProcessConfig {
    fn default() -> Self {
        Self {
            max_entries: 20_000_000,
            producer_count: 2,
            config_path: None,
        }
    }
}

/// TOML file structure for ingestion configuration.
#[derive(Debug, Deserialize)]
struct IngestionConfigFile {
    pruning: Option<HashMap<String, PruningThresholdToml>>,
}

#[derive(Debug, Deserialize)]
struct PruningThresholdToml {
    min_global: Option<u64>,
    min_bucket: Option<u32>,
}

/// Load PruningConfig from TOML file if provided, otherwise from env vars.
/// Precedence: env vars > TOML file > hardcoded defaults.
fn load_pruning_config(config_path: &Option<PathBuf>) -> anyhow::Result<PruningConfig> {
    let mut config = match config_path {
        Some(path) => {
            let contents = std::fs::read_to_string(path)
                .with_context(|| format!("Failed to read config file {}", path.display()))?;
            let file: IngestionConfigFile = toml::from_str(&contents)
                .with_context(|| format!("Failed to parse config file {}", path.display()))?;

            let mut config = PruningConfig::default();
            if let Some(pruning) = file.pruning {
                for (key, thresh) in pruning {
                    let n: u8 = key.parse()
                        .with_context(|| format!("Invalid n-gram order '{}' in [pruning]", key))?;
                    config.set_threshold(n, thresh.min_global, thresh.min_bucket);
                }
            }
            tracing::info!("Loaded pruning config from {}", path.display());
            config
        }
        None => PruningConfig::default(),
    };
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

// ============================================================================
// Public entry point
// ============================================================================

pub async fn process(
    data_dir: &Path,
    months: &[YearMonth],
    start: &YearMonth,
    end: &YearMonth,
    mode: OutputMode,
    ch: Option<&HnClickHouse>,
    config: &ProcessConfig,
) -> anyhow::Result<()> {
    match mode {
        OutputMode::ClickHouse => {
            let ch = ch.expect("ClickHouse connection required for clickhouse output mode");
            process_clickhouse(data_dir, months, start, end, ch, config).await
        }
        OutputMode::Parquet => process_parquet(data_dir, months, start, end, config).await,
    }
}

// ============================================================================
// ClickHouse mode — incremental, watermark-based
// ============================================================================

async fn process_clickhouse(
    data_dir: &Path,
    months: &[YearMonth],
    start: &YearMonth,
    end: &YearMonth,
    ch: &HnClickHouse,
    proc_config: &ProcessConfig,
) -> anyhow::Result<()> {
    let config = load_pruning_config(&proc_config.config_path)?;
    let tv = TOKENIZER_VERSION.to_string();
    let total = months.len();
    let run_start = std::time::Instant::now();

    // Guard: check for data from a different tokenizer version
    if let Some(other) = ch.check_other_tokenizer_versions().await? {
        anyhow::bail!(
            "Database has ingestion data for tokenizer version '{}', but current version is '{}'. \
             A tokenizer change requires a full rebuild (process --output parquet + import).",
            other,
            tv
        );
    }

    // Load cumulative global counts from ClickHouse
    let mut global_counts = ch.load_global_counts().await.unwrap_or_default();
    tracing::info!("Loaded {} global count entries", global_counts.len());

    // Load current vocabulary from ClickHouse
    let mut prev_vocab = ch.load_vocabulary().await.unwrap_or_default();
    tracing::info!("Current vocabulary: {} admitted n-grams", prev_vocab.len());

    // Read watermark from ingestion_log
    let watermark = ch.get_latest_watermark().await?.unwrap_or(0);
    let mut max_ts = watermark;
    let mut total_comments = 0u64;
    let mut total_count_rows = 0u64;
    let mut total_total_rows = 0u64;
    let mut total_vocab_rows = 0u64;

    if watermark > 0 {
        let dt = time::OffsetDateTime::from_unix_timestamp_nanos((watermark as i128) * 1_000_000)
            .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
        tracing::info!("Watermark: {} ({})", watermark, dt.date());
    }

    for (i, ym) in months.iter().enumerate() {
        let rel_path = ym.file_path();
        let local_path = data_dir.join(&rel_path);
        if !local_path.exists() {
            continue;
        }

        // Read only comments after the watermark
        let path = local_path.clone();
        let wm = watermark;
        let comments =
            tokio::task::spawn_blocking(move || parquet::read_comments_after(&path, wm)).await??;

        if comments.is_empty() {
            continue;
        }

        tracing::info!(
            "Processing: {} ({}/{}) — {} new comments",
            rel_path,
            i + 1,
            total,
            comments.len()
        );
        let file_start = std::time::Instant::now();

        // Track max timestamp
        if let Some(ts) = comments.iter().map(|c| c.ts_ms).max() {
            max_ts = max_ts.max(ts);
        }
        let comment_count = comments.len();
        total_comments += comment_count as u64;

        let counter =
            tokio::task::spawn_blocking(move || parquet::process_comments_parallel(&comments))
                .await?;

        // Update global counts
        let month_globals = counter.global_counts();
        for ((n, ngram), count) in &month_globals {
            *global_counts.entry((*n, ngram.clone())).or_insert(0) += count;
        }

        // Rebuild vocabulary from updated global counts
        let vocabulary = build_vocabulary(&global_counts, &config);

        // Re-insert all vocabulary entries with updated global_count.
        // ReplacingMergeTree(admitted_at) keeps the latest version.
        // This covers both new admissions and updated counts for existing entries.
        let now = time::OffsetDateTime::now_utc();
        let mut new_admissions = 0u64;
        let mut vocab_rows: Vec<NgramVocabularyRow> = Vec::new();
        for ((n, ngram), _) in &vocabulary {
            let gc = global_counts.get(&(*n, ngram.clone())).copied().unwrap_or(0);
            if !prev_vocab.contains_key(&(*n, ngram.clone())) {
                new_admissions += 1;
            }
            vocab_rows.push(NgramVocabularyRow {
                tokenizer_version: tv.clone(),
                n: *n,
                ngram: ngram.clone(),
                global_count: gc,
                admitted_at: now,
            });
        }

        if !vocab_rows.is_empty() {
            if new_admissions > 0 {
                tracing::info!("  New vocabulary admissions: {}", new_admissions);
            }
            ch.insert_vocabulary(&vocab_rows).await?;
            total_vocab_rows += new_admissions;

            // Update local prev_vocab with new admissions
            for ((n, ngram), _) in &vocabulary {
                prev_vocab.entry((* n, ngram.clone())).or_insert(());
            }
        }

        // Merge all previously admitted n-grams into filter set
        let mut filter_vocab = vocabulary;
        for (key, val) in &prev_vocab {
            filter_vocab.entry(key.clone()).or_insert(val.clone());
        }

        // Filter counts against vocabulary
        let filtered_counts = counter.filter_to_vocabulary(&filter_vocab, &config);
        let totals = counter.totals();

        // Convert and insert
        let mut count_rows: Vec<NgramCountRow> = Vec::with_capacity(filtered_counts.len());
        for (key, count) in &filtered_counts {
            let bucket = parse_bucket_date(&key.bucket)?;
            count_rows.push(NgramCountRow {
                tokenizer_version: tv.clone(),
                n: key.n,
                ngram: key.ngram.clone(),
                bucket,
                count: *count,
            });
        }

        let mut total_rows: Vec<BucketTotalRow> = Vec::with_capacity(totals.len());
        for (key, &total_count) in totals {
            let bucket = parse_bucket_date(&key.bucket)?;
            total_rows.push(BucketTotalRow {
                tokenizer_version: tv.clone(),
                n: key.n,
                bucket,
                total_count,
            });
        }

        if !count_rows.is_empty() {
            ch.insert_ngram_counts(&count_rows).await?;
        }
        if !total_rows.is_empty() {
            ch.insert_bucket_totals(&total_rows).await?;
        }

        total_count_rows += count_rows.len() as u64;
        total_total_rows += total_rows.len() as u64;

        let elapsed = file_start.elapsed();
        tracing::info!(
            "  Comments: {} | Counts: {} | Totals: {} | Vocab: +{} | Elapsed: {:.1}s",
            comment_count,
            count_rows.len(),
            total_rows.len(),
            vocab_rows.len(),
            elapsed.as_secs_f64()
        );
    }

    // Persist state if any new comments were processed
    if max_ts > watermark {
        // Save updated global counts to ClickHouse (ReplacingMergeTree deduplicates)
        let gc_rows: Vec<GlobalCountRow> = global_counts
            .iter()
            .map(|((n, ngram), &count)| GlobalCountRow {
                tokenizer_version: tv.clone(),
                n: *n,
                ngram: ngram.clone(),
                count,
            })
            .collect();
        ch.insert_global_counts(&gc_rows).await?;
        tracing::info!("Saved {} global count entries to ClickHouse", gc_rows.len());

        let duration = run_start.elapsed();
        ch.insert_ingestion_log(&IngestionLogRow {
            tokenizer_version: tv,
            command: "process".to_string(),
            last_ingested_ts: max_ts,
            comments_processed: total_comments,
            ngram_counts_inserted: total_count_rows,
            bucket_totals_inserted: total_total_rows,
            vocabulary_inserted: total_vocab_rows,
            start_month: start.to_string(),
            end_month: end.to_string(),
            duration_ms: duration.as_millis() as u64,
        })
        .await?;

        tracing::info!(
            "Processing complete — {} new comments, watermark advanced to {}",
            total_comments,
            max_ts
        );
    } else {
        tracing::info!("No new comments found");
    }

    Ok(())
}

// ============================================================================
// Parquet mode — full corpus, from scratch, two-pass
// ============================================================================

async fn process_parquet(
    data_dir: &Path,
    months: &[YearMonth],
    start: &YearMonth,
    end: &YearMonth,
    proc_config: &ProcessConfig,
) -> anyhow::Result<()> {
    let config = load_pruning_config(&proc_config.config_path)?;
    let tv = TOKENIZER_VERSION.to_string();
    let total = months.len();
    let run_start = std::time::Instant::now();

    // Clean output directory — always from scratch
    let output_dir = data_dir.join("output");
    if output_dir.exists() {
        std::fs::remove_dir_all(&output_dir)?;
    }
    std::fs::create_dir_all(&output_dir)?;

    // ================================================================
    // Single pass: read source data once, emit three partial streams
    // Producers stream NgramCounter batches through a bounded channel.
    // Consumer accumulates globals + counts + totals, flushes all three
    // together when combined cardinality exceeds threshold.
    // ================================================================

    if vocabulary::is_pass_complete(data_dir) {
        tracing::info!("Source processing already complete (.complete marker found), skipping to merge");
    } else {
        tracing::info!(
            "Processing {} source files (max_entries={}, producers={})",
            total,
            proc_config.max_entries,
            proc_config.producer_count,
        );

        // Ensure partial directory exists (keep existing partials for resume)
        let p_dir = vocabulary::partial_dir(data_dir);
        std::fs::create_dir_all(&p_dir)?;

        // Load set of already-processed source files for resume
        let done_files = vocabulary::load_done_files(data_dir);

        // Bounded channel: capacity 1 for tight backpressure.
        use tokenizer::counter::NgramCounter;
        enum Msg {
            Batch(NgramCounter),
            FileDone(String), // rel_path of completed source file
        }
        // Channel capacity = producer_count so producers can stay busy while consumer merges
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Msg>(proc_config.producer_count * 2);

        // Spawn producers (bounded by semaphore)
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(proc_config.producer_count));

        let source_files: Vec<(usize, String, std::path::PathBuf)> = months
            .iter()
            .enumerate()
            .filter_map(|(i, ym)| {
                let rel_path = ym.file_path();
                if done_files.contains(&rel_path) {
                    return None; // Already processed in a previous run
                }
                let local_path = data_dir.join(&rel_path);
                if local_path.exists() {
                    Some((i, rel_path, local_path))
                } else {
                    None
                }
            })
            .collect();

        if !done_files.is_empty() {
            tracing::info!("Resuming — {} files already processed, {} remaining",
                done_files.len(), source_files.len());
        }

        let file_count = source_files.len();

        // Consumer must run concurrently with the producer-spawning loop.
        // Otherwise: producers fill the channel, block on send, hold semaphore
        // permits, and the spawning loop deadlocks waiting for a permit.
        let data_dir_owned = data_dir.to_path_buf();
        let max_entries = proc_config.max_entries;
        let consumer = tokio::spawn(async move {
            let data_dir = &data_dir_owned;
            let mut globals: std::collections::HashMap<(u8, String), u64> =
                std::collections::HashMap::new();
            let mut counts: std::collections::HashMap<NgramKey, u32> =
                std::collections::HashMap::new();
            let mut totals: std::collections::HashMap<BucketKey, u64> =
                std::collections::HashMap::new();

            let mut partial_counter = vocabulary::next_partial_counter(data_dir);
            let mut flush_count = 0u32;

            // Files completed since last flush (will be added to done set on flush)
            let mut pending_done: Vec<String> = Vec::new();

            while let Some(msg) = rx.recv().await {
                match msg {
                    Msg::Batch(batch_counter) => {
                        let batch_globals = batch_counter.global_counts();
                        for ((n, ngram), count) in batch_globals {
                            *globals.entry((n, ngram)).or_insert(0) += count;
                        }
                        for (key, &count) in batch_counter.counts() {
                            *counts.entry(key.clone()).or_insert(0) += count;
                        }
                        for (key, &total) in batch_counter.totals() {
                            *totals.entry(key.clone()).or_insert(0) += total;
                        }

                        // Check flush threshold: globals.len() + counts.len()
                        if globals.len() + counts.len() >= max_entries {
                            let g_path = vocabulary::numbered_partial_path(data_dir, partial_counter, "globals");
                            let c_path = vocabulary::numbered_partial_path(data_dir, partial_counter, "counts");
                            let t_path = vocabulary::numbered_partial_path(data_dir, partial_counter, "totals");
                            vocabulary::write_globals(&g_path, &globals)?;
                            vocabulary::write_counts(&c_path, &counts)?;
                            vocabulary::write_totals(&t_path, &totals)?;
                            // Mark pending files as durably flushed
                            vocabulary::append_done_files(data_dir, &pending_done)?;
                            tracing::info!(
                                "  Flushed partial {} (globals: {}, counts: {}, totals: {}, files done: +{})",
                                partial_counter, globals.len(), counts.len(), totals.len(), pending_done.len()
                            );
                            globals.clear();
                            counts.clear();
                            totals.clear();
                            pending_done.clear();
                            partial_counter += 1;
                            flush_count += 1;
                        }
                    }
                    Msg::FileDone(rel_path) => {
                        pending_done.push(rel_path);
                    }
                }
            }

            // Flush remaining
            if !globals.is_empty() || !counts.is_empty() {
                let g_path = vocabulary::numbered_partial_path(data_dir, partial_counter, "globals");
                let c_path = vocabulary::numbered_partial_path(data_dir, partial_counter, "counts");
                let t_path = vocabulary::numbered_partial_path(data_dir, partial_counter, "totals");
                vocabulary::write_globals(&g_path, &globals)?;
                vocabulary::write_counts(&c_path, &counts)?;
                vocabulary::write_totals(&t_path, &totals)?;
                vocabulary::append_done_files(data_dir, &pending_done)?;
                tracing::info!(
                    "  Flushed final partial {} (globals: {}, counts: {}, totals: {})",
                    partial_counter, globals.len(), counts.len(), totals.len()
                );
                flush_count += 1;
            }

            vocabulary::mark_pass_complete(data_dir)?;
            Ok::<(u32,), anyhow::Error>((flush_count,))
        });

        // Producer spawning loop — runs concurrently with consumer above
        for (i, rel_path, local_path) in source_files {
            let tx = tx.clone();

            // Acquire permit before spawning to guarantee FIFO start order
            let permit = sem.clone().acquire_owned().await.unwrap();

            tokio::spawn(async move {
                let _permit = permit;
                tracing::info!("Processing: {} ({}/{})", rel_path, i + 1, total);

                let path = local_path;
                let rel = rel_path.clone();
                let tx2 = tx.clone();
                let result = tokio::task::spawn_blocking(move || {
                    parquet::stream_counters(&path, 0, |counter| {
                        tx.blocking_send(Msg::Batch(counter))
                            .map_err(|_| anyhow::anyhow!("Consumer dropped"))
                    })
                })
                .await;

                match result {
                    Ok(Ok(())) => {
                        let _ = tx2.send(Msg::FileDone(rel)).await;
                    }
                    Ok(Err(e)) => tracing::error!("Producer failed for {}: {}", rel_path, e),
                    Err(e) => tracing::error!("Producer failed for {}: {}", rel_path, e),
                }
            });
        }
        drop(tx);

        // Wait for consumer to finish processing all messages
        let (flush_count,) = consumer.await??;
        tracing::info!(
            "Source processing complete — {} files, {} partial flushes",
            file_count,
            flush_count,
        );
    }

    // ================================================================
    // Merge 1: globals → vocabulary + global_counts.parquet
    // ================================================================
    tracing::info!("Merge 1: globals → vocabulary + global_counts.parquet");

    let gc_path = output_dir.join("global_counts.parquet");
    let mut gc_writer = parquet_writer::GlobalCountsWriter::new(&gc_path)?;

    let mut vocabulary: std::collections::HashMap<(u8, String), ()> =
        std::collections::HashMap::new();
    let mut vocab_counts: std::collections::HashMap<(u8, String), u64> =
        std::collections::HashMap::new();

    let mut total_unigrams = 0u64;
    let mut total_bigrams = 0u64;
    let mut total_trigrams = 0u64;
    let mut total_gc_rows = 0u64;

    vocabulary::merge_globals_streaming(data_dir, |entry| {
        gc_writer.write_one(&GlobalCountRow {
            tokenizer_version: tv.clone(),
            n: entry.n,
            ngram: entry.ngram.clone(),
            count: entry.count,
        })?;
        total_gc_rows += 1;

        match entry.n {
            1 => total_unigrams += 1,
            2 => total_bigrams += 1,
            3 => total_trigrams += 1,
            _ => {}
        }

        let min_global = config.min_global_count(entry.n);
        if entry.count >= min_global {
            vocabulary.insert((entry.n, entry.ngram.clone()), ());
            vocab_counts.insert((entry.n, entry.ngram), entry.count);
        }

        Ok(())
    })?;

    gc_writer.finish()?;

    let admitted_bigrams = vocabulary.iter().filter(|((n, _), _)| *n == 2).count();
    let admitted_trigrams = vocabulary.iter().filter(|((n, _), _)| *n == 3).count();

    tracing::info!(
        "Global counts: {} unigrams, {} bigram candidates, {} trigram candidates",
        total_unigrams, total_bigrams, total_trigrams,
    );
    tracing::info!(
        "Admitted: {} bigrams (of {}), {} trigrams (of {})",
        admitted_bigrams, total_bigrams, admitted_trigrams, total_trigrams,
    );
    tracing::info!("Wrote {} global count rows to parquet", total_gc_rows);

    // ================================================================
    // Merge 2: counts → filter by vocabulary → ngram_counts.parquet
    // ================================================================
    tracing::info!("Merge 2: counts → ngram_counts.parquet (filtered by vocabulary)");

    let counts_path = output_dir.join("ngram_counts.parquet");
    let mut counts_writer = parquet_writer::NgramCountsWriter::new(&counts_path)?;
    let mut total_count_rows = 0u64;
    let mut max_ts: i64 = 0;

    vocabulary::merge_counts_streaming(data_dir, |entry| {
        // Filter: unigrams always admitted, bigrams/trigrams need vocabulary check
        let dominated = if entry.n == 1 {
            entry.count >= config.min_bucket_count(1)
        } else {
            vocabulary.contains_key(&(entry.n, entry.ngram.clone()))
                && entry.count >= config.min_bucket_count(entry.n)
        };

        if dominated {
            let bucket = parse_bucket_date(&entry.bucket)?;
            counts_writer.write_batch(&[NgramCountRow {
                tokenizer_version: tv.clone(),
                n: entry.n,
                ngram: entry.ngram,
                bucket,
                count: entry.count,
            }])?;
            total_count_rows += 1;
        }

        Ok(())
    })?;

    counts_writer.finish()?;
    tracing::info!("Wrote {} ngram count rows to parquet", total_count_rows);

    // ================================================================
    // Merge 3: totals → bucket_totals.parquet (no filtering)
    // ================================================================
    tracing::info!("Merge 3: totals → bucket_totals.parquet");

    let totals_path = output_dir.join("bucket_totals.parquet");
    let mut totals_writer = parquet_writer::BucketTotalsWriter::new(&totals_path)?;
    let mut total_total_rows = 0u64;

    vocabulary::merge_totals_streaming(data_dir, |entry| {
        let bucket = parse_bucket_date(&entry.bucket)?;

        // Track max timestamp from bucket dates for watermark
        let epoch = time::Date::from_ordinal_date(1970, 1).unwrap();
        let days = (bucket - epoch).whole_days();
        let bucket_end_ms = (days + 1) * 86400 * 1000; // end of day
        max_ts = max_ts.max(bucket_end_ms);

        totals_writer.write_batch(&[BucketTotalRow {
            tokenizer_version: tv.clone(),
            n: entry.n,
            bucket,
            total_count: entry.total,
        }])?;
        total_total_rows += 1;

        Ok(())
    })?;

    totals_writer.finish()?;
    tracing::info!("Wrote {} bucket total rows to parquet", total_total_rows);

    // ================================================================
    // Write vocabulary + ingestion_log parquet
    // ================================================================

    let now = time::OffsetDateTime::now_utc();
    let vocab_rows: Vec<NgramVocabularyRow> = vocab_counts
        .iter()
        .map(|((n, ngram), &gc)| NgramVocabularyRow {
            tokenizer_version: tv.clone(),
            n: *n,
            ngram: ngram.clone(),
            global_count: gc,
            admitted_at: now,
        })
        .collect();

    let vocab_path = output_dir.join("ngram_vocabulary.parquet");
    parquet_writer::write_vocabulary_parquet(&vocab_path, &vocab_rows)?;
    tracing::info!("Wrote {} vocabulary rows", vocab_rows.len());

    let duration = run_start.elapsed();
    let log_row = IngestionLogRow {
        tokenizer_version: tv,
        command: "process".to_string(),
        last_ingested_ts: max_ts,
        comments_processed: 0, // not tracked in single-pass (would need source-level counting)
        ngram_counts_inserted: total_count_rows,
        bucket_totals_inserted: total_total_rows,
        vocabulary_inserted: vocab_rows.len() as u64,
        start_month: start.to_string(),
        end_month: end.to_string(),
        duration_ms: duration.as_millis() as u64,
    };

    let log_path = output_dir.join("ingestion_log.parquet");
    parquet_writer::write_ingestion_log_parquet(&log_path, &log_row)?;

    tracing::info!(
        "Processing complete — {} count rows, {} total rows, {:.1}s",
        total_count_rows,
        total_total_rows,
        duration.as_secs_f64()
    );
    tracing::info!("Output written to {}", output_dir.display());

    Ok(())
}

// ============================================================================
// Parquet writing helpers
// ============================================================================

mod parquet_writer {
    use super::*;
    use arrow::array::{
        Date32Builder, StringBuilder, TimestampMillisecondBuilder, UInt32Builder, UInt64Builder,
        UInt8Builder,
    };
    use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
    use arrow::record_batch::RecordBatch;
    use ::parquet::arrow::ArrowWriter;
    use ::parquet::basic::Compression;
    use ::parquet::file::properties::WriterProperties;
    use std::sync::Arc;

    fn writer_props() -> WriterProperties {
        WriterProperties::builder()
            .set_compression(Compression::ZSTD(Default::default()))
            .build()
    }

    /// Convert a time::Date to Date32 (days since 1970-01-01).
    fn date_to_days(d: time::Date) -> i32 {
        let epoch = time::Date::from_ordinal_date(1970, 1).unwrap();
        (d - epoch).whole_days() as i32
    }

    // ====================================================================
    // Streaming writers (for large tables written incrementally)
    // ====================================================================

    pub struct NgramCountsWriter {
        writer: ArrowWriter<std::fs::File>,
    }

    impl NgramCountsWriter {
        pub fn new(path: &Path) -> anyhow::Result<Self> {
            let schema = Arc::new(Schema::new(vec![
                Field::new("tokenizer_version", DataType::Utf8, false),
                Field::new("n", DataType::UInt8, false),
                Field::new("ngram", DataType::Utf8, false),
                Field::new("bucket", DataType::Date32, false),
                Field::new("count", DataType::UInt32, false),
            ]));
            let file = std::fs::File::create(path)
                .with_context(|| format!("Failed to create {}", path.display()))?;
            let writer = ArrowWriter::try_new(file, schema, Some(writer_props()))?;
            Ok(Self { writer })
        }

        pub fn write_batch(&mut self, rows: &[NgramCountRow]) -> anyhow::Result<()> {
            if rows.is_empty() {
                return Ok(());
            }

            let mut tv = StringBuilder::with_capacity(rows.len(), rows.len() * 2);
            let mut n = UInt8Builder::with_capacity(rows.len());
            let mut ngram = StringBuilder::with_capacity(rows.len(), rows.len() * 16);
            let mut bucket = Date32Builder::with_capacity(rows.len());
            let mut count = UInt32Builder::with_capacity(rows.len());

            for row in rows {
                tv.append_value(&row.tokenizer_version);
                n.append_value(row.n);
                ngram.append_value(&row.ngram);
                bucket.append_value(date_to_days(row.bucket));
                count.append_value(row.count);
            }

            let batch = RecordBatch::try_from_iter(vec![
                ("tokenizer_version", Arc::new(tv.finish()) as _),
                ("n", Arc::new(n.finish()) as _),
                ("ngram", Arc::new(ngram.finish()) as _),
                ("bucket", Arc::new(bucket.finish()) as _),
                ("count", Arc::new(count.finish()) as _),
            ])?;

            self.writer.write(&batch)?;
            Ok(())
        }

        pub fn finish(self) -> anyhow::Result<()> {
            self.writer.close()?;
            Ok(())
        }
    }

    pub struct BucketTotalsWriter {
        writer: ArrowWriter<std::fs::File>,
    }

    impl BucketTotalsWriter {
        pub fn new(path: &Path) -> anyhow::Result<Self> {
            let schema = Arc::new(Schema::new(vec![
                Field::new("tokenizer_version", DataType::Utf8, false),
                Field::new("n", DataType::UInt8, false),
                Field::new("bucket", DataType::Date32, false),
                Field::new("total_count", DataType::UInt64, false),
            ]));
            let file = std::fs::File::create(path)
                .with_context(|| format!("Failed to create {}", path.display()))?;
            let writer = ArrowWriter::try_new(file, schema, Some(writer_props()))?;
            Ok(Self { writer })
        }

        pub fn write_batch(&mut self, rows: &[BucketTotalRow]) -> anyhow::Result<()> {
            if rows.is_empty() {
                return Ok(());
            }

            let mut tv = StringBuilder::with_capacity(rows.len(), rows.len() * 2);
            let mut n = UInt8Builder::with_capacity(rows.len());
            let mut bucket = Date32Builder::with_capacity(rows.len());
            let mut total_count = UInt64Builder::with_capacity(rows.len());

            for row in rows {
                tv.append_value(&row.tokenizer_version);
                n.append_value(row.n);
                bucket.append_value(date_to_days(row.bucket));
                total_count.append_value(row.total_count);
            }

            let batch = RecordBatch::try_from_iter(vec![
                ("tokenizer_version", Arc::new(tv.finish()) as _),
                ("n", Arc::new(n.finish()) as _),
                ("bucket", Arc::new(bucket.finish()) as _),
                ("total_count", Arc::new(total_count.finish()) as _),
            ])?;

            self.writer.write(&batch)?;
            Ok(())
        }

        pub fn finish(self) -> anyhow::Result<()> {
            self.writer.close()?;
            Ok(())
        }
    }

    // ====================================================================
    // One-shot writers (for small tables written all at once)
    // ====================================================================

    pub fn write_vocabulary_parquet(
        path: &Path,
        rows: &[NgramVocabularyRow],
    ) -> anyhow::Result<()> {
        let schema = Arc::new(Schema::new(vec![
            Field::new("tokenizer_version", DataType::Utf8, false),
            Field::new("n", DataType::UInt8, false),
            Field::new("ngram", DataType::Utf8, false),
            Field::new("global_count", DataType::UInt64, false),
            Field::new(
                "admitted_at",
                DataType::Timestamp(TimeUnit::Millisecond, Some("UTC".into())),
                false,
            ),
        ]));

        let file = std::fs::File::create(path)
            .with_context(|| format!("Failed to create {}", path.display()))?;
        let mut writer = ArrowWriter::try_new(file, schema, Some(writer_props()))?;

        if !rows.is_empty() {
            let mut tv = StringBuilder::with_capacity(rows.len(), rows.len() * 2);
            let mut n = UInt8Builder::with_capacity(rows.len());
            let mut ngram = StringBuilder::with_capacity(rows.len(), rows.len() * 16);
            let mut global_count = UInt64Builder::with_capacity(rows.len());
            let mut admitted_at =
                TimestampMillisecondBuilder::with_capacity(rows.len()).with_timezone("UTC");

            for row in rows {
                tv.append_value(&row.tokenizer_version);
                n.append_value(row.n);
                ngram.append_value(&row.ngram);
                global_count.append_value(row.global_count);
                // Convert OffsetDateTime to millis since epoch
                let ms = (row.admitted_at.unix_timestamp_nanos() / 1_000_000) as i64;
                admitted_at.append_value(ms);
            }

            let batch = RecordBatch::try_from_iter(vec![
                ("tokenizer_version", Arc::new(tv.finish()) as _),
                ("n", Arc::new(n.finish()) as _),
                ("ngram", Arc::new(ngram.finish()) as _),
                ("global_count", Arc::new(global_count.finish()) as _),
                ("admitted_at", Arc::new(admitted_at.finish()) as _),
            ])?;

            writer.write(&batch)?;
        }

        writer.close()?;
        Ok(())
    }

    pub struct GlobalCountsWriter {
        writer: ArrowWriter<std::fs::File>,
        buf_tv: StringBuilder,
        buf_n: UInt8Builder,
        buf_ngram: StringBuilder,
        buf_count: UInt64Builder,
        buf_len: usize,
    }

    const GC_FLUSH_SIZE: usize = 65536;

    impl GlobalCountsWriter {
        pub fn new(path: &Path) -> anyhow::Result<Self> {
            let schema = Arc::new(Schema::new(vec![
                Field::new("tokenizer_version", DataType::Utf8, false),
                Field::new("n", DataType::UInt8, false),
                Field::new("ngram", DataType::Utf8, false),
                Field::new("count", DataType::UInt64, false),
            ]));
            let file = std::fs::File::create(path)
                .with_context(|| format!("Failed to create {}", path.display()))?;
            let writer = ArrowWriter::try_new(file, schema, Some(writer_props()))?;
            Ok(Self {
                writer,
                buf_tv: StringBuilder::new(),
                buf_n: UInt8Builder::new(),
                buf_ngram: StringBuilder::new(),
                buf_count: UInt64Builder::new(),
                buf_len: 0,
            })
        }

        pub fn write_one(&mut self, row: &GlobalCountRow) -> anyhow::Result<()> {
            self.buf_tv.append_value(&row.tokenizer_version);
            self.buf_n.append_value(row.n);
            self.buf_ngram.append_value(&row.ngram);
            self.buf_count.append_value(row.count);
            self.buf_len += 1;

            if self.buf_len >= GC_FLUSH_SIZE {
                self.flush_buf()?;
            }
            Ok(())
        }

        fn flush_buf(&mut self) -> anyhow::Result<()> {
            if self.buf_len == 0 {
                return Ok(());
            }
            let batch = RecordBatch::try_from_iter(vec![
                ("tokenizer_version", Arc::new(self.buf_tv.finish()) as _),
                ("n", Arc::new(self.buf_n.finish()) as _),
                ("ngram", Arc::new(self.buf_ngram.finish()) as _),
                ("count", Arc::new(self.buf_count.finish()) as _),
            ])?;
            self.writer.write(&batch)?;
            self.buf_len = 0;
            Ok(())
        }

        pub fn finish(mut self) -> anyhow::Result<()> {
            self.flush_buf()?;
            self.writer.close()?;
            Ok(())
        }
    }

    pub fn write_ingestion_log_parquet(
        path: &Path,
        row: &IngestionLogRow,
    ) -> anyhow::Result<()> {
        let schema = Arc::new(Schema::new(vec![
            Field::new("tokenizer_version", DataType::Utf8, false),
            Field::new("command", DataType::Utf8, false),
            Field::new("last_ingested_ts", DataType::Int64, false),
            Field::new("comments_processed", DataType::UInt64, false),
            Field::new("ngram_counts_inserted", DataType::UInt64, false),
            Field::new("bucket_totals_inserted", DataType::UInt64, false),
            Field::new("vocabulary_inserted", DataType::UInt64, false),
            Field::new("start_month", DataType::Utf8, false),
            Field::new("end_month", DataType::Utf8, false),
            Field::new("duration_ms", DataType::UInt64, false),
        ]));

        let file = std::fs::File::create(path)
            .with_context(|| format!("Failed to create {}", path.display()))?;
        let mut writer = ArrowWriter::try_new(file, schema, Some(writer_props()))?;

        use arrow::array::{Int64Builder, StringBuilder as SB, UInt64Builder as U64};
        let batch = RecordBatch::try_from_iter(vec![
            ("tokenizer_version", Arc::new({
                let mut b = SB::new();
                b.append_value(&row.tokenizer_version);
                b.finish()
            }) as _),
            ("command", Arc::new({
                let mut b = SB::new();
                b.append_value(&row.command);
                b.finish()
            }) as _),
            ("last_ingested_ts", Arc::new({
                let mut b = Int64Builder::new();
                b.append_value(row.last_ingested_ts);
                b.finish()
            }) as _),
            ("comments_processed", Arc::new({
                let mut b = U64::new();
                b.append_value(row.comments_processed);
                b.finish()
            }) as _),
            ("ngram_counts_inserted", Arc::new({
                let mut b = U64::new();
                b.append_value(row.ngram_counts_inserted);
                b.finish()
            }) as _),
            ("bucket_totals_inserted", Arc::new({
                let mut b = U64::new();
                b.append_value(row.bucket_totals_inserted);
                b.finish()
            }) as _),
            ("vocabulary_inserted", Arc::new({
                let mut b = U64::new();
                b.append_value(row.vocabulary_inserted);
                b.finish()
            }) as _),
            ("start_month", Arc::new({
                let mut b = SB::new();
                b.append_value(&row.start_month);
                b.finish()
            }) as _),
            ("end_month", Arc::new({
                let mut b = SB::new();
                b.append_value(&row.end_month);
                b.finish()
            }) as _),
            ("duration_ms", Arc::new({
                let mut b = U64::new();
                b.append_value(row.duration_ms);
                b.finish()
            }) as _),
        ])?;

        writer.write(&batch)?;
        writer.close()?;
        Ok(())
    }
}
