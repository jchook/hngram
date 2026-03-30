//! HN N-gram ingest pipeline (RFC-004)
//!
//! Subcommands:
//!   download — fetch Parquet files from HuggingFace
//!   process  — tokenize, count, prune → ClickHouse (default) or Parquet files
//!   import   — load Parquet output into ClickHouse with atomic table swap

mod config;
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
#[command(name = "ingest", about = "HN N-gram ingest pipeline (RFC-004)")]
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
        /// First month to process (YYYY-MM, overrides config file, default: 2006-10)
        #[arg(long)]
        start: Option<String>,
        /// Last month to process (YYYY-MM, overrides config file, default: current month)
        #[arg(long)]
        end: Option<String>,
        /// Output mode: "clickhouse" or "parquet" (overrides config file)
        #[arg(long)]
        output: Option<OutputArg>,
        /// Flush threshold: combined globals + counts entries (overrides config file)
        #[arg(long)]
        max_entries: Option<usize>,
        /// Concurrent file processing workers (overrides config file)
        #[arg(long)]
        producer_count: Option<usize>,
        /// Number of shards for parallel merge (overrides config file)
        #[arg(long)]
        merge_shards: Option<usize>,
        /// Bucket granularity: daily, monthly, yearly (overrides config file)
        #[arg(long)]
        bucket_granularity: Option<config::BucketGranularity>,
        /// Path to TOML config file
        #[arg(long)]
        config: Option<PathBuf>,
    },

    /// Load Parquet output into ClickHouse with atomic table swap
    Import {
        /// Directory containing output/ folder
        #[arg(long, default_value = "./data/hn")]
        data_dir: PathBuf,
    },

    /// Drop and recreate all ClickHouse tables
    ResetDb,

    /// Full reset: drop/recreate tables + delete output/ and partial/ dirs
    Reset {
        /// Directory with downloaded Parquet files
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

    tracing::info!("HN N-gram ingest pipeline");
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
            merge_shards,
            bucket_granularity,
            config,
        } => {
            // Load TOML config file if provided
            let toml_config = match &config {
                Some(path) => {
                    let cfg = config::load(path)?;
                    tracing::info!("Loaded config from {}", path.display());
                    cfg
                }
                None => config::IngestConfig::default(),
            };
            let toml_process = toml_config.process.unwrap_or_default();

            // Resolve date range: CLI > TOML > default
            let start_str = start
                .or(toml_process.start)
                .unwrap_or_else(|| "2006-10".to_string());
            let start = YearMonth::parse(&start_str)?;
            let end_str = end.or(toml_process.end);
            let end = parse_end(&end_str)?;
            let months = month_range(start, end);

            // Resolve: CLI arg > TOML > default
            let defaults = process::ProcessConfig::default();

            let mode = match output {
                Some(OutputArg::Clickhouse) => OutputMode::ClickHouse,
                Some(OutputArg::Parquet) => OutputMode::Parquet,
                None => match toml_process.output.as_deref() {
                    Some("parquet") => OutputMode::Parquet,
                    Some("clickhouse") | None => OutputMode::ClickHouse,
                    Some(other) => anyhow::bail!("Unknown output mode '{}' in config file", other),
                },
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
                max_entries: max_entries
                    .or(toml_process.max_entries)
                    .unwrap_or(defaults.max_entries),
                producer_count: producer_count
                    .or(toml_process.producer_count)
                    .unwrap_or(defaults.producer_count),
                merge_shards: merge_shards
                    .or(toml_process.merge_shards)
                    .unwrap_or(defaults.merge_shards),
                bucket_granularity: bucket_granularity
                    .or(toml_process.bucket_granularity)
                    .unwrap_or(defaults.bucket_granularity),
                prune: toml_process.prune,
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

        Command::ResetDb => {
            let ch = HnClickHouse::from_env();
            ch.ping()
                .await
                .context("Cannot connect to ClickHouse — is it running?")?;
            reset_db(&ch).await?;
        }

        Command::Reset { data_dir } => {
            let ch = HnClickHouse::from_env();
            ch.ping()
                .await
                .context("Cannot connect to ClickHouse — is it running?")?;
            reset_db(&ch).await?;

            let output_dir = data_dir.join("output");
            if output_dir.exists() {
                tracing::info!("Removing {}", output_dir.display());
                std::fs::remove_dir_all(&output_dir)?;
            }
            let partial_dir = data_dir.join("partial");
            if partial_dir.exists() {
                tracing::info!("Removing {}", partial_dir.display());
                std::fs::remove_dir_all(&partial_dir)?;
            }
            tracing::info!("Full reset complete");
        }
    }

    Ok(())
}

use hn_clickhouse::{
    TABLE_BUCKET_TOTALS, TABLE_GLOBAL_COUNTS, TABLE_INGEST_LOG, TABLE_NGRAM_COUNTS,
    TABLE_NGRAM_VOCABULARY,
};

const ALL_TABLES: [&str; 5] = [
    TABLE_NGRAM_COUNTS,
    TABLE_BUCKET_TOTALS,
    TABLE_NGRAM_VOCABULARY,
    TABLE_GLOBAL_COUNTS,
    TABLE_INGEST_LOG,
];

async fn reset_db(ch: &HnClickHouse) -> anyhow::Result<()> {
    for table in &ALL_TABLES {
        tracing::info!("Dropping {}", table);
        ch.execute_sql(&format!("DROP TABLE IF EXISTS {}", table)).await?;
    }
    tracing::info!("Recreating schema...");
    // Re-run schema DDL statements individually.
    // Strip SQL comments before splitting on ';' so that comment blocks
    // preceding a statement don't cause the whole chunk to be skipped.
    let schema = include_str!("../../../etc/clickhouse/init/001-schema.sql");
    let stripped: String = schema
        .lines()
        .filter(|line| !line.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    for stmt in stripped.split(';') {
        let stmt = stmt.trim();
        if stmt.is_empty() {
            continue;
        }
        ch.execute_sql(stmt).await?;
    }
    tracing::info!("Database reset complete");
    Ok(())
}
