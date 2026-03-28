//! HN N-gram ingestion pipeline (RFC-004)
//!
//! Subcommands:
//!   download   — fetch Parquet files from HuggingFace
//!   ingest     — single-pass: tokenize, update vocabulary, insert counts
//!   vocabulary — (legacy) pass 1: build vocabulary from global counts
//!   backfill   — (legacy) pass 2: generate daily aggregates, insert to ClickHouse
//!   status     — show manifest state

mod backfill;
mod download;
mod ingest;
mod manifest;
mod months;
mod parquet;
mod vocabulary;

use anyhow::Context;
use clap::{Parser, Subcommand};
use hn_clickhouse::HnClickHouse;
use manifest::Manifest;
use months::{month_range, YearMonth};
use std::path::PathBuf;
use time::OffsetDateTime;
use tokenizer::TOKENIZER_VERSION;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser)]
#[command(name = "ingestion", about = "HN N-gram ingestion pipeline (RFC-004)")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Download Parquet files from HuggingFace
    Download {
        /// Local storage directory
        #[arg(long, default_value = "./data/hn")]
        data_dir: PathBuf,
        /// First month to download (YYYY-MM)
        #[arg(long, default_value = "2006-10")]
        start: String,
        /// Last month to download (YYYY-MM, default: current month)
        #[arg(long)]
        end: Option<String>,
    },

    /// Build vocabulary from global n-gram counts (pass 1)
    Vocabulary {
        /// Directory with downloaded Parquet files
        #[arg(long, default_value = "./data/hn")]
        data_dir: PathBuf,
        /// First month to process (YYYY-MM)
        #[arg(long, default_value = "2006-10")]
        start: String,
        /// Last month to process (YYYY-MM, default: current month)
        #[arg(long)]
        end: Option<String>,
    },

    /// Single-pass ingestion: tokenize, update vocabulary, insert counts
    Ingest {
        /// Directory with downloaded Parquet files
        #[arg(long, default_value = "./data/hn")]
        data_dir: PathBuf,
        /// First month to process (YYYY-MM)
        #[arg(long, default_value = "2006-10")]
        start: String,
        /// Last month to process (YYYY-MM, default: current month)
        #[arg(long)]
        end: Option<String>,
    },

    /// Generate daily aggregates and insert into ClickHouse (pass 2)
    Backfill {
        /// Directory with downloaded Parquet files
        #[arg(long, default_value = "./data/hn")]
        data_dir: PathBuf,
        /// First month to process (YYYY-MM)
        #[arg(long, default_value = "2006-10")]
        start: String,
        /// Last month to process (YYYY-MM, default: current month)
        #[arg(long)]
        end: Option<String>,
    },

    /// Show current manifest state
    Status {
        /// Data directory
        #[arg(long, default_value = "./data/hn")]
        data_dir: PathBuf,
    },
}

fn parse_end(end: &Option<String>) -> anyhow::Result<YearMonth> {
    match end {
        Some(s) => YearMonth::parse(s),
        None => Ok(YearMonth::now_utc()),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();

    let cli = Cli::parse();

    tracing::info!("HN N-gram ingestion pipeline");
    tracing::info!("Tokenizer version: {}", TOKENIZER_VERSION);

    match cli.command {
        Command::Download {
            data_dir,
            start,
            end,
        } => {
            let start = YearMonth::parse(&start)?;
            let end = parse_end(&end)?;
            let months = month_range(start, end);
            tracing::info!(
                "Downloading {} files ({} to {})",
                months.len(),
                start,
                end
            );
            download::download(&data_dir, &months).await?;
        }

        Command::Ingest {
            data_dir,
            start,
            end,
        } => {
            let start = YearMonth::parse(&start)?;
            let end = parse_end(&end)?;
            let months = month_range(start, end);

            let mut manifest = Manifest::load(&data_dir)?;
            manifest.validate_version()?;

            tracing::info!(
                "Ingesting {} months ({} to {})",
                months.len(),
                start,
                end
            );

            let ch = HnClickHouse::from_env();
            ch.ping().await.context("Cannot connect to ClickHouse — is it running?")?;
            ingest::ingest(&data_dir, &months, &mut manifest, &ch).await?;
        }

        Command::Vocabulary {
            data_dir,
            start,
            end,
        } => {
            let start = YearMonth::parse(&start)?;
            let end = parse_end(&end)?;
            let months = month_range(start, end);

            let mut manifest = Manifest::load(&data_dir)?;
            manifest.validate_version()?;

            tracing::info!(
                "Building vocabulary from {} files ({} to {})",
                months.len(),
                start,
                end
            );

            let ch = HnClickHouse::from_env();
            ch.ping().await.context("Cannot connect to ClickHouse — is it running?")?;
            vocabulary::build_vocabulary_phase(&data_dir, &months, &mut manifest, &ch).await?;
        }

        Command::Backfill {
            data_dir,
            start,
            end,
        } => {
            let start = YearMonth::parse(&start)?;
            let end = parse_end(&end)?;
            let months = month_range(start, end);

            let mut manifest = Manifest::load(&data_dir)?;
            manifest.validate_version()?;

            tracing::info!(
                "Backfilling {} files ({} to {})",
                months.len(),
                start,
                end
            );

            let ch = HnClickHouse::from_env();
            ch.ping().await.context("Cannot connect to ClickHouse — is it running?")?;
            backfill::backfill_phase(&data_dir, &months, &mut manifest, &ch).await?;
        }

        Command::Status { data_dir } => {
            let manifest = Manifest::load(&data_dir)?;
            println!("Manifest: {}", data_dir.join("manifest.json").display());
            println!("Tokenizer version: {}", manifest.tokenizer_version);
            println!("Vocabulary built: {}", manifest.vocabulary_built);
            if manifest.last_ingested_ts > 0 {
                let dt = OffsetDateTime::from_unix_timestamp_nanos(
                    (manifest.last_ingested_ts as i128) * 1_000_000,
                )
                .unwrap_or(OffsetDateTime::UNIX_EPOCH);
                println!("Watermark: {} ({})", manifest.last_ingested_ts, dt.date());
            } else {
                println!("Watermark: none");
            }
            println!("Phase 1 completed files: {}", manifest.phase1_count());
            println!("Phase 2 completed files: {}", manifest.phase2_count());
        }
    }

    Ok(())
}
