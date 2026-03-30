//! Parquet reading, row filtering, and parallel processing (RFC-004 §2, §6).

use crate::config::BucketGranularity;
use anyhow::Context;
use arrow::array::{
    Array, Int8Array, StringArray, TimestampMicrosecondArray, TimestampMillisecondArray, UInt8Array,
};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use std::path::Path;
use tokenizer::counter::NgramCounter;

/// A filtered HN comment ready for tokenization.
pub struct Comment {
    /// Bucket in "YYYY-MM-DD" format (UTC), granularity depends on config.
    pub bucket: String,
    /// Raw HTML text of the comment.
    pub text: String,
    /// Original timestamp in milliseconds since epoch.
    pub ts_ms: i64,
}

/// Read a Parquet file and return filtered comments with `time > min_ts`.
///
/// Filters: type=2 (comment), deleted=0, dead=0, text not null, time > min_ts.
/// Extracts bucket as UTC date from the `time` column.
pub fn read_comments_after(path: &Path, min_ts: i64, granularity: BucketGranularity) -> anyhow::Result<Vec<Comment>> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open {}", path.display()))?;

    let builder = ParquetRecordBatchReaderBuilder::try_new(file)
        .with_context(|| format!("Failed to read Parquet metadata from {}", path.display()))?
        .with_batch_size(8192);

    let reader = builder.build()?;
    let mut comments = Vec::new();

    for batch_result in reader {
        let batch = batch_result?;
        comments.extend(extract_comments_from_batch(&batch, min_ts, granularity)?);
    }

    Ok(comments)
}

/// Convert milliseconds since epoch to a bucket date string in UTC.
/// Granularity controls the resolution:
///   Daily:   "YYYY-MM-DD"
///   Monthly: "YYYY-MM-01"
///   Yearly:  "YYYY-01-01"
fn millis_to_date_string(ms: i64, granularity: BucketGranularity) -> Option<String> {
    let secs = ms / 1000;
    let nanos = ((ms % 1000) * 1_000_000) as i128;
    let total_nanos = (secs as i128) * 1_000_000_000 + nanos;
    let dt = time::OffsetDateTime::from_unix_timestamp_nanos(total_nanos).ok()?;
    let date = dt.date();
    match granularity {
        BucketGranularity::Daily => Some(format!(
            "{}-{:02}-{:02}",
            date.year(),
            date.month() as u8,
            date.day()
        )),
        BucketGranularity::Monthly => Some(format!(
            "{}-{:02}-01",
            date.year(),
            date.month() as u8,
        )),
        BucketGranularity::Yearly => Some(format!(
            "{}-01-01",
            date.year(),
        )),
    }
}

/// Stream NgramCounter data from a Parquet file, one batch at a time.
/// Calls `on_batch` with each batch's NgramCounter (per-bucket counts + totals).
/// Memory per batch: one NgramCounter for ~8192 rows (a few MB).
pub fn stream_counters<F>(path: &Path, min_ts: i64, granularity: BucketGranularity, mut on_batch: F) -> anyhow::Result<()>
where
    F: FnMut(NgramCounter) -> anyhow::Result<()>,
{
    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open {}", path.display()))?;

    let builder = ParquetRecordBatchReaderBuilder::try_new(file)
        .with_context(|| format!("Failed to read Parquet metadata from {}", path.display()))?
        .with_batch_size(65536);

    let reader = builder.build()?;

    for batch_result in reader {
        let batch = batch_result?;
        let comments = extract_comments_from_batch(&batch, min_ts, granularity)?;
        if comments.is_empty() {
            continue;
        }

        let counter = process_comments_parallel(&comments);
        on_batch(counter)?;
    }

    Ok(())
}

