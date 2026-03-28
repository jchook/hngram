//! Vocabulary utilities: partial count file I/O and k-way sorted merge.

use anyhow::Context;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

/// Directory for partial count files.
pub fn partial_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("partial")
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

/// Discover all .counts files in the partial directory (recursively, sorted).
pub fn find_partial_files(data_dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let dir = partial_dir(data_dir);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut paths: Vec<PathBuf> = Vec::new();
    collect_counts_files(&dir, &mut paths)?;
    paths.sort();
    Ok(paths)
}

fn collect_counts_files(dir: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_counts_files(&path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("counts") {
            out.push(path);
        }
    }
    Ok(())
}

/// Find the next available partial file number in the partial directory.
/// Scans once and returns a counter starting after the highest existing number.
pub fn next_partial_counter(data_dir: &Path) -> u32 {
    let dir = partial_dir(data_dir);
    if !dir.exists() {
        return 0;
    }
    let mut max_num: u32 = 0;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if let Some(stem) = entry.path().file_stem().and_then(|s| s.to_str()) {
                if let Ok(num) = stem.parse::<u32>() {
                    max_num = max_num.max(num + 1);
                }
            }
        }
    }
    max_num
}

/// Build the path for a numbered partial file.
pub fn numbered_partial_path(data_dir: &Path, index: u32) -> PathBuf {
    partial_dir(data_dir).join(format!("{:04}.counts", index))
}

/// Check if pass 1 completed successfully (atomic marker exists).
pub fn is_pass1_complete(data_dir: &Path) -> bool {
    partial_dir(data_dir).join(".complete").exists()
}

/// Mark pass 1 as complete (atomic write).
pub fn mark_pass1_complete(data_dir: &Path) -> anyhow::Result<()> {
    let marker = partial_dir(data_dir).join(".complete");
    let tmp = marker.with_extension("complete.tmp");
    std::fs::write(&tmp, "done")?;
    std::fs::rename(&tmp, &marker)?;
    Ok(())
}

// ============================================================================
// K-way sorted merge
// ============================================================================

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

fn read_next(
    reader: &mut std::io::BufReader<std::fs::File>,
    buf: &mut String,
) -> anyhow::Result<Option<(u8, String, u64)>> {
    loop {
        buf.clear();
        let bytes = reader.read_line(buf)?;
        if bytes == 0 {
            return Ok(None);
        }
        if let Some(entry) = parse_line(buf.trim_end()) {
            return Ok(Some(entry));
        }
    }
}

/// A single merged (n, ngram, total_count) result from the k-way merge.
pub struct MergedEntry {
    pub n: u8,
    pub ngram: String,
    pub count: u64,
}

/// K-way merge of all sorted partial count files in the partial directory.
/// Discovers files via glob, opens them sorted, and streams merged results
/// to the callback. Memory usage is O(num_files).
pub fn merge_partial_counts_streaming<F>(
    data_dir: &Path,
    mut callback: F,
) -> anyhow::Result<()>
where
    F: FnMut(MergedEntry) -> anyhow::Result<()>,
{
    let paths = find_partial_files(data_dir)?;
    if paths.is_empty() {
        tracing::warn!("No partial count files found");
        return Ok(());
    }

    // Open all partial files and seed the heap
    let mut readers: Vec<std::io::BufReader<std::fs::File>> = Vec::new();
    let mut heap: BinaryHeap<Reverse<HeapEntry>> = BinaryHeap::new();
    let mut bufs: Vec<String> = Vec::new();

    for (i, path) in paths.iter().enumerate() {
        let file = std::fs::File::open(path)
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
    }

    tracing::info!("Streaming merge of {} partial files", paths.len());

    let mut merged_count = 0u64;

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
