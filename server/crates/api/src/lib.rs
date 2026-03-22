//! HN N-gram API types and handlers (RFC-005)

pub use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
pub use hn_clickhouse::{
    default_start_date, HnClickHouse, MAX_NGRAM_ORDER, MAX_PHRASES,
    TOKENIZER_VERSION,
};
pub use serde::{Deserialize, Serialize};
pub use std::collections::HashMap;
pub use std::sync::Arc;
pub use time::macros::format_description;
pub use time::Date;
pub use tower_http::{cors::CorsLayer, trace::TraceLayer};
pub use utoipa::{IntoParams, OpenApi, ToSchema};

// ============================================================================
// OpenAPI Documentation
// ============================================================================

#[derive(OpenApi)]
#[openapi(
    paths(health, query, totals),
    components(schemas(
        HealthResponse,
        QueryParams,
        QueryResponse,
        QueryMeta,
        Series,
        SeriesStatus,
        Point,
        TotalsParams,
        TotalsResponse,
        TotalsMeta,
        TotalSeries,
        TotalPoint,
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
        .route("/query", get(query))
        .route("/totals", get(totals))
        .merge(
            utoipa_swagger_ui::SwaggerUi::new("/swagger-ui")
                .url("/api-doc/openapi.json", ApiDoc::openapi()),
        )
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

// ============================================================================
// Health Endpoint
// ============================================================================

#[derive(Serialize, ToSchema)]
pub struct HealthResponse {
    status: String,
    version: String,
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
// Query Endpoint - Request Types
// ============================================================================

#[derive(Deserialize, ToSchema, IntoParams)]
pub struct QueryParams {
    /// Comma-separated phrases to query (max 10)
    phrases: String,
    /// Start date (YYYY-MM-DD). Default: 2011-01-01
    start: Option<String>,
    /// End date (YYYY-MM-DD). Default: today
    end: Option<String>,
}

// ============================================================================
// Query Endpoint - Response Types
// ============================================================================

#[derive(Serialize, ToSchema)]
pub struct QueryResponse {
    series: Vec<Series>,
    meta: QueryMeta,
}

#[derive(Serialize, ToSchema)]
pub struct QueryMeta {
    tokenizer_version: String,
    start: String,
    end: String,
}

#[derive(Serialize, ToSchema)]
pub struct Series {
    /// Original phrase from input
    phrase: String,
    /// Normalized/tokenized form used for lookup
    normalized: String,
    /// Status of this phrase
    status: SeriesStatus,
    /// Data points (empty for NotIndexed/Invalid)
    points: Vec<Point>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
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
    t: String,
    /// Raw occurrence count (client computes relative frequency using /totals)
    v: u32,
}

// ============================================================================
// Error Response Types (RFC-005 Section 9)
// ============================================================================

#[derive(Serialize, ToSchema)]
pub struct ErrorResponse {
    error: ErrorDetail,
}

#[derive(Serialize, ToSchema)]
pub struct ErrorDetail {
    code: String,
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
// Date Parsing
// ============================================================================

fn parse_date(s: &str) -> Option<Date> {
    let format = format_description!("[year]-[month]-[day]");
    Date::parse(s, format).ok()
}

fn format_date(date: Date) -> String {
    date.to_string()
}

// ============================================================================
// Sparse Filtering (RFC-005 Section 6, RFC-007-optimizations §2)
// ============================================================================

/// Return only non-zero points (sparse). Client handles zero-fill.
fn sparse_points(data: &HashMap<Date, u32>) -> Vec<Point> {
    let mut points: Vec<Point> = data
        .iter()
        .filter(|(_, &count)| count > 0)
        .map(|(&date, &count)| Point {
            t: format_date(date),
            v: count,
        })
        .collect();
    points.sort_by(|a, b| a.t.cmp(&b.t));
    points
}

// ============================================================================
// N-gram Order Helper
// ============================================================================

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
    path = "/query",
    params(QueryParams),
    responses(
        (status = 200, description = "Query results", body = QueryResponse),
        (status = 400, description = "Invalid query parameters", body = ErrorResponse)
    )
)]
pub async fn query(
    State(state): State<Arc<AppState>>,
    Query(params): Query<QueryParams>,
) -> impl IntoResponse {
    // Parse and validate phrases
    let phrases: Vec<&str> = params
        .phrases
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    if phrases.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(
                "MISSING_PHRASES",
                "No phrases provided",
            )),
        )
            .into_response();
    }

    if phrases.len() > MAX_PHRASES {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(
                "TOO_MANY_PHRASES",
                format!("Maximum {} phrases allowed, got {}", MAX_PHRASES, phrases.len()),
            )),
        )
            .into_response();
    }

    // Parse date range with defaults
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

    // Validate date range
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

    // Build response metadata (RFC-007-optimizations §3: no granularity, always daily)
    let meta = QueryMeta {
        tokenizer_version: TOKENIZER_VERSION.to_string(),
        start: format_date(start),
        end: format_date(end),
    };

    // Process each phrase
    let mut series = Vec::with_capacity(phrases.len());

    for phrase in phrases {
        // Tokenize the phrase
        let tokens = tokenizer::tokenize(phrase);
        let normalized = tokens.join(" ");

        // Check if it's a valid n-gram (1, 2, or 3 tokens)
        let n = match ngram_order(&tokens) {
            Some(n) => n,
            None => {
                series.push(Series {
                    phrase: phrase.to_string(),
                    normalized,
                    status: SeriesStatus::Invalid,
                    points: vec![],
                });
                continue;
            }
        };

        // Query ClickHouse — always daily, raw counts (RFC-007-optimizations §2, §3)
        let query_result = state
            .clickhouse
            .query_ngrams(n, &[normalized.clone()], start, end)
            .await
            .map(|rows| {
                rows.into_iter()
                    .map(|r| (r.bucket, r.count))
                    .collect::<HashMap<Date, u32>>()
            });

        match query_result {
            Ok(data) if data.is_empty() => {
                series.push(Series {
                    phrase: phrase.to_string(),
                    normalized,
                    status: SeriesStatus::NotIndexed,
                    points: vec![],
                });
            }
            Ok(data) => {
                let points = sparse_points(&data);
                series.push(Series {
                    phrase: phrase.to_string(),
                    normalized,
                    status: SeriesStatus::Indexed,
                    points,
                });
            }
            Err(e) => {
                tracing::warn!("ClickHouse query failed for '{}': {}", phrase, e);
                series.push(Series {
                    phrase: phrase.to_string(),
                    normalized,
                    status: SeriesStatus::NotIndexed,
                    points: vec![],
                });
            }
        }
    }

    (
        StatusCode::OK,
        [(header::CACHE_CONTROL, "public, max-age=3600")],
        Json(QueryResponse { series, meta }),
    )
        .into_response()
}

