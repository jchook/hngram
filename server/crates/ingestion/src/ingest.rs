//! Single-pass ingestion: tokenize once, update vocabulary + insert counts.

use crate::backfill::parse_bucket_date;
use crate::manifest::Manifest;
use crate::months::YearMonth;
use crate::parquet;
use crate::vocabulary;
use hn_clickhouse::{BucketTotalRow, HnClickHouse, NgramCountRow, NgramVocabularyRow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use tokenizer::counter::{build_vocabulary, PruningConfig};
use tokenizer::TOKENIZER_VERSION;

/// Snapshot format version. Increment if the serialized format changes.
const SNAPSHOT_VERSION: u8 = 1;

#[derive(Serialize, Deserialize)]
struct CumulativeSnapshot {
    version: u8,
    counts: HashMap<(u8, String), u64>,
}

fn snapshot_path(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join("cumulative.bin")
}

fn load_cumulative_snapshot(data_dir: &Path) -> HashMap<(u8, String), u64> {
    let path = snapshot_path(data_dir);
    if !path.exists() {
        return HashMap::new();
    }
    match std::fs::read(&path) {
        Ok(data) => match bincode::deserialize::<CumulativeSnapshot>(&data) {
            Ok(snap) if snap.version == SNAPSHOT_VERSION => {
                tracing::info!("Loaded cumulative snapshot ({} entries)", snap.counts.len());
                snap.counts
            }
            Ok(snap) => {
                tracing::warn!(
                    "Snapshot version mismatch (got {}, expected {}), rebuilding",
                    snap.version,
                    SNAPSHOT_VERSION
                );
                HashMap::new()
            }
            Err(e) => {
                tracing::warn!("Failed to deserialize snapshot: {}, rebuilding", e);
                HashMap::new()
            }
        },
        Err(e) => {
            tracing::warn!("Failed to read snapshot: {}, rebuilding", e);
            HashMap::new()
        }
    }
}

fn save_cumulative_snapshot(
    data_dir: &Path,
    counts: &HashMap<(u8, String), u64>,
) -> anyhow::Result<()> {
    let snap = CumulativeSnapshot {
        version: SNAPSHOT_VERSION,
        counts: counts.clone(),
    };
    let data = bincode::serialize(&snap)?;
    let path = snapshot_path(data_dir);
    let tmp = path.with_extension("bin.tmp");
    std::fs::create_dir_all(data_dir)?;
    std::fs::write(&tmp, &data)?;
    std::fs::rename(&tmp, &path)?;
    tracing::info!("Saved cumulative snapshot ({} entries)", counts.len());
    Ok(())
}

/// Run single-pass ingestion: tokenize once, update vocabulary, insert counts.
pub async fn ingest(
    data_dir: &Path,
    months: &[YearMonth],
    manifest: &mut Manifest,
    ch: &HnClickHouse,
) -> anyhow::Result<()> {
    let config = PruningConfig::from_env();
    let tv = TOKENIZER_VERSION.to_string();
    let total = months.len();

    // Load cumulative global counts
    let mut global_counts = load_cumulative_snapshot(data_dir);

    // If snapshot was empty but we have partial TSVs, rebuild from them
    if global_counts.is_empty() {
        let existing_partials: Vec<&YearMonth> = months
            .iter()
            .filter(|ym| vocabulary::partial_path(data_dir, ym).exists())
            .collect();
        if !existing_partials.is_empty() {
            tracing::info!(
                "No snapshot found, rebuilding from {} partial files",
                existing_partials.len()
            );
            let all_months: Vec<YearMonth> = existing_partials.into_iter().cloned().collect();
            global_counts = vocabulary::merge_partial_counts(data_dir, &all_months)?;
        }
    }

    // Load current vocabulary from ClickHouse once
    let mut prev_vocab = ch.load_vocabulary().await.unwrap_or_default();
    tracing::info!("Current vocabulary: {} admitted n-grams", prev_vocab.len());

    let watermark = manifest.last_ingested_ts;
    let mut max_ts = watermark;
    let mut total_comments = 0u64;

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
            tokio::task::spawn_blocking(move || parquet::read_comments_after(&path, wm))
                .await??;

        if comments.is_empty() {
            continue;
        }

        tracing::info!("Ingesting: {} ({}/{}) — {} new comments", rel_path, i + 1, total, comments.len());
        let start = std::time::Instant::now();

        // Track max timestamp
        if let Some(ts) = comments.iter().map(|c| c.ts_ms).max() {
            max_ts = max_ts.max(ts);
        }
        let comment_count = comments.len();
        total_comments += comment_count as u64;

        let counter =
            tokio::task::spawn_blocking(move || parquet::process_comments_parallel(&comments))
                .await?;

        // Step 2: Update global counts
        let month_globals = counter.global_counts();
        for ((n, ngram), count) in &month_globals {
            *global_counts.entry((*n, ngram.clone())).or_insert(0) += count;
        }

        // Step 3: Write partial TSV (for debugging / recovery)
        let partial = vocabulary::partial_path(data_dir, ym);
        vocabulary::write_partial_counts(&partial, &month_globals)?;

        // Step 4: Rebuild vocabulary from updated global counts
        let mut vocabulary = build_vocabulary(&global_counts, &config);

        // Step 5: Merge with ClickHouse vocab (append-only) + delta insert
        let new_admission_keys: Vec<(u8, String)> = vocabulary
            .keys()
            .filter(|k| !prev_vocab.contains_key(k))
            .cloned()
            .collect();

        if !new_admission_keys.is_empty() {
            let now = time::OffsetDateTime::now_utc();
            let rows: Vec<NgramVocabularyRow> = new_admission_keys
                .iter()
                .map(|(n, ngram)| {
                    let gc = global_counts.get(&(*n, ngram.clone())).copied().unwrap_or(0);
                    NgramVocabularyRow {
                        tokenizer_version: tv.clone(),
                        n: *n,
                        ngram: ngram.clone(),
                        global_count: gc,
                        admitted_at: now,
                    }
                })
                .collect();

            tracing::info!("  New vocabulary admissions: {}", rows.len());
            ch.insert_vocabulary(&rows).await?;

            // Update local prev_vocab for subsequent months
            for key in &new_admission_keys {
                prev_vocab.insert(key.clone(), ());
            }
        }

        // Include all previously admitted n-grams in the filter set
        for (key, val) in &prev_vocab {
            vocabulary.entry(key.clone()).or_insert(val.clone());
        }

        // Step 6: Filter counts against updated vocabulary
        let filtered_counts = counter.filter_to_vocabulary(&vocabulary, &config);
        let totals = counter.totals();

        // Step 7: Convert and insert into ClickHouse
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

        let elapsed = start.elapsed();
        tracing::info!(
            "  Comments: {} | Counts: {} | Totals: {} | Vocab: +{} | Elapsed: {:.1}s",
            comment_count,
            count_rows.len(),
            total_rows.len(),
            new_admission_keys.len(),
            elapsed.as_secs_f64()
        );

    }

    // Persist state if any new comments were processed
    if max_ts > watermark {
        save_cumulative_snapshot(data_dir, &global_counts)?;
        manifest.set_last_ingested_ts(max_ts, data_dir)?;
        tracing::info!(
            "Ingestion complete — {} new comments, watermark advanced to {}",
            total_comments,
            max_ts
        );
    } else {
        tracing::info!("No new comments found");
    }

    Ok(())
}
