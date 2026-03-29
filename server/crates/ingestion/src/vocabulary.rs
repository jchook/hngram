//! Partial file I/O and k-way sorted merge for ingestion.
//!
//! Three partial file types:
//! - `.globals`  — (n, ngram, global_count) for vocabulary admission
//! - `.counts`   — (bucket, n, ngram, count) for per-bucket n-gram counts
//! - `.totals`   — (bucket, n, total) for per-bucket denominators

use anyhow::Context;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use tokenizer::counter::{BucketKey, NgramKey};

// ============================================================================
// Directory and path helpers
// ============================================================================

pub fn partial_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("partial")
}

pub fn numbered_partial_path(data_dir: &Path, index: u32, ext: &str) -> PathBuf {
    partial_dir(data_dir).join(format!("{:04}.{}", index, ext))
}

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

pub fn is_pass_complete(data_dir: &Path) -> bool {
    partial_dir(data_dir).join(".complete").exists()
}

pub fn mark_pass_complete(data_dir: &Path) -> anyhow::Result<()> {
    let marker = partial_dir(data_dir).join(".complete");
    let tmp = marker.with_extension("complete.tmp");
    std::fs::write(&tmp, "done")?;
    std::fs::rename(&tmp, &marker)?;
    Ok(())
}

/// Load the set of source files that have been durably flushed to partials.
pub fn load_done_files(data_dir: &Path) -> std::collections::HashSet<String> {
    let path = partial_dir(data_dir).join(".progress");
    if !path.exists() {
        return std::collections::HashSet::new();
    }
    match std::fs::read_to_string(&path) {
        Ok(contents) => contents.lines().map(|s| s.to_string()).collect(),
        Err(_) => std::collections::HashSet::new(),
    }
}

