//! Phase 0: Download Parquet files from HuggingFace (RFC-004 §2.1).

use crate::months::YearMonth;
use anyhow::Context;
use futures_util::StreamExt;
use std::path::Path;

/// Download Parquet files for the given months to data_dir.
/// Skips files that already exist locally.
pub async fn download(data_dir: &Path, months: &[YearMonth]) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .user_agent("hngram-ingest/0.1")
        .build()?;

    let total = months.len();

    for (i, ym) in months.iter().enumerate() {
        let rel_path = ym.file_path();
        let local_path = data_dir.join(&rel_path);

        // Skip if already downloaded
        if local_path.exists() {
            tracing::debug!("Skipping {} (already exists)", rel_path);
            continue;
        }

        let url = ym.download_url();
        tracing::info!("Downloading {} ({}/{})", rel_path, i + 1, total);

        let start = std::time::Instant::now();

        let response = match client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("Failed to fetch {}: {}", url, e);
                continue;
            }
        };

        if !response.status().is_success() {
            tracing::warn!(
                "HTTP {} for {} — skipping (file may not exist for this month)",
                response.status(),
                rel_path
            );
            continue;
        }

        // Create parent directories
        if let Some(parent) = local_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Stream to temp file, then rename
        let tmp_path = local_path.with_extension("parquet.tmp");
        let mut file = tokio::fs::File::create(&tmp_path)
            .await
            .with_context(|| format!("Failed to create {}", tmp_path.display()))?;

        let mut stream = response.bytes_stream();
        let mut downloaded: u64 = 0;

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.with_context(|| format!("Download error for {}", rel_path))?;
            downloaded += chunk.len() as u64;
            tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await?;
        }

        tokio::io::AsyncWriteExt::flush(&mut file).await?;
        drop(file);

        tokio::fs::rename(&tmp_path, &local_path).await?;

        let elapsed = start.elapsed();
        let mb = downloaded as f64 / (1024.0 * 1024.0);
        tracing::info!(
            "  {:.1} MB in {:.1}s ({:.1} MB/s)",
            mb,
            elapsed.as_secs_f64(),
            mb / elapsed.as_secs_f64()
        );
    }

    tracing::info!("Download complete");
    Ok(())
}
