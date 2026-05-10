//! HN N-gram API types and handlers (RFC-005)

pub use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
pub use hn_clickhouse::{
    default_start_date, Granularity, HnClickHouse, MAX_NGRAM_ORDER, MAX_PHRASES,
    TOKENIZER_VERSION,
};
pub use moka::future::Cache;
pub use serde::{Deserialize, Serialize};
pub use serde_json::json;
pub use std::sync::Arc;
pub use std::time::Duration;
pub use time::macros::format_description;
pub use time::Date;
pub use tower::limit::ConcurrencyLimitLayer;
pub use tower_http::{cors::CorsLayer, trace::TraceLayer};
pub use utoipa::{IntoParams, OpenApi, ToSchema};
pub use utoipa_scalar::{Scalar, Servable};

// ============================================================================
// OpenAPI Documentation
// ============================================================================

#[derive(OpenApi)]
#[openapi(
    paths(health, ngram, freshness),
    components(schemas(
        HealthResponse,
        FreshnessResponse,
        QueryParams,
        QueryResponse,
        QueryMeta,
        SeriesStatus,
        Point,
        ErrorResponse,
        ErrorDetail
    ))
)]
pub struct ApiDoc;

// ============================================================================
// Application State
// ============================================================================

pub struct AppState {
    pub clickhouse: HnClickHouse,
    /// Hot-path response cache for /ngram. On hit, the response is served
    /// without touching ClickHouse. TTL matches the Cache-Control header.
    pub cache: NgramResponseCache,
}

/// Cache key for a fully-resolved /ngram request. Includes everything that
/// changes the response so popular landing-page queries land on the same key.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct NgramCacheKey {
    pub n: u8,
    pub phrase: String,
    pub start: Date,
    pub end: Date,
    pub granularity: Granularity,
}

pub type NgramResponseCache = Cache<NgramCacheKey, Arc<QueryResponse>>;

/// Build a fresh response cache. ~500 entries with a 1h TTL — comfortably
/// covers the landing-page query and a few hundred popular variants without
/// blowing the API container's memory budget.
pub fn build_response_cache() -> NgramResponseCache {
    Cache::builder()
        .max_capacity(500)
        .time_to_live(Duration::from_secs(3600))
        .build()
}

// ============================================================================
// Router
// ============================================================================

pub fn api_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/freshness", get(freshness))
        .route("/ngram", get(ngram))
        .merge(Scalar::with_url("/scalar", ApiDoc::openapi()))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .layer(ConcurrencyLimitLayer::new(64))
        .with_state(state)
}

// ============================================================================
// Health Endpoint
// ============================================================================

#[derive(Serialize, ToSchema)]
pub struct HealthResponse {
    #[schema(example = "ok")]
    status: String,
    #[schema(example = "0.1.0")]
    version: String,
    #[schema(example = "1")]
    tokenizer_version: String,
}

#[utoipa::path(
    get,
    path = "/health",
    responses(
        (status = 200, description = "Service is healthy", body = HealthResponse)
    )
)]
pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        tokenizer_version: TOKENIZER_VERSION.to_string(),
    })
}

// ============================================================================
// Freshness Endpoint
// ============================================================================

#[derive(Serialize, ToSchema)]
pub struct FreshnessResponse {
    /// Date of the most recent comment ingested (YYYY-MM-DD), or null if no data.
    #[schema(example = "2026-04-30")]
    last_ingested_date: Option<String>,
    /// Unix timestamp in milliseconds for the most recent comment ingested, or null.
    #[schema(example = 1745977200000_i64)]
    last_ingested_ts: Option<i64>,
    #[schema(example = "1")]
    tokenizer_version: String,
}

