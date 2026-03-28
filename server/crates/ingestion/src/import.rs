//! Import: load Parquet output files into ClickHouse with atomic table swap (RFC-004 §10).
//!
//! Reads the four Parquet files from {data-dir}/output/, loads them into staging
//! tables, then atomically swaps each data table. The ingestion_log entry is
//! appended (not swapped) to preserve audit history.

use anyhow::{bail, Context};
use arrow::array::{Array, Int64Array, StringArray, UInt32Array, UInt64Array, UInt8Array};
use hn_clickhouse::{
    BucketTotalRow, GlobalCountRow, HnClickHouse, IngestionLogRow, NgramCountRow,
    NgramVocabularyRow, TABLE_BUCKET_TOTALS, TABLE_GLOBAL_COUNTS, TABLE_NGRAM_COUNTS,
    TABLE_NGRAM_VOCABULARY,
};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use std::path::Path;
use time::OffsetDateTime;

/// The data tables that get swapped during import.
const DATA_TABLES: [&str; 4] = [
    TABLE_NGRAM_COUNTS,
    TABLE_BUCKET_TOTALS,
    TABLE_NGRAM_VOCABULARY,
    TABLE_GLOBAL_COUNTS,
];

/// Run the import: load Parquet output into staging tables, swap, append log.
pub async fn import(data_dir: &Path, ch: &HnClickHouse) -> anyhow::Result<()> {
    let output_dir = data_dir.join("output");
    let run_start = std::time::Instant::now();

    // Validate all output files exist
    let counts_path = output_dir.join("ngram_counts.parquet");
    let totals_path = output_dir.join("bucket_totals.parquet");
    let vocab_path = output_dir.join("ngram_vocabulary.parquet");
    let gc_path = output_dir.join("global_counts.parquet");
    let log_path = output_dir.join("ingestion_log.parquet");

    for path in [&counts_path, &totals_path, &vocab_path, &gc_path, &log_path] {
        if !path.exists() {
            bail!("Missing output file: {}", path.display());
        }
    }

    // Create staging tables
    tracing::info!("Creating staging tables...");
    for table in &DATA_TABLES {
        ch.create_staging_table(table).await?;
    }

    // Load data into staging tables (streamed batch by batch from Parquet)
    tracing::info!("Loading ngram_counts.parquet...");
    let staging = format!("{}_staging", TABLE_NGRAM_COUNTS);
    let count = stream_ngram_counts(&counts_path, &staging, ch).await?;
    tracing::info!("  {} rows loaded", count);

    tracing::info!("Loading bucket_totals.parquet...");
    let staging = format!("{}_staging", TABLE_BUCKET_TOTALS);
    let count = stream_bucket_totals(&totals_path, &staging, ch).await?;
    tracing::info!("  {} rows loaded", count);

    tracing::info!("Loading ngram_vocabulary.parquet...");
    let staging = format!("{}_staging", TABLE_NGRAM_VOCABULARY);
    let count = stream_vocabulary(&vocab_path, &staging, ch).await?;
    tracing::info!("  {} rows loaded", count);

    tracing::info!("Loading global_counts.parquet...");
    let staging = format!("{}_staging", TABLE_GLOBAL_COUNTS);
    let count = stream_global_counts(&gc_path, &staging, ch).await?;
    tracing::info!("  {} rows loaded", count);

    // Atomic swap for each data table
    tracing::info!("Swapping tables...");
    for table in &DATA_TABLES {
        ch.exchange_tables(table).await?;
        tracing::info!("  Swapped {}", table);
    }

    // Drop old staging tables
    for table in &DATA_TABLES {
        ch.drop_staging_table(table).await?;
    }

    // Append ingestion_log entry (not swapped — preserves history)
    tracing::info!("Loading ingestion_log.parquet...");
    let log_row = read_ingestion_log(&log_path)?;
    ch.insert_ingestion_log(&log_row).await?;
    tracing::info!(
        "  Watermark: {} | Duration: {:.1}s",
        log_row.last_ingested_ts,
        run_start.elapsed().as_secs_f64()
    );

    tracing::info!("Import complete");
    Ok(())
}

// ============================================================================
// Streaming Parquet → ClickHouse loaders (batch by batch, bounded memory)
// ============================================================================

