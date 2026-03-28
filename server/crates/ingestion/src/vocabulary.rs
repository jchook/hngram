//! Vocabulary utilities: partial count file I/O and merging.

use crate::months::YearMonth;
use anyhow::Context;
use std::collections::HashMap;
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

/// Write partial global counts to a TSV file: n\tngram\tcount\n
pub fn write_partial_counts(
    path: &Path,
    counts: &HashMap<(u8, String), u64>,
) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("counts.tmp");
    let mut file = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
    for ((n, ngram), count) in counts {
        writeln!(file, "{}\t{}\t{}", n, ngram, count)?;
    }
    file.flush()?;
    drop(file);
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Read and merge all partial count files into global counts.
pub fn merge_partial_counts(
    data_dir: &Path,
    months: &[YearMonth],
) -> anyhow::Result<HashMap<(u8, String), u64>> {
    let mut global: HashMap<(u8, String), u64> = HashMap::new();

    for ym in months {
        let path = partial_path(data_dir, ym);
        if !path.exists() {
            continue;
        }

        let file = std::io::BufReader::new(
            std::fs::File::open(&path)
                .with_context(|| format!("Failed to open {}", path.display()))?,
        );

        for line in file.lines() {
            let line = line?;
            let parts: Vec<&str> = line.splitn(3, '\t').collect();
            if parts.len() != 3 {
                continue;
            }
            let n: u8 = parts[0].parse().unwrap_or(0);
            let ngram = parts[1].to_string();
            let count: u64 = parts[2].parse().unwrap_or(0);
            if n >= 1 && n <= 3 && count > 0 {
                *global.entry((n, ngram)).or_insert(0) += count;
            }
        }
    }

    Ok(global)
}