// ============================================================================
// Totals Endpoint (RFC-007-optimizations §2)
// ============================================================================

#[derive(Deserialize, ToSchema, IntoParams)]
pub struct TotalsParams {
    /// Start date (YYYY-MM-DD). Default: 2011-01-01
    start: Option<String>,
    /// End date (YYYY-MM-DD). Default: today
    end: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct TotalsResponse {
    /// One entry per n-gram order (1, 2, 3), each containing sparse daily totals
    totals: Vec<TotalSeries>,
    meta: TotalsMeta,
}

#[derive(Serialize, ToSchema)]
pub struct TotalsMeta {
    tokenizer_version: String,
    start: String,
    end: String,
}

#[derive(Serialize, ToSchema)]
pub struct TotalSeries {
    /// N-gram order (1, 2, or 3)
    n: u8,
    /// Sparse daily total counts (only non-zero days)
    points: Vec<TotalPoint>,
}

#[derive(Serialize, ToSchema)]
pub struct TotalPoint {
    /// Bucket timestamp (YYYY-MM-DD)
    t: String,
    /// Total n-gram count for this day and order
    v: u64,
}

#[utoipa::path(
    get,
    path = "/totals",
    params(TotalsParams),
    responses(
        (status = 200, description = "Bucket totals for normalization", body = TotalsResponse),
        (status = 400, description = "Invalid parameters", body = ErrorResponse)
    )
)]
pub async fn totals(
    State(state): State<Arc<AppState>>,
    Query(params): Query<TotalsParams>,
) -> impl IntoResponse {
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

    match state.clickhouse.query_bucket_totals(start, end).await {
        Ok(rows) => {
            // Group by n, return sparse (non-zero) points
            let mut by_n: HashMap<u8, Vec<TotalPoint>> = HashMap::new();
            for row in rows {
                if row.total_count > 0 {
                    by_n.entry(row.n).or_default().push(TotalPoint {
                        t: format_date(row.bucket),
                        v: row.total_count,
                    });
                }
            }

            let mut totals_series: Vec<TotalSeries> = (1..=3)
                .map(|n| TotalSeries {
                    n,
                    points: by_n.remove(&n).unwrap_or_default(),
                })
                .collect();
            for ts in &mut totals_series {
                ts.points.sort_by(|a, b| a.t.cmp(&b.t));
            }

            let meta = TotalsMeta {
                tokenizer_version: TOKENIZER_VERSION.to_string(),
                start: format_date(start),
                end: format_date(end),
            };

            (
                StatusCode::OK,
                // Cache aggressively — totals change only on ingestion
                [(header::CACHE_CONTROL, "public, max-age=86400")],
                Json(TotalsResponse {
                    totals: totals_series,
                    meta,
                }),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("ClickHouse totals query failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("INTERNAL_ERROR", "Failed to query totals")),
            )
                .into_response()
        }
    }
}