async fn stream_ngram_counts(
    path: &Path,
    table: &str,
    ch: &HnClickHouse,
) -> anyhow::Result<u64> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open {}", path.display()))?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)?.build()?;
    let mut total = 0u64;

    for batch_result in reader {
        let batch = batch_result?;
        let tv_col = batch.column_by_name("tokenizer_version").and_then(|c| c.as_any().downcast_ref::<StringArray>()).context("Missing tokenizer_version")?;
        let n_col = batch.column_by_name("n").and_then(|c| c.as_any().downcast_ref::<UInt8Array>()).context("Missing n")?;
        let ngram_col = batch.column_by_name("ngram").and_then(|c| c.as_any().downcast_ref::<StringArray>()).context("Missing ngram")?;
        let bucket_col = batch.column_by_name("bucket").and_then(|c| c.as_any().downcast_ref::<arrow::array::Date32Array>()).context("Missing bucket")?;
        let count_col = batch.column_by_name("count").and_then(|c| c.as_any().downcast_ref::<UInt32Array>()).context("Missing count")?;

        let mut rows = Vec::with_capacity(batch.num_rows());
        for i in 0..batch.num_rows() {
            rows.push(NgramCountRow {
                tokenizer_version: tv_col.value(i).to_string(),
                n: n_col.value(i),
                ngram: ngram_col.value(i).to_string(),
                bucket: days_to_date(bucket_col.value(i))?,
                count: count_col.value(i),
            });
        }
        ch.insert_ngram_counts_to(table, &rows).await?;
        total += rows.len() as u64;
    }
    Ok(total)
}

async fn stream_bucket_totals(
    path: &Path,
    table: &str,
    ch: &HnClickHouse,
) -> anyhow::Result<u64> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open {}", path.display()))?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)?.build()?;
    let mut total = 0u64;

    for batch_result in reader {
        let batch = batch_result?;
        let tv_col = batch.column_by_name("tokenizer_version").and_then(|c| c.as_any().downcast_ref::<StringArray>()).context("Missing tokenizer_version")?;
        let n_col = batch.column_by_name("n").and_then(|c| c.as_any().downcast_ref::<UInt8Array>()).context("Missing n")?;
        let bucket_col = batch.column_by_name("bucket").and_then(|c| c.as_any().downcast_ref::<arrow::array::Date32Array>()).context("Missing bucket")?;
        let total_col = batch.column_by_name("total_count").and_then(|c| c.as_any().downcast_ref::<UInt64Array>()).context("Missing total_count")?;

        let mut rows = Vec::with_capacity(batch.num_rows());
        for i in 0..batch.num_rows() {
            rows.push(BucketTotalRow {
                tokenizer_version: tv_col.value(i).to_string(),
                n: n_col.value(i),
                bucket: days_to_date(bucket_col.value(i))?,
                total_count: total_col.value(i),
            });
        }
        ch.insert_bucket_totals_to(table, &rows).await?;
        total += rows.len() as u64;
    }
    Ok(total)
}

async fn stream_vocabulary(
    path: &Path,
    table: &str,
    ch: &HnClickHouse,
) -> anyhow::Result<u64> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open {}", path.display()))?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)?.build()?;
    let mut total = 0u64;

    for batch_result in reader {
        let batch = batch_result?;
        let tv_col = batch.column_by_name("tokenizer_version").and_then(|c| c.as_any().downcast_ref::<StringArray>()).context("Missing tokenizer_version")?;
        let n_col = batch.column_by_name("n").and_then(|c| c.as_any().downcast_ref::<UInt8Array>()).context("Missing n")?;
        let ngram_col = batch.column_by_name("ngram").and_then(|c| c.as_any().downcast_ref::<StringArray>()).context("Missing ngram")?;
        let gc_col = batch.column_by_name("global_count").and_then(|c| c.as_any().downcast_ref::<UInt64Array>()).context("Missing global_count")?;
        let admitted_col = batch.column_by_name("admitted_at").and_then(|c| c.as_any().downcast_ref::<arrow::array::TimestampMillisecondArray>()).context("Missing admitted_at")?;

        let mut rows = Vec::with_capacity(batch.num_rows());
        for i in 0..batch.num_rows() {
            let ms = admitted_col.value(i);
            let admitted_at = OffsetDateTime::from_unix_timestamp_nanos((ms as i128) * 1_000_000)
                .unwrap_or(OffsetDateTime::UNIX_EPOCH);
            rows.push(NgramVocabularyRow {
                tokenizer_version: tv_col.value(i).to_string(),
                n: n_col.value(i),
                ngram: ngram_col.value(i).to_string(),
                global_count: gc_col.value(i),
                admitted_at,
            });
        }
        ch.insert_vocabulary_to(table, &rows).await?;
        total += rows.len() as u64;
    }
    Ok(total)
}

