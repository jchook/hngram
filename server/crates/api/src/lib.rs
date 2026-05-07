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
pub use serde::{Deserialize, Serialize};
pub use serde_json::json;
pub use std::sync::Arc;
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

    // Query ClickHouse — server computes relative frequency
    let query_result = match granularity {
        Granularity::Day => state
            .clickhouse
            .query_ngrams(n, &[normalized.clone()], start, end)
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
                    .collect::<Vec<Point>>()
            }),
        _ => state
            .clickhouse
            .query_ngrams_aggregated(n, &[normalized.clone()], start, end, granularity)
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
                    .collect::<Vec<Point>>()
            }),
    };

    // Fetch global count for this phrase
    let global_count = state
        .clickhouse
        .get_global_count(n, &normalized)
        .await
        .unwrap_or(0);

    match query_result {
        Ok(points) if points.is_empty() => {
            // Determine status: unigrams are always indexed (never pruned).
            // For bigrams/trigrams, check the vocabulary table.
            let status = if n == 1 {
                SeriesStatus::Indexed
            } else {
                match state.clickhouse.is_in_vocabulary(n, &normalized).await {
                    Ok(true) => SeriesStatus::Indexed,
                    _ => SeriesStatus::NotIndexed,
                }
            };
            (
                StatusCode::OK,
                [(header::CACHE_CONTROL, "public, max-age=3600")],
                Json(QueryResponse {
                    phrase: phrase.to_string(),
                    normalized,
                    status,
                    points: vec![],
                    global_count,
                    meta,
                }),
            )
                .into_response()
        }
        Ok(points) => (
            StatusCode::OK,
            [(header::CACHE_CONTROL, "public, max-age=3600")],
            Json(QueryResponse {
                phrase: phrase.to_string(),
                normalized,
                status: SeriesStatus::Indexed,
                points,
                global_count,
                meta,
            }),
        )
            .into_response(),
        Err(e) => {
            tracing::warn!("ClickHouse query failed for '{}': {}", phrase, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("INTERNAL_ERROR", "Query failed")),
            )
                .into_response()
        }
    }
}