#[utoipa::path(
    get,
    path = "/freshness",
    responses(
        (status = 200, description = "Data freshness info", body = FreshnessResponse)
    )
)]
pub async fn freshness(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let ts = state
        .clickhouse
        .get_latest_watermark()
        .await
        .ok()
        .flatten();
    let date = ts.and_then(|ms| {
        time::OffsetDateTime::from_unix_timestamp(ms / 1000)
            .ok()
            .map(|dt| format_date(dt.date()))
    });
    (
        StatusCode::OK,
        [(header::CACHE_CONTROL, "public, max-age=3600")],
        Json(FreshnessResponse {
            last_ingested_date: date,
            last_ingested_ts: ts,
            tokenizer_version: TOKENIZER_VERSION.to_string(),
        }),
    )
}

// ============================================================================
// Query Endpoint - Types
// ============================================================================

/// Query a single phrase's n-gram frequency over time.
/// The client makes one request per phrase (up to MAX_PHRASES in parallel).
#[derive(Deserialize, ToSchema, IntoParams)]
pub struct QueryParams {
    /// Single phrase to query
    #[param(example = "rust programming")]
    phrase: String,
    /// Start date (YYYY-MM-DD). Default: 2011-01-01
    #[param(example = "2020-01-01")]
    start: Option<String>,
    /// End date (YYYY-MM-DD). Default: today
    #[param(example = "2024-12-01")]
    end: Option<String>,
    /// Time granularity: day, week, month, year. Default: month
    #[param(example = "month")]
    granularity: Option<String>,
}

#[derive(Serialize, ToSchema)]
#[schema(example = json!({
    "phrase": "rust programming",
    "normalized": "rust programming",
    "status": "indexed",
    "points": [
        {"t": "2020-01-01", "v": 0.0000812, "count": 98,  "total": 1206897},
        {"t": "2020-07-01", "v": 0.0001034, "count": 124, "total": 1199226},
        {"t": "2021-01-01", "v": 0.0001305, "count": 159, "total": 1218391},
        {"t": "2024-06-01", "v": 0.0001501, "count": 187, "total": 1245836}
    ],
    "global_count": 18432,
    "meta": {
        "tokenizer_version": "1",
        "start": "2020-01-01",
        "end": "2024-12-01",
        "granularity": "month"
    }
}))]
pub struct QueryResponse {
    /// Original phrase from input
    phrase: String,
    /// Normalized/tokenized form used for lookup
    normalized: String,
    /// Status of this phrase
    status: SeriesStatus,
    /// Sparse data points (only non-zero buckets). Client handles zero-fill.
    points: Vec<Point>,
    /// Total occurrences of this phrase across all time
    global_count: u64,
    meta: QueryMeta,
}

#[derive(Serialize, ToSchema)]
pub struct QueryMeta {
    #[schema(example = "1")]
    tokenizer_version: String,
    #[schema(example = "2020-01-01")]
    start: String,
    #[schema(example = "2024-12-01")]
    end: String,
    #[schema(example = "month")]
    granularity: String,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(example = "indexed")]
pub enum SeriesStatus {
    /// Phrase is in vocabulary, data returned
    Indexed,
    /// Phrase is valid but not in vocabulary (too rare)
    NotIndexed,
    /// Phrase failed validation (e.g., > 3 tokens, empty)
    Invalid,
}

#[derive(Serialize, ToSchema)]
pub struct Point {
    /// Bucket timestamp (YYYY-MM-DD)
    #[schema(example = "2024-06-01")]
    t: String,
    /// Relative frequency (count / total_count)
    #[schema(example = 0.0001501)]
    v: f64,
    /// Raw occurrence count for this phrase in this bucket
    #[schema(example = 187)]
    count: u64,
    /// Total n-grams of this order in this bucket
    #[schema(example = 1245836)]
    total: u64,
}

// ============================================================================
// Error Response Types (RFC-005 Section 9)
// ============================================================================

#[derive(Serialize, ToSchema)]
#[schema(example = json!({
    "error": {
        "code": "INVALID_DATE_FORMAT",
        "message": "Invalid start date '2020/01/01', expected YYYY-MM-DD"
    }
}))]
pub struct ErrorResponse {
    error: ErrorDetail,
}

