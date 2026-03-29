//! HN N-gram ingestion pipeline (RFC-004)
//!
//! Subcommands:
//!   download — fetch Parquet files from HuggingFace
//!   process  — tokenize, count, prune → ClickHouse (default) or Parquet files
//!   import   — load Parquet output into ClickHouse with atomic table swap

mod download;
mod import;
mod months;
mod parquet;
mod process;
mod vocabulary;

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use hn_clickhouse::HnClickHouse;
use months::{month_range, YearMonth};
use process::OutputMode;
use std::path::PathBuf;
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

    /// Tokenize, count, prune, and output results
    Process {
        /// Directory with downloaded Parquet files
        #[arg(long, default_value = "./data/hn")]
        data_dir: PathBuf,
        /// First month to process (YYYY-MM)
        #[arg(long, default_value = "2006-10")]
        start: String,
        /// Last month to process (YYYY-MM, default: current month)
        #[arg(long)]
        end: Option<String>,
        /// Output mode: "clickhouse" (default) or "parquet"
        #[arg(long, default_value = "clickhouse")]
        output: OutputArg,
        /// Flush threshold: combined globals + counts entries (parquet mode)
        #[arg(long, default_value = "20000000")]
        max_entries: usize,
        /// Concurrent file processing workers
        #[arg(long, default_value = "2")]
        producer_count: usize,
        /// Path to TOML config file for pruning thresholds
        #[arg(long)]
        config: Option<PathBuf>,
    },

    /// Load Parquet output into ClickHouse with atomic table swap
    Import {
        /// Directory containing output/ folder
        #[arg(long, default_value = "./data/hn")]
        data_dir: PathBuf,
    },
}

#[derive(Debug, Clone, ValueEnum)]
enum OutputArg {
    Clickhouse,
    Parquet,
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

        Command::Process {
            data_dir,
            start,
            end,
            output,
            max_entries,
            producer_count,
            config,
        } => {
            let start = YearMonth::parse(&start)?;
            let end = parse_end(&end)?;
            let months = month_range(start, end);

            let mode = match output {
                OutputArg::Clickhouse => OutputMode::ClickHouse,
                OutputArg::Parquet => OutputMode::Parquet,
            };

            let ch = if mode == OutputMode::ClickHouse {
                let ch = HnClickHouse::from_env();
                ch.ping()
                    .await
                    .context("Cannot connect to ClickHouse — is it running?")?;
                Some(ch)
            } else {
                None
            };

            tracing::info!(
                "Processing {} months ({} to {}) → {:?}",
                months.len(),
                start,
                end,
                mode,
            );

            let proc_config = process::ProcessConfig {
                max_entries,
                producer_count,
                config_path: config,
            };
            process::process(&data_dir, &months, &start, &end, mode, ch.as_ref(), &proc_config)
                .await?;
        }

        Command::Import { data_dir } => {
            let ch = HnClickHouse::from_env();
            ch.ping()
                .await
                .context("Cannot connect to ClickHouse — is it running?")?;
            import::import(&data_dir, &ch).await?;
        }
    }

    Ok(())
}