/// Extract filtered comments from a single Arrow record batch.
fn extract_comments_from_batch(
    batch: &arrow::record_batch::RecordBatch,
    min_ts: i64,
    granularity: BucketGranularity,
) -> anyhow::Result<Vec<Comment>> {
    let num_rows = batch.num_rows();

    let type_col = batch
        .column_by_name("type")
        .and_then(|c| c.as_any().downcast_ref::<Int8Array>())
        .context("Missing or invalid 'type' column")?;
    let deleted_col = batch
        .column_by_name("deleted")
        .and_then(|c| c.as_any().downcast_ref::<UInt8Array>())
        .context("Missing or invalid 'deleted' column")?;
    let dead_col = batch
        .column_by_name("dead")
        .and_then(|c| c.as_any().downcast_ref::<UInt8Array>())
        .context("Missing or invalid 'dead' column")?;
    let text_col = batch
        .column_by_name("text")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>())
        .context("Missing or invalid 'text' column")?;
    let time_col_raw = batch
        .column_by_name("time")
        .context("Missing 'time' column")?;
    let time_ms_col = time_col_raw
        .as_any()
        .downcast_ref::<TimestampMillisecondArray>();
    let time_us_col = time_col_raw
        .as_any()
        .downcast_ref::<TimestampMicrosecondArray>();
    if time_ms_col.is_none() && time_us_col.is_none() {
        anyhow::bail!(
            "'time' column has type {:?}, expected Timestamp(Millisecond) or Timestamp(Microsecond)",
            time_col_raw.data_type()
        );
    }

    let mut comments = Vec::new();
    for i in 0..num_rows {
        if type_col.is_null(i) || type_col.value(i) != 2 {
            continue;
        }
        if deleted_col.is_null(i) || deleted_col.value(i) != 0 {
            continue;
        }
        if dead_col.is_null(i) || dead_col.value(i) != 0 {
            continue;
        }
        if text_col.is_null(i) {
            continue;
        }
        if time_col_raw.is_null(i) {
            continue;
        }
        let text = text_col.value(i);
        if text.is_empty() {
            continue;
        }
        let ts_ms = if let Some(col) = time_us_col {
            col.value(i) / 1000
        } else if let Some(col) = time_ms_col {
            col.value(i)
        } else {
            continue;
        };
        if ts_ms <= min_ts {
            continue;
        }
        let bucket = match millis_to_date_string(ts_ms, granularity) {
            Some(s) => s,
            None => continue,
        };
        comments.push(Comment {
            bucket,
            text: text.to_string(),
            ts_ms,
        });
    }
    Ok(comments)
}

/// Process comments in parallel using rayon, returning a merged NgramCounter.
pub fn process_comments_parallel(comments: &[Comment]) -> NgramCounter {
    use rayon::prelude::*;

    if comments.is_empty() {
        return NgramCounter::new();
    }

    comments
        .par_chunks(1024)
        .map(|chunk| {
            let mut counter = NgramCounter::new();
            for c in chunk {
                let tokens = tokenizer::tokenize(&c.text);
                counter.process_comment(&c.bucket, &tokens);
            }
            counter
        })
        .reduce(NgramCounter::new, |mut a, b| {
            a.merge(b);
            a
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_millis_to_date_daily() {
        // 2024-01-15 12:30:00 UTC = 1705318200000 ms
        assert_eq!(
            millis_to_date_string(1705318200000, BucketGranularity::Daily),
            Some("2024-01-15".to_string())
        );
    }

    #[test]
    fn test_millis_to_date_monthly() {
        assert_eq!(
            millis_to_date_string(1705318200000, BucketGranularity::Monthly),
            Some("2024-01-01".to_string())
        );
    }

    #[test]
    fn test_millis_to_date_yearly() {
        assert_eq!(
            millis_to_date_string(1705318200000, BucketGranularity::Yearly),
            Some("2024-01-01".to_string())
        );
    }

    #[test]
    fn test_millis_epoch() {
        assert_eq!(millis_to_date_string(0, BucketGranularity::Daily), Some("1970-01-01".to_string()));
    }

    #[test]
    fn test_millis_to_date_2006() {
        // 2006-10-09 = first HN items
        // 1160352000000 ms
        assert_eq!(
            millis_to_date_string(1160352000000, BucketGranularity::Daily),
            Some("2006-10-09".to_string())
        );
        assert_eq!(
            millis_to_date_string(1160352000000, BucketGranularity::Monthly),
            Some("2006-10-01".to_string())
        );
    }
}
