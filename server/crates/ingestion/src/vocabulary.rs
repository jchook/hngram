//! Vocabulary utilities: partial count file I/O and k-way sorted merge.

use crate::months::YearMonth;
use anyhow::Context;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

/// Directory for partial count files.
pub fn partial_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("partial")
}

/// Path for a partial count file.
pub fn partial_path(data_dir: &Path, ym: &YearMonth) -> PathBuf {
    partial_dir(data_dir).join(format!("{}.counts", ym))
}

/// Write partial global counts to a sorted TSV file: n\tngram\tcount\n
/// Sorted by (n, ngram) to enable k-way merge.
pub fn write_partial_counts(
    path: &Path,
    counts: &HashMap<(u8, String), u64>,
) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Sort by (n, ngram) for merge compatibility
    let mut entries: Vec<_> = counts.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));

    let tmp = path.with_extension("counts.tmp");
    let mut file = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
    for ((n, ngram), count) in entries {
        writeln!(file, "{}\t{}\t{}", n, ngram, count)?;
    }
    file.flush()?;
    drop(file);
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// An entry from a partial count file, used in the merge heap.
#[derive(Eq, PartialEq)]
struct HeapEntry {
    n: u8,
    ngram: String,
    count: u64,
    file_idx: usize,
}

impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.n
            .cmp(&other.n)
            .then_with(|| self.ngram.cmp(&other.ngram))
    }
}

impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Parse one TSV line into (n, ngram, count). Returns None on invalid lines.
fn parse_line(line: &str) -> Option<(u8, String, u64)> {
    let parts: Vec<&str> = line.splitn(3, '\t').collect();
    if parts.len() != 3 {
        return None;
    }
    let n: u8 = parts[0].parse().ok()?;
    let ngram = parts[1].to_string();
    let count: u64 = parts[2].parse().ok()?;
    if n >= 1 && n <= 3 && count > 0 {
        Some((n, ngram, count))
    } else {
        None
    }
}

/// Read the next valid entry from a buffered reader.
fn read_next(
    reader: &mut std::io::BufReader<std::fs::File>,
    buf: &mut String,
) -> anyhow::Result<Option<(u8, String, u64)>> {
    loop {
        buf.clear();
        let bytes = reader.read_line(buf)?;
        if bytes == 0 {
            return Ok(None); // EOF
        }
        if let Some(entry) = parse_line(buf.trim_end()) {
            return Ok(Some(entry));
        }
        // Skip invalid lines
    }
}

/// A single merged (n, ngram, total_count) result from the k-way merge.
pub struct MergedEntry {
    pub n: u8,
    pub ngram: String,
    pub count: u64,
}

/// K-way merge of sorted partial count files. Calls `callback` for each unique
/// (n, ngram) with its summed count across all files. Memory usage is O(num_files)
/// — one buffered line per open file, plus the heap.
pub fn merge_partial_counts_streaming<F>(
    data_dir: &Path,
    months: &[YearMonth],
    mut callback: F,
) -> anyhow::Result<()>
where
    F: FnMut(MergedEntry) -> anyhow::Result<()>,
{
    let total = months.len();

    // Open all partial files and seed the heap
    let mut readers: Vec<std::io::BufReader<std::fs::File>> = Vec::new();
    let mut heap: BinaryHeap<Reverse<HeapEntry>> = BinaryHeap::new();
    let mut bufs: Vec<String> = Vec::new();
    let mut opened = 0usize;

    for (i, ym) in months.iter().enumerate() {
        let path = partial_path(data_dir, ym);
        if !path.exists() {
            // Push a placeholder to keep indices aligned
            readers.push(std::io::BufReader::new(
                std::fs::File::open("/dev/null").unwrap(),
            ));
            bufs.push(String::new());
            continue;
        }

        let file = std::fs::File::open(&path)
            .with_context(|| format!("Failed to open {}", path.display()))?;
        let mut reader = std::io::BufReader::new(file);
        let mut buf = String::new();

        if let Some((n, ngram, count)) = read_next(&mut reader, &mut buf)? {
            heap.push(Reverse(HeapEntry {
                n,
                ngram,
                count,
                file_idx: i,
            }));
        }

        readers.push(reader);
        bufs.push(buf);
        opened += 1;
    }

    tracing::info!(
        "Streaming merge of {} partial files (of {} months)",
        opened,
        total,
    );

    let mut merged_count = 0u64;

    // K-way merge: pop smallest key, accumulate all entries with same key, emit
    while let Some(Reverse(entry)) = heap.pop() {
        let current_n = entry.n;
        let current_ngram = entry.ngram;
        let mut current_count = entry.count;

        // Advance the file this entry came from
        if let Some((n, ngram, count)) =
            read_next(&mut readers[entry.file_idx], &mut bufs[entry.file_idx])?
        {
            heap.push(Reverse(HeapEntry {
                n,
                ngram,
                count,
                file_idx: entry.file_idx,
            }));
        }

        // Drain all entries with the same key
        while let Some(Reverse(peek)) = heap.peek() {
            if peek.n != current_n || peek.ngram != current_ngram {
                break;
            }
            current_count += peek.count;
            let file_idx = peek.file_idx;

            heap.pop();

            // Advance that file
            if let Some((n, ngram, count)) =
                read_next(&mut readers[file_idx], &mut bufs[file_idx])?
            {
                heap.push(Reverse(HeapEntry {
                    n,
                    ngram,
                    count,
                    file_idx,
                }));
            }
        }

        callback(MergedEntry {
            n: current_n,
            ngram: current_ngram,
            count: current_count,
        })?;

        merged_count += 1;
        if merged_count % 1_000_000 == 0 {
            tracing::info!("  Merged {} unique n-grams so far...", merged_count);
        }
    }

    tracing::info!("Merge complete — {} unique n-grams", merged_count);
    Ok(())
}

