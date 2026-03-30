//! Month range utilities for iterating over YYYY-MM file partitions.

use anyhow::{bail, Context};
use time::macros::format_description;

/// A year-month pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct YearMonth {
    pub year: i32,
    pub month: u8,
}

impl YearMonth {
    #[cfg(test)]
    pub fn new(year: i32, month: u8) -> Self {
        Self { year, month }
    }

    /// Parse from "YYYY-MM" string.
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() != 2 {
            bail!("Invalid YYYY-MM format: '{}'", s);
        }
        let year: i32 = parts[0].parse().context("Invalid year")?;
        let month: u8 = parts[1].parse().context("Invalid month")?;
        if !(1..=12).contains(&month) {
            bail!("Month must be 1-12, got {}", month);
        }
        Ok(Self { year, month })
    }

    /// Get the current month in UTC.
    pub fn now_utc() -> Self {
        let now = time::OffsetDateTime::now_utc();
        Self {
            year: now.year(),
            month: now.month() as u8,
        }
    }

    /// Advance to the next month.
    pub fn next(self) -> Self {
        if self.month == 12 {
            Self {
                year: self.year + 1,
                month: 1,
            }
        } else {
            Self {
                year: self.year,
                month: self.month + 1,
            }
        }
    }

    /// Relative Parquet file path: `data/YYYY/YYYY-MM.parquet`
    pub fn file_path(&self) -> String {
        format!("data/{}/{}-{:02}.parquet", self.year, self.year, self.month)
    }

    /// HuggingFace download URL.
    pub fn download_url(&self) -> String {
        format!(
            "https://huggingface.co/datasets/open-index/hacker-news/resolve/main/{}",
            self.file_path()
        )
    }

}

impl std::fmt::Display for YearMonth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{:02}", self.year, self.month)
    }
}

/// Parse a "YYYY-MM-DD" bucket string into a `time::Date`.
pub fn parse_bucket_date(s: &str) -> anyhow::Result<time::Date> {
    let format = format_description!("[year]-[month]-[day]");
    time::Date::parse(s, format).with_context(|| format!("Invalid bucket date: '{}'", s))
}

/// Generate an inclusive range of months from start to end.
pub fn month_range(start: YearMonth, end: YearMonth) -> Vec<YearMonth> {
    let mut months = Vec::new();
    let mut current = start;
    while current <= end {
        months.push(current);
        current = current.next();
    }
    months
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid() {
        let ym = YearMonth::parse("2024-01").unwrap();
        assert_eq!(ym, YearMonth::new(2024, 1));
    }

    #[test]
    fn parse_december() {
        let ym = YearMonth::parse("2006-12").unwrap();
        assert_eq!(ym, YearMonth::new(2006, 12));
    }

    #[test]
    fn parse_invalid() {
        assert!(YearMonth::parse("2024").is_err());
        assert!(YearMonth::parse("2024-13").is_err());
        assert!(YearMonth::parse("2024-00").is_err());
        assert!(YearMonth::parse("abc-01").is_err());
    }

    #[test]
    fn next_month() {
        assert_eq!(YearMonth::new(2024, 1).next(), YearMonth::new(2024, 2));
        assert_eq!(YearMonth::new(2024, 12).next(), YearMonth::new(2025, 1));
    }

    #[test]
    fn file_path() {
        assert_eq!(
            YearMonth::new(2024, 1).file_path(),
            "data/2024/2024-01.parquet"
        );
    }

    #[test]
    fn range() {
        let months = month_range(YearMonth::new(2024, 10), YearMonth::new(2025, 2));
        assert_eq!(months.len(), 5);
        assert_eq!(months[0], YearMonth::new(2024, 10));
        assert_eq!(months[4], YearMonth::new(2025, 2));
    }

    #[test]
    fn range_single() {
        let months = month_range(YearMonth::new(2024, 6), YearMonth::new(2024, 6));
        assert_eq!(months.len(), 1);
    }
}
