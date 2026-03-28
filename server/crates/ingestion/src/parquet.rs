//! Parquet reading, row filtering, and parallel processing (RFC-004 §2, §6).

use anyhow::Context;
use arrow::array::{
    Array, Int8Array, StringArray, TimestampMicrosecondArray, TimestampMillisecondArray, UInt8Array,
};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use std::path::Path;
use tokenizer::counter::NgramCounter;

/// A filtered HN comment ready for tokenization.
pub struct Comment {
    /// Daily bucket in "YYYY-MM-DD" format (UTC).
    pub bucket: String,
    /// Raw HTML text of the comment.
    pub text: String,
    /// Original timestamp in milliseconds since epoch.
    pub ts_ms: i64,
}

/// Read a Parquet file and return all filtered comments.
pub fn read_comments(path: &Path) -> anyhow::Result<Vec<Comment>> {
    read_comments_after(path, 0)
}

/// Read a Parquet file and return filtered comments with `time > min_ts`.
///
/// Filters: type=2 (comment), deleted=0, dead=0, text not null, time > min_ts.
/// Extracts bucket as UTC date from the `time` column.
pub fn read_comments_after(path: &Path, min_ts: i64) -> anyhow::Result<Vec<Comment>> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open {}", path.display()))?;

    let builder = ParquetRecordBatchReaderBuilder::try_new(file)
        .with_context(|| format!("Failed to read Parquet metadata from {}", path.display()))?
        .with_batch_size(8192);

    let reader = builder.build()?;
    let mut comments = Vec::new();

    for batch_result in reader {
        let batch = batch_result?;
        let num_rows = batch.num_rows();

        // Extract columns by name
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

        // Handle both millisecond and microsecond timestamp columns —
        // older HuggingFace files use ms, newer ones use us.
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

        for i in 0..num_rows {
            // Filter: type = 2 (comment)
            if type_col.is_null(i) || type_col.value(i) != 2 {
                continue;
            }
            // Filter: deleted = 0
            if deleted_col.is_null(i) || deleted_col.value(i) != 0 {
                continue;
            }
            // Filter: dead = 0
            if dead_col.is_null(i) || dead_col.value(i) != 0 {
                continue;
            }
            // Filter: text not null
            if text_col.is_null(i) {
                continue;
            }
            // Filter: time not null
            if time_col_raw.is_null(i) {
                continue;
            }

            let text = text_col.value(i);
            if text.is_empty() {
                continue;
            }

            // Extract timestamp as millis, handling both ms and us columns
            let ts_ms = if let Some(col) = time_us_col {
                col.value(i) / 1000
            } else if let Some(col) = time_ms_col {
                col.value(i)
            } else {
                continue;
            };

            // Watermark filter: skip comments already ingested
            if ts_ms <= min_ts {
                continue;
            }

            let bucket = match millis_to_date_string(ts_ms) {
                Some(s) => s,
                None => continue,
            };

            comments.push(Comment {
                bucket,
                text: text.to_string(),
                ts_ms,
            });
        }
    }

    Ok(comments)
}

/// Convert milliseconds since epoch to "YYYY-MM-DD" string in UTC.
fn millis_to_date_string(ms: i64) -> Option<String> {
    let secs = ms / 1000;
    let nanos = ((ms % 1000) * 1_000_000) as i128;
    let total_nanos = (secs as i128) * 1_000_000_000 + nanos;
    let dt = time::OffsetDateTime::from_unix_timestamp_nanos(total_nanos).ok()?;
    let date = dt.date();
    Some(format!(
        "{}-{:02}-{:02}",
        date.year(),
        date.month() as u8,
        date.day()
    ))
}

/// Process comments in parallel using rayon, returning a merged NgramCounter.
pub fn process_comments_parallel(comments: &[Comment]) -> NgramCounter {
    use rayon::prelude::*;

    if comments.is_empty() {
        return NgramCounter::new();
    }

    let counters: Vec<NgramCounter> = comments
        .par_chunks(1024)
        .map(|chunk| {
            let mut counter = NgramCounter::new();
            for c in chunk {
                let tokens = tokenizer::tokenize(&c.text);
                counter.process_comment(&c.bucket, &tokens);
            }
            counter
        })
        .collect();

    let mut merged = NgramCounter::new();
    for c in counters {
        merged.merge(c);
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_millis_to_date() {
        // 2024-01-15 12:30:00 UTC = 1705318200000 ms
        assert_eq!(
            millis_to_date_string(1705318200000),
            Some("2024-01-15".to_string())
        );
    }

    #[test]
    fn test_millis_epoch() {
        assert_eq!(millis_to_date_string(0), Some("1970-01-01".to_string()));
    }

    #[test]
    fn test_millis_to_date_2006() {
        // 2006-10-09 = first HN items
        // 1160352000000 ms
        assert_eq!(
            millis_to_date_string(1160352000000),
            Some("2006-10-09".to_string())
        );
    }
}
