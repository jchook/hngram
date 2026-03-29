//! Import: load Parquet output files into ClickHouse with atomic table swap (RFC-004 §10).
//!
//! POSTs Parquet files directly to ClickHouse's HTTP API (`FORMAT Parquet`),
//! bypassing row-by-row deserialization. Loads into staging tables, then
//! atomically swaps each data table.

use anyhow::{bail, Context};
use arrow::array::{Array, Int64Array, StringArray, UInt64Array};
use hn_clickhouse::{
    HnClickHouse, IngestionLogRow, TABLE_BUCKET_TOTALS, TABLE_GLOBAL_COUNTS, TABLE_NGRAM_COUNTS,
    TABLE_NGRAM_VOCABULARY,
};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use std::path::Path;

/// The data tables that get swapped during import.
const DATA_TABLES: [&str; 4] = [
    TABLE_NGRAM_COUNTS,
    TABLE_BUCKET_TOTALS,
    TABLE_NGRAM_VOCABULARY,
    TABLE_GLOBAL_COUNTS,
];

/// Parquet file → ClickHouse table mapping.
const FILE_TABLE_MAP: [(&str, &str); 4] = [
    ("ngram_counts.parquet", TABLE_NGRAM_COUNTS),
    ("bucket_totals.parquet", TABLE_BUCKET_TOTALS),
    ("ngram_vocabulary.parquet", TABLE_NGRAM_VOCABULARY),
    ("global_counts.parquet", TABLE_GLOBAL_COUNTS),
];

/// Run the import: POST Parquet files to staging tables, swap, append log.
pub async fn import(data_dir: &Path, ch: &HnClickHouse) -> anyhow::Result<()> {
    let output_dir = data_dir.join("output");
    let run_start = std::time::Instant::now();

    // Validate all output files exist
    let log_path = output_dir.join("ingestion_log.parquet");
    for (file, _) in &FILE_TABLE_MAP {
        let path = output_dir.join(file);
        if !path.exists() {
            bail!("Missing output file: {}", path.display());
        }
    }
    if !log_path.exists() {
        bail!("Missing output file: {}", log_path.display());
    }

    // Build reqwest client for direct Parquet POST
    let http_client = reqwest::Client::new();

    // Create staging tables
    tracing::info!("Creating staging tables...");
    for table in &DATA_TABLES {
        ch.create_staging_table(table).await?;
    }

    // POST each Parquet file directly to its staging table
    for (file, table) in &FILE_TABLE_MAP {
        let path = output_dir.join(file);
        let staging = format!("{}_staging", table);
        tracing::info!("Loading {} → {}...", file, staging);

        let start = std::time::Instant::now();
        post_parquet_file(&http_client, ch, &staging, &path).await?;
        tracing::info!("  Loaded in {:.1}s", start.elapsed().as_secs_f64());
    }

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
// Direct Parquet POST to ClickHouse HTTP API
// ============================================================================

/// Batch size for chunked Parquet import (rows per POST).
const IMPORT_BATCH_SIZE: usize = 100_000;

/// Stream a Parquet file to ClickHouse in chunks.
///
/// Reads the file in batches with Arrow, re-encodes each batch as a small
/// in-memory Parquet buffer, and POSTs it. This bounds ClickHouse memory
/// regardless of total file size.
async fn post_parquet_file(
    http_client: &reqwest::Client,
    ch: &HnClickHouse,
    table: &str,
    path: &Path,
) -> anyhow::Result<()> {
    use parquet::arrow::ArrowWriter;

    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open {}", path.display()))?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)
        .with_context(|| format!("Failed to read Parquet metadata from {}", path.display()))?;
    let schema = builder.schema().clone();
    let file_meta = builder.metadata().file_metadata();
    let expected_rows = file_meta.num_rows() as u64;
    let reader = builder.with_batch_size(IMPORT_BATCH_SIZE).build()?;

    let query = format!("INSERT INTO {} FORMAT Parquet", table);
    let mut total_rows = 0u64;
    let mut last_pct = 0u8;

    for batch_result in reader {
        let batch = batch_result?;
        let num_rows = batch.num_rows();

        // Re-encode batch as a small Parquet buffer
        let mut buf: Vec<u8> = Vec::new();
        let mut writer = ArrowWriter::try_new(&mut buf, schema.clone(), None)?;
        writer.write(&batch)?;
        writer.close()?;

        // POST the chunk
        post_parquet_bytes(http_client, ch, table, &query, buf).await?;
        total_rows += num_rows as u64;

        if expected_rows > 0 {
            let pct = (total_rows * 100 / expected_rows).min(100) as u8;
            let bucket = pct / 10 * 10;
            if bucket > last_pct && bucket < 100 {
                last_pct = bucket;
                tracing::info!("  {}%  ({} / {} rows)", bucket, total_rows, expected_rows);
            }
        }
    }

    tracing::info!("  {} total rows", total_rows);
    Ok(())
}

/// POST a Parquet byte buffer to ClickHouse.
async fn post_parquet_bytes(
    http_client: &reqwest::Client,
    ch: &HnClickHouse,
    table: &str,
    query: &str,
    body: Vec<u8>,
) -> anyhow::Result<()> {
    let mut req = http_client
        .post(ch.http_url())
        .query(&[("query", query), ("database", ch.database())])
        .header("Content-Type", "application/octet-stream")
        .body(body);

    let user = ch.http_user();
    let password = ch.http_password();
    if !user.is_empty() {
        req = req.header("X-ClickHouse-User", user);
    }
    if !password.is_empty() {
        req = req.header("X-ClickHouse-Key", password);
    }

    let resp = req.send().await.context("Failed to POST Parquet to ClickHouse")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!(
            "ClickHouse rejected Parquet upload to {}: {} — {}",
            table,
            status,
            body.trim()
        );
    }

    Ok(())
}

// ============================================================================
// Ingestion log reader (small file, still uses Arrow)
// ============================================================================

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