async fn stream_global_counts(
    path: &Path,
    table: &str,
    ch: &HnClickHouse,
) -> anyhow::Result<u64> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open {}", path.display()))?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)?.build()?;
    let mut total = 0u64;

    for batch_result in reader {
        let batch = batch_result?;
        let tv_col = batch.column_by_name("tokenizer_version").and_then(|c| c.as_any().downcast_ref::<StringArray>()).context("Missing tokenizer_version")?;
        let n_col = batch.column_by_name("n").and_then(|c| c.as_any().downcast_ref::<UInt8Array>()).context("Missing n")?;
        let ngram_col = batch.column_by_name("ngram").and_then(|c| c.as_any().downcast_ref::<StringArray>()).context("Missing ngram")?;
        let count_col = batch.column_by_name("count").and_then(|c| c.as_any().downcast_ref::<UInt64Array>()).context("Missing count")?;

        let mut rows = Vec::with_capacity(batch.num_rows());
        for i in 0..batch.num_rows() {
            rows.push(GlobalCountRow {
                tokenizer_version: tv_col.value(i).to_string(),
                n: n_col.value(i),
                ngram: ngram_col.value(i).to_string(),
                count: count_col.value(i),
            });
        }
        ch.insert_global_counts_to(table, &rows).await?;
        total += rows.len() as u64;
    }
    Ok(total)
}

fn read_ingestion_log(path: &Path) -> anyhow::Result<IngestionLogRow> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open {}", path.display()))?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)?.build()?;

    for batch_result in reader {
        let batch = batch_result?;
        if batch.num_rows() == 0 {
            continue;
        }

        let tv = batch
            .column_by_name("tokenizer_version")
            .and_then(|c| c.as_any().downcast_ref::<StringArray>())
            .context("Missing tokenizer_version")?;
        let cmd = batch
            .column_by_name("command")
            .and_then(|c| c.as_any().downcast_ref::<StringArray>())
            .context("Missing command")?;
        let ts = batch
            .column_by_name("last_ingested_ts")
            .and_then(|c| c.as_any().downcast_ref::<Int64Array>())
            .context("Missing last_ingested_ts")?;
        let cp = batch
            .column_by_name("comments_processed")
            .and_then(|c| c.as_any().downcast_ref::<UInt64Array>())
            .context("Missing comments_processed")?;
        let nci = batch
            .column_by_name("ngram_counts_inserted")
            .and_then(|c| c.as_any().downcast_ref::<UInt64Array>())
            .context("Missing ngram_counts_inserted")?;
        let bti = batch
            .column_by_name("bucket_totals_inserted")
            .and_then(|c| c.as_any().downcast_ref::<UInt64Array>())
            .context("Missing bucket_totals_inserted")?;
        let vi = batch
            .column_by_name("vocabulary_inserted")
            .and_then(|c| c.as_any().downcast_ref::<UInt64Array>())
            .context("Missing vocabulary_inserted")?;
        let sm = batch
            .column_by_name("start_month")
            .and_then(|c| c.as_any().downcast_ref::<StringArray>())
            .context("Missing start_month")?;
        let em = batch
            .column_by_name("end_month")
            .and_then(|c| c.as_any().downcast_ref::<StringArray>())
            .context("Missing end_month")?;
        let dur = batch
            .column_by_name("duration_ms")
            .and_then(|c| c.as_any().downcast_ref::<UInt64Array>())
            .context("Missing duration_ms")?;

        return Ok(IngestionLogRow {
            tokenizer_version: tv.value(0).to_string(),
            command: cmd.value(0).to_string(),
            last_ingested_ts: ts.value(0),
            comments_processed: cp.value(0),
            ngram_counts_inserted: nci.value(0),
            bucket_totals_inserted: bti.value(0),
            vocabulary_inserted: vi.value(0),
            start_month: sm.value(0).to_string(),
            end_month: em.value(0).to_string(),
            duration_ms: dur.value(0),
        });
    }

    bail!("ingestion_log.parquet is empty")
}

// ============================================================================
// Helpers
// ============================================================================

/// Convert Date32 (days since 1970-01-01) to time::Date.
fn days_to_date(days: i32) -> anyhow::Result<time::Date> {
    let epoch = time::Date::from_ordinal_date(1970, 1).unwrap();
    let date = epoch
        .checked_add(time::Duration::days(days as i64))
        .context("Invalid Date32 value")?;
    Ok(date)
}
