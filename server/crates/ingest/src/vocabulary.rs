//! Sharded binary partial file I/O and parallel merge for ingest.
//!
//! During source processing, n-gram counts are flushed to sharded binary partial
//! files. Each entry is routed to a shard by `hash(key) % num_shards`, guaranteeing
//! all occurrences of a given key land in the same shard.
//!
//! At merge time, each shard is processed independently in parallel (rayon),
//! aggregating counts into a HashMap. Globals and totals are derived from counts.
//!
//! Binary record format (counts):
//!   [bucket_len:u16][bucket:bytes][n:u8][ngram_len:u16][ngram:bytes][count:u32]

use anyhow::Context;
use rayon::prelude::*;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokenizer::counter::NgramKey;

// ============================================================================
// Directory and path helpers
// ============================================================================

pub fn partial_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("partial")
}

/// Compute the next partial counter by scanning existing files.
pub fn next_partial_counter(data_dir: &Path) -> u32 {
    let dir = partial_dir(data_dir);
    if !dir.exists() {
        return 0;
    }
    let mut max_num: u32 = 0;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            // Files are named NNNN.sNN — extract the prefix before first '.'
            if let Some(prefix) = name.split('.').next() {
                if let Ok(num) = prefix.parse::<u32>() {
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

// ============================================================================
// Shard path helpers
// ============================================================================

/// Path for a shard file: `partial/NNNN.sNN`
fn shard_path(data_dir: &Path, index: u32, shard: usize, num_shards: usize) -> PathBuf {
    let width = num_shards.to_string().len();
    partial_dir(data_dir).join(format!("{:04}.s{:0>width$}", index, shard))
}

/// Temp path for atomic write: `partial/NNNN.sNN.tmp`
fn shard_tmp_path(data_dir: &Path, index: u32, shard: usize, num_shards: usize) -> PathBuf {
    let width = num_shards.to_string().len();
    partial_dir(data_dir).join(format!("{:04}.s{:0>width$}.tmp", index, shard))
}

/// Find all partial files for a given shard index.
/// Returns files matching `partial/*.sNN` sorted by name.
fn find_shard_files(data_dir: &Path, shard: usize, num_shards: usize) -> anyhow::Result<Vec<PathBuf>> {
    let dir = partial_dir(data_dir);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let width = num_shards.to_string().len();
    let suffix = format!(".s{:0>width$}", shard);
    let mut paths: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.ends_with(&suffix) && !name.ends_with(".tmp") {
            paths.push(entry.path());
        }
    }
    paths.sort();
    Ok(paths)
}

// ============================================================================
// Binary I/O helpers
// ============================================================================

fn write_str(w: &mut impl Write, s: &str) -> std::io::Result<()> {
    w.write_all(&(s.len() as u16).to_le_bytes())?;
    w.write_all(s.as_bytes())
}

fn write_u8(w: &mut impl Write, v: u8) -> std::io::Result<()> {
    w.write_all(&[v])
}

fn write_u32(w: &mut impl Write, v: u32) -> std::io::Result<()> {
    w.write_all(&v.to_le_bytes())
}

/// Read a length-prefixed string. Returns None on EOF (before the length prefix).
fn read_str(r: &mut impl Read, buf: &mut Vec<u8>) -> std::io::Result<Option<String>> {
    let mut len_buf = [0u8; 2];
    match r.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u16::from_le_bytes(len_buf) as usize;
    buf.resize(len, 0);
    r.read_exact(&mut buf[..len])?;
    String::from_utf8(buf[..len].to_vec())
        .map(Some)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

fn read_u8(r: &mut impl Read) -> std::io::Result<u8> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b)?;
    Ok(b[0])
}

fn read_u32(r: &mut impl Read) -> std::io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

// ============================================================================
// Sharded binary write
// ============================================================================

/// Compute shard index for an NgramKey.
fn shard_for_key(key: &NgramKey, num_shards: usize) -> usize {
    let mut hasher = std::hash::DefaultHasher::new();
    key.hash(&mut hasher);
    (hasher.finish() as usize) % num_shards
}

/// Write counts to sharded binary partial files.
///
/// Each entry is routed to `shard = hash(key) % num_shards`.
/// Files are written atomically (write to .tmp, then rename).
pub fn write_sharded(
    data_dir: &Path,
    index: u32,
    num_shards: usize,
    counts: &HashMap<NgramKey, u32>,
) -> anyhow::Result<()> {
    let p_dir = partial_dir(data_dir);
    std::fs::create_dir_all(&p_dir)?;

    // Open all shard writers
    let mut writers: Vec<BufWriter<std::fs::File>> = (0..num_shards)
        .map(|shard| {
            let tmp = shard_tmp_path(data_dir, index, shard, num_shards);
            let file = std::fs::File::create(&tmp)
                .with_context(|| format!("Failed to create {}", tmp.display()))
                .unwrap();
            BufWriter::new(file)
        })
        .collect();

    // Write each entry to its shard
    for (key, &count) in counts {
        let shard = shard_for_key(key, num_shards);
        let w = &mut writers[shard];
        write_str(w, &key.bucket)?;
        write_u8(w, key.n)?;
        write_str(w, &key.ngram)?;
        write_u32(w, count)?;
    }

    // Flush and rename all shards atomically
    for (shard, writer) in writers.into_iter().enumerate() {
        drop(writer); // flush via BufWriter drop
        let tmp = shard_tmp_path(data_dir, index, shard, num_shards);
        let final_path = shard_path(data_dir, index, shard, num_shards);
        std::fs::rename(&tmp, &final_path)?;
    }

    Ok(())
}

// ============================================================================
// Parallel shard merge
// ============================================================================

/// Read all binary records from a single file into a HashMap, summing counts.
fn read_shard_file(
    path: &Path,
    map: &mut HashMap<NgramKey, u32>,
    bucket_intern: &mut HashMap<String, Arc<str>>,
) -> anyhow::Result<()> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut str_buf = Vec::new();

    loop {
        let bucket_raw = match read_str(&mut reader, &mut str_buf)? {
            Some(s) => s,
            None => break, // EOF
        };
        let n = read_u8(&mut reader)?;
        let ngram = read_str(&mut reader, &mut str_buf)?
            .ok_or_else(|| anyhow::anyhow!("Unexpected EOF reading ngram"))?;
        let count = read_u32(&mut reader)?;

        // Intern the bucket string to share Arc allocations
        let bucket: Arc<str> = bucket_intern
            .entry(bucket_raw)
            .or_insert_with_key(|k| Arc::from(k.as_str()))
            .clone();

        let key = NgramKey { bucket, n, ngram };
        *map.entry(key).or_insert(0) += count;
    }

    Ok(())
}

/// Merge all sharded partial files in parallel.
///
/// Returns one HashMap per shard. Shards are disjoint by key, so no
/// cross-shard aggregation is needed.
pub fn merge_shards_parallel(
    data_dir: &Path,
    num_shards: usize,
) -> anyhow::Result<Vec<HashMap<NgramKey, u32>>> {
    let completed = Arc::new(AtomicUsize::new(0));

    let results: Vec<anyhow::Result<HashMap<NgramKey, u32>>> = (0..num_shards)
        .into_par_iter()
        .map(|shard| {
            let files = find_shard_files(data_dir, shard, num_shards)?;
            let mut map: HashMap<NgramKey, u32> = HashMap::new();
            let mut bucket_intern: HashMap<String, Arc<str>> = HashMap::new();

            for file in &files {
                read_shard_file(file, &mut map, &mut bucket_intern)?;
            }

            let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
            tracing::info!("  Merge: {}/{} shards complete", done, num_shards);

            Ok(map)
        })
        .collect();

    // Collect results, propagating any errors
    results.into_iter().collect()
}