/// Append source file paths to the done-files progress tracker.
/// Atomic: writes to tmp then renames (appends to existing content).
pub fn append_done_files(data_dir: &Path, files: &[String]) -> anyhow::Result<()> {
    if files.is_empty() {
        return Ok(());
    }
    let path = partial_dir(data_dir).join(".progress");
    let mut contents = if path.exists() {
        std::fs::read_to_string(&path).unwrap_or_default()
    } else {
        String::new()
    };
    for f in files {
        contents.push_str(f);
        contents.push('\n');
    }
    let tmp = path.with_extension("progress.tmp");
    std::fs::write(&tmp, &contents)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

fn find_files_with_ext(data_dir: &Path, ext: &str) -> anyhow::Result<Vec<PathBuf>> {
    let dir = partial_dir(data_dir);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut paths: Vec<PathBuf> = Vec::new();
    collect_files_recursive(&dir, ext, &mut paths)?;
    paths.sort();
    Ok(paths)
}

fn collect_files_recursive(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files_recursive(&path, ext, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some(ext) {
            out.push(path);
        }
    }
    Ok(())
}

// ============================================================================
// Write sorted partial files
// ============================================================================

fn atomic_write<F>(path: &Path, tmp_ext: &str, write_fn: F) -> anyhow::Result<()>
where
    F: FnOnce(&mut std::io::BufWriter<std::fs::File>) -> anyhow::Result<()>,
{
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(tmp_ext);
    let mut file = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
    write_fn(&mut file)?;
    file.flush()?;
    drop(file);
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Write global counts sorted by (n, ngram). Format: n\tngram\tcount\n
pub fn write_globals(path: &Path, counts: &HashMap<(u8, String), u64>) -> anyhow::Result<()> {
    let mut entries: Vec<_> = counts.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    atomic_write(path, "globals.tmp", |file| {
        for ((n, ngram), count) in entries {
            writeln!(file, "{}\t{}\t{}", n, ngram, count)?;
        }
        Ok(())
    })
}

/// Write per-bucket counts sorted by (bucket, n, ngram). Format: bucket\tn\tngram\tcount\n
pub fn write_counts(path: &Path, counts: &HashMap<NgramKey, u32>) -> anyhow::Result<()> {
    let mut entries: Vec<_> = counts.iter().collect();
    entries.sort_by(|a, b| {
        a.0.bucket
            .cmp(&b.0.bucket)
            .then(a.0.n.cmp(&b.0.n))
            .then(a.0.ngram.cmp(&b.0.ngram))
    });
    atomic_write(path, "counts.tmp", |file| {
        for (key, count) in entries {
            writeln!(file, "{}\t{}\t{}\t{}", key.bucket, key.n, key.ngram, count)?;
        }
        Ok(())
    })
}

/// Write per-bucket totals sorted by (bucket, n). Format: bucket\tn\ttotal\n
pub fn write_totals(path: &Path, totals: &HashMap<BucketKey, u64>) -> anyhow::Result<()> {
    let mut entries: Vec<_> = totals.iter().collect();
    entries.sort_by(|a, b| a.0.bucket.cmp(&b.0.bucket).then(a.0.n.cmp(&b.0.n)));
    atomic_write(path, "totals.tmp", |file| {
        for (key, total) in entries {
            writeln!(file, "{}\t{}\t{}", key.bucket, key.n, total)?;
        }
        Ok(())
    })
}

// ============================================================================
// Reusable k-way merge
// ============================================================================

fn read_next_line(
    reader: &mut std::io::BufReader<std::fs::File>,
    buf: &mut String,
) -> anyhow::Result<Option<String>> {
    buf.clear();
    let bytes = reader.read_line(buf)?;
    if bytes == 0 {
        Ok(None)
    } else {
        Ok(Some(buf.trim_end().to_string()))
    }
}

struct KWayMerge {
    readers: Vec<std::io::BufReader<std::fs::File>>,
    bufs: Vec<String>,
    heap: BinaryHeap<Reverse<(String, usize)>>,
}

impl KWayMerge {
    fn new(paths: &[PathBuf]) -> anyhow::Result<Self> {
        let mut readers = Vec::with_capacity(paths.len());
        let mut bufs = Vec::with_capacity(paths.len());
        let mut heap = BinaryHeap::new();

        for (i, path) in paths.iter().enumerate() {
            let file = std::fs::File::open(path)
                .with_context(|| format!("Failed to open {}", path.display()))?;
            let mut reader = std::io::BufReader::new(file);
            let mut buf = String::new();

            if let Some(line) = read_next_line(&mut reader, &mut buf)? {
                heap.push(Reverse((line, i)));
            }

            readers.push(reader);
            bufs.push(buf);
        }

        Ok(Self {
            readers,
            bufs,
            heap,
        })
    }

    fn pop(&mut self) -> anyhow::Result<Option<String>> {
        if let Some(Reverse((line, idx))) = self.heap.pop() {
            if let Some(next) = read_next_line(&mut self.readers[idx], &mut self.bufs[idx])? {
                self.heap.push(Reverse((next, idx)));
            }
            Ok(Some(line))
        } else {
            Ok(None)
        }
    }

    fn peek_line(&self) -> Option<&str> {
        self.heap.peek().map(|Reverse((line, _))| line.as_str())
    }
}

/// Split a TSV line into key prefix (everything before last tab) and value (after last tab).
fn split_key_value(line: &str) -> (&str, &str) {
    match line.rfind('\t') {
        Some(pos) => (&line[..pos], &line[pos + 1..]),
        None => (line, ""),
    }
}

// ============================================================================
// Globals merge
// ============================================================================

pub struct MergedGlobal {
    pub n: u8,
    pub ngram: String,
    pub count: u64,
}

pub fn merge_globals_streaming<F>(data_dir: &Path, mut callback: F) -> anyhow::Result<()>
where
    F: FnMut(MergedGlobal) -> anyhow::Result<()>,
{
    let paths = find_files_with_ext(data_dir, "globals")?;
    if paths.is_empty() {
        tracing::warn!("No .globals partial files found");
        return Ok(());
    }

    tracing::info!("Streaming merge of {} .globals files", paths.len());
    let mut merger = KWayMerge::new(&paths)?;
    let mut merged_count = 0u64;

    while let Some(line) = merger.pop()? {
        let (key, val) = split_key_value(&line);
        let mut total: u64 = val.parse().unwrap_or(0);

        // Sum all lines with the same key prefix
        while let Some(peek) = merger.peek_line() {
            let (peek_key, _) = split_key_value(peek);
            if peek_key != key {
                break;
            }
            let next = merger.pop()?.unwrap();
            let (_, v) = split_key_value(&next);
            total += v.parse::<u64>().unwrap_or(0);
        }

        // Parse key: "n\tngram"
        if let Some(tab) = key.find('\t') {
            let n: u8 = key[..tab].parse().unwrap_or(0);
            let ngram = key[tab + 1..].to_string();
            if n >= 1 && n <= 3 {
                callback(MergedGlobal {
                    n,
                    ngram,
                    count: total,
                })?;
                merged_count += 1;
                if merged_count % 1_000_000 == 0 {
                    tracing::info!("  Merged {} unique n-grams so far...", merged_count);
                }
            }
        }
    }

    tracing::info!("Globals merge complete — {} unique n-grams", merged_count);
    Ok(())
}

// ============================================================================
// Counts merge
// ============================================================================

pub struct MergedCount {
    pub bucket: String,
    pub n: u8,
    pub ngram: String,
    pub count: u32,
}

pub fn merge_counts_streaming<F>(data_dir: &Path, mut callback: F) -> anyhow::Result<()>
where
    F: FnMut(MergedCount) -> anyhow::Result<()>,
{
    let paths = find_files_with_ext(data_dir, "counts")?;
    if paths.is_empty() {
        tracing::warn!("No .counts partial files found");
        return Ok(());
    }

    tracing::info!("Streaming merge of {} .counts files", paths.len());
    let mut merger = KWayMerge::new(&paths)?;
    let mut merged_count = 0u64;

    while let Some(line) = merger.pop()? {
        let (key, val) = split_key_value(&line);
        let mut total: u64 = val.parse().unwrap_or(0);

        while let Some(peek) = merger.peek_line() {
            let (peek_key, _) = split_key_value(peek);
            if peek_key != key {
                break;
            }
            let next = merger.pop()?.unwrap();
            let (_, v) = split_key_value(&next);
            total += v.parse::<u64>().unwrap_or(0);
        }

        // Parse key: "bucket\tn\tngram"
        let parts: Vec<&str> = key.splitn(3, '\t').collect();
        if parts.len() == 3 {
            let bucket = parts[0].to_string();
            let n: u8 = parts[1].parse().unwrap_or(0);
            let ngram = parts[2].to_string();

            callback(MergedCount {
                bucket,
                n,
                ngram,
                count: total as u32,
            })?;

            merged_count += 1;
            if merged_count % 5_000_000 == 0 {
                tracing::info!("  Merged {} count entries so far...", merged_count);
            }
        }
    }

    tracing::info!("Counts merge complete — {} entries", merged_count);
    Ok(())
}

// ============================================================================
// Totals merge
// ============================================================================

pub struct MergedTotal {
    pub bucket: String,
    pub n: u8,
    pub total: u64,
}

pub fn merge_totals_streaming<F>(data_dir: &Path, mut callback: F) -> anyhow::Result<()>
where
    F: FnMut(MergedTotal) -> anyhow::Result<()>,
{
    let paths = find_files_with_ext(data_dir, "totals")?;
    if paths.is_empty() {
        tracing::warn!("No .totals partial files found");
        return Ok(());
    }

    tracing::info!("Streaming merge of {} .totals files", paths.len());
    let mut merger = KWayMerge::new(&paths)?;
    let mut merged_count = 0u64;

    while let Some(line) = merger.pop()? {
        let (key, val) = split_key_value(&line);
        let mut total: u64 = val.parse().unwrap_or(0);

        while let Some(peek) = merger.peek_line() {
            let (peek_key, _) = split_key_value(peek);
            if peek_key != key {
                break;
            }
            let next = merger.pop()?.unwrap();
            let (_, v) = split_key_value(&next);
            total += v.parse::<u64>().unwrap_or(0);
        }

        // Parse key: "bucket\tn"
        if let Some(tab) = key.find('\t') {
            let bucket = key[..tab].to_string();
            let n: u8 = key[tab + 1..].parse().unwrap_or(0);

            callback(MergedTotal {
                bucket,
                n,
                total,
            })?;
            merged_count += 1;
        }
    }

    tracing::info!("Totals merge complete — {} entries", merged_count);
    Ok(())
}