#[derive(Serialize, ToSchema)]
pub struct ErrorDetail {
    #[schema(example = "INVALID_DATE_FORMAT")]
    code: String,
    #[schema(example = "Invalid start date '2020/01/01', expected YYYY-MM-DD")]
    message: String,
}

impl ErrorResponse {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            error: ErrorDetail {
                code: code.to_string(),
                message: message.into(),
            },
        }
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn parse_date(s: &str) -> Option<Date> {
    let format = format_description!("[year]-[month]-[day]");
    Date::parse(s, format).ok()
}

fn format_date(date: Date) -> String {
    date.to_string()
}

/// Snap a date down to the first of its month. Used for cache-key alignment:
/// data is monthly-granular at typical query paths, so dates within the same
/// month should share a cache entry.
fn snap_to_month_start(d: Date) -> Date {
    d.replace_day(1).expect("first of month is always valid")
}

/// Snap a date up to the last day of its month (inclusive). Pairs with
/// `snap_to_month_start` so that a user-picked range like 2024-03-15 → 2024-05-09
/// becomes 2024-03-01 → 2024-05-31 — fully covering all months touched by the
/// range and collapsing intra-month variation onto one cache key.
fn snap_to_month_end(d: Date) -> Date {
    let first_of_next = if d.month() == time::Month::December {
        Date::from_calendar_date(d.year() + 1, time::Month::January, 1)
    } else {
        Date::from_calendar_date(d.year(), d.month().next(), 1)
    }
    .expect("first of next month is always valid");
    first_of_next - time::Duration::days(1)
}

fn ngram_order(tokens: &[String]) -> Option<u8> {
    let len = tokens.len();
    if len >= 1 && len <= MAX_NGRAM_ORDER as usize {
        Some(len as u8)
    } else {
        None
    }
}

// ============================================================================
// Query Endpoint Handler
// ============================================================================

