//! Phase 2: Backfill — daily aggregates with vocabulary filter (RFC-004 §11 pass 2).

use crate::manifest::Manifest;
use crate::months::YearMonth;
use crate::parquet;
use crate::vocabulary;
use anyhow::{bail, Context};
use hn_clickhouse::{BucketTotalRow, HnClickHouse, NgramCountRow};
use std::path::Path;
use time::macros::format_description;
use tokenizer::counter::PruningConfig;
use tokenizer::TOKENIZER_VERSION;

/// Parse a "YYYY-MM-DD" bucket string into a `time::Date`.
fn parse_bucket_date(s: &str) -> anyhow::Result<time::Date> {
    let format = format_description!("[year]-[month]-[day]");
    time::Date::parse(s, format).with_context(|| format!("Invalid bucket date: '{}'", s))
}

/// Run phase 2: generate daily aggregates and insert into ClickHouse.
pub async fn backfill_phase(
    data_dir: &Path,
    months: &[YearMonth],
    manifest: &mut Manifest,
    ch: &HnClickHouse,
) -> anyhow::Result<()> {
    if !manifest.vocabulary_built {
        bail!(
            "Vocabulary has not been built. Run `ingestion vocabulary` first."
        );
    }

    // Load vocabulary
    tracing::info!("Loading vocabulary...");
    let vocabulary = vocabulary::load_vocabulary(data_dir)?;
    let vocab_size = vocabulary.len();
    tracing::info!("  Loaded {} admitted n-grams", vocab_size);

    let config = PruningConfig::default();
    let total = months.len();
    let tv = TOKENIZER_VERSION.to_string();

    for (i, ym) in months.iter().enumerate() {
        let rel_path = ym.file_path();
        if manifest.is_phase2_done(&rel_path) {
            tracing::debug!("Skipping phase 2 for {} (already done)", rel_path);
            continue;
        }

        let local_path = data_dir.join(&rel_path);
        if !local_path.exists() {
            tracing::warn!("File not found: {} — skipping", local_path.display());
            continue;
        }

        tracing::info!("Phase 2: {} ({}/{})", rel_path, i + 1, total);
        let start = std::time::Instant::now();

        // Read and filter comments
        let path = local_path.clone();
        let comments = tokio::task::spawn_blocking(move || parquet::read_comments(&path))
            .await??;

        let comment_count = comments.len();

        // Tokenize + count in parallel
        let counter =
            tokio::task::spawn_blocking(move || parquet::process_comments_parallel(&comments))
                .await?;

        // Apply vocabulary filter + per-bucket pruning
        let filtered_counts = counter.filter_to_vocabulary(&vocabulary, &config);
        let totals = counter.totals();

        // Convert to ClickHouse rows
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

        // Batch insert into ClickHouse
        if !count_rows.is_empty() {
            ch.insert_ngram_counts(&count_rows).await?;
        }
        if !total_rows.is_empty() {
            ch.insert_bucket_totals(&total_rows).await?;
        }

        let elapsed = start.elapsed();
        tracing::info!(
            "  Comments: {} | Counts: {} rows | Totals: {} rows | Elapsed: {:.1}s",
            comment_count,
            count_rows.len(),
            total_rows.len(),
            elapsed.as_secs_f64()
        );

        manifest.mark_phase2_done(&rel_path, data_dir)?;
    }

    tracing::info!("Phase 2 complete — backfill done");
    Ok(())
}
