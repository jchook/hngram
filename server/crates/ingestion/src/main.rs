//! HN N-gram ingestion pipeline
//!
//! Fetches HN comments from HuggingFace, tokenizes them, generates n-grams,
//! and inserts aggregated counts into ClickHouse.
//!
//! See RFC-004 for specification.

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("HN N-gram ingestion pipeline");
    tracing::info!("Tokenizer version: {}", tokenizer::TOKENIZER_VERSION);

    // TODO: Implement per RFC-004
    // 1. Fetch Parquet files from HuggingFace
    // 2. Parse and filter comments
    // 3. Tokenize and generate n-grams
    // 4. Aggregate counts locally
    // 5. Insert to ClickHouse

    tracing::warn!("Ingestion pipeline not yet implemented");

    Ok(())
}