#[utoipa::path(
    get,
    path = "/ngram",
    params(QueryParams),
    responses(
        (status = 200, description = "Query result for a single phrase", body = QueryResponse),
        (status = 400, description = "Invalid query parameters", body = ErrorResponse)
    )
)]
pub async fn ngram(
    State(state): State<Arc<AppState>>,
    Query(params): Query<QueryParams>,
) -> impl IntoResponse {
    let phrase = params.phrase.trim();

    if phrase.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new("MISSING_PHRASE", "No phrase provided")),
        )
            .into_response();
    }

    // Parse date range
    let start = match &params.start {
        Some(s) => match parse_date(s) {
            Some(d) => d,
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse::new(
                        "INVALID_DATE_FORMAT",
                        format!("Invalid start date '{}', expected YYYY-MM-DD", s),
                    )),
                )
                    .into_response();
            }
        },
        None => default_start_date(),
    };

    let end = match &params.end {
        Some(s) => match parse_date(s) {
            Some(d) => d,
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse::new(
                        "INVALID_DATE_FORMAT",
                        format!("Invalid end date '{}', expected YYYY-MM-DD", s),
                    )),
                )
                    .into_response();
            }
        },
        None => time::OffsetDateTime::now_utc().date(),
    };

    // Snap to month boundaries before validation. Two effects:
    //   1. Cache cardinality drops dramatically — every date within the same
    //      month maps to the same cache key, so e.g. 2024-03-15 and 2024-03-22
    //      share a cached response.
    //   2. UI date pickers can present month-only granularity without losing
    //      precision (data is monthly-granular at typical aggregations anyway).
    let start = snap_to_month_start(start);
    let end = snap_to_month_end(end);

    if start > end {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(
                "INVALID_DATE_RANGE",
                format!("Start date {} is after end date {}", start, end),
            )),
        )
            .into_response();
    }

    // Parse granularity
    let granularity = match &params.granularity {
        Some(s) => match Granularity::from_str(s) {
            Some(g) => g,
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse::new(
                        "INVALID_GRANULARITY",
                        format!(
                            "Invalid granularity '{}', expected day, week, month, or year",
                            s
                        ),
                    )),
                )
                    .into_response();
            }
        },
        None => Granularity::default(),
    };

    let meta = QueryMeta {
        tokenizer_version: TOKENIZER_VERSION.to_string(),
        start: format_date(start),
        end: format_date(end),
        granularity: format!("{:?}", granularity).to_lowercase(),
    };

    // Tokenize the phrase
    let tokens = tokenizer::tokenize(phrase);
    let normalized = tokens.join(" ");

    let n = match ngram_order(&tokens) {
        Some(n) => n,
        None => {
            return (
                StatusCode::OK,
                [(header::CACHE_CONTROL, "public, max-age=3600")],
                Json(QueryResponse {
                    phrase: phrase.to_string(),
                    normalized,
                    status: SeriesStatus::Invalid,
                    points: vec![],
                    global_count: 0,
                    meta,
                }),
            )
                .into_response();
        }
    };

    // Single-flight cache lookup: concurrent misses on the same key dedupe to
    // one ClickHouse query. Prevents thundering-herd on TTL expiry — important
    // under traffic spikes (HN front page) where many visitors hit the same
    // popular phrases simultaneously.
    let cache_key = NgramCacheKey {
        n,
        phrase: normalized.clone(),
        start,
        end,
        granularity,
    };

    let phrase_owned = phrase.to_string();
    let normalized_owned = normalized;
    let clickhouse = state.clickhouse.clone();

    let result: Result<Arc<QueryResponse>, Arc<String>> = state
        .cache
        .try_get_with(cache_key, async move {
            // Main timeseries query
            let points_result: Result<Vec<Point>, _> = match granularity {
                Granularity::Day => clickhouse
                    .query_ngrams(n, &[normalized_owned.clone()], start, end)
                    .await
                    .map(|rows| {
                        rows.into_iter()
                            .filter_map(|r| {
                                if r.total_count > 0 && r.count > 0 {
                                    Some(Point {
                                        t: format_date(r.bucket),
                                        v: r.count as f64 / r.total_count as f64,
                                        count: r.count as u64,
                                        total: r.total_count,
                                    })
                                } else {
                                    None
                                }
                            })
                            .collect()
                    }),
                _ => clickhouse
                    .query_ngrams_aggregated(
                        n,
                        &[normalized_owned.clone()],
                        start,
                        end,
                        granularity,
                    )
                    .await
                    .map(|rows| {
                        rows.into_iter()
                            .filter_map(|r| {
                                if r.sum_total > 0 && r.sum_count > 0 {
                                    Some(Point {
                                        t: format_date(r.bucket),
                                        v: r.sum_count as f64 / r.sum_total as f64,
                                        count: r.sum_count,
                                        total: r.sum_total,
                                    })
                                } else {
                                    None
                                }
                            })
                            .collect()
                    }),
            };

            let points = points_result.map_err(|e| format!("ClickHouse query failed: {}", e))?;
            let global_count = clickhouse
                .get_global_count(n, &normalized_owned)
                .await
                .unwrap_or(0);

            // Indexing status: unigrams are never pruned; for higher-order
            // ngrams, check the vocabulary table when the result was empty.
            let status = if !points.is_empty() || n == 1 {
                SeriesStatus::Indexed
            } else {
                match clickhouse.is_in_vocabulary(n, &normalized_owned).await {
                    Ok(true) => SeriesStatus::Indexed,
                    _ => SeriesStatus::NotIndexed,
                }
            };

            Ok::<Arc<QueryResponse>, String>(Arc::new(QueryResponse {
                phrase: phrase_owned,
                normalized: normalized_owned,
                status,
                points,
                global_count,
                meta,
            }))
        })
        .await;

    match result {
        Ok(arc) => (
            StatusCode::OK,
            [(header::CACHE_CONTROL, "public, max-age=3600")],
            Json(arc),
        )
            .into_response(),
        Err(err_arc) => {
            tracing::warn!("ngram query failed for '{}': {}", phrase, err_arc);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("INTERNAL_ERROR", "Query failed")),
            )
                .into_response()
        }
    }
}
