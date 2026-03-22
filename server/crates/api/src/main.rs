//! HN N-gram API server (RFC-005)

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use hn_clickhouse::{
    default_start_date, Granularity, HnClickHouse, MAX_NGRAM_ORDER, MAX_PHRASES, TOKENIZER_VERSION,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use time::macros::format_description;
use time::Date;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use utoipa::{IntoParams, OpenApi, ToSchema};

// ============================================================================
// OpenAPI Documentation
// ============================================================================

#[derive(OpenApi)]
#[openapi(
    paths(health, query),
    components(schemas(
        HealthResponse,
        QueryParams,
        QueryResponse,
        QueryMeta,
        Series,
        SeriesStatus,
        Point,
        ErrorResponse,
        ErrorDetail
    ))
)]
struct ApiDoc;

// ============================================================================
// Application State
// ============================================================================

struct AppState {
    clickhouse: HnClickHouse,
}

// ============================================================================
// Main Entry Point
// ============================================================================

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let state = Arc::new(AppState {
        clickhouse: HnClickHouse::from_env(),
    });

    let app = Router::new()
        .route("/health", get(health))
        .route("/query", get(query))
        .merge(
            utoipa_swagger_ui::SwaggerUi::new("/swagger-ui")
                .url("/api-doc/openapi.json", ApiDoc::openapi()),
        )
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let port: u16 = std::env::var("API_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("API server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// ============================================================================
// Health Endpoint
// ============================================================================

#[derive(Serialize, ToSchema)]
struct HealthResponse {
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
async fn health() -> Json<HealthResponse> {
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
struct QueryParams {
    /// Comma-separated phrases to query (max 10)
    phrases: String,
    /// Start date (YYYY-MM-DD). Default: 2011-01-01
    start: Option<String>,
    /// End date (YYYY-MM-DD). Default: today
    end: Option<String>,
    /// Time granularity: day, week, month, year. Default: month
    granularity: Option<String>,
}

// ============================================================================
// Query Endpoint - Response Types
// ============================================================================

#[derive(Serialize, ToSchema)]
struct QueryResponse {
    series: Vec<Series>,
    meta: QueryMeta,
}

#[derive(Serialize, ToSchema)]
struct QueryMeta {
    tokenizer_version: String,
    start: String,
    end: String,
    granularity: String,
}

#[derive(Serialize, ToSchema)]
struct Series {
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
enum SeriesStatus {
    /// Phrase is in vocabulary, data returned
    Indexed,
    /// Phrase is valid but not in vocabulary (too rare)
    NotIndexed,
    /// Phrase failed validation (e.g., > 3 tokens, empty)
    Invalid,
}

#[derive(Serialize, ToSchema)]
struct Point {
    /// Bucket timestamp (YYYY-MM-DD)
    t: String,
    /// Relative frequency
    v: f64,
}

// ============================================================================
// Error Response Types (RFC-005 Section 9)
// ============================================================================

#[derive(Serialize, ToSchema)]
struct ErrorResponse {
    error: ErrorDetail,
}

#[derive(Serialize, ToSchema)]
struct ErrorDetail {
    code: String,
    message: String,
}

impl ErrorResponse {
    fn new(code: &str, message: impl Into<String>) -> Self {
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
// Zero-Fill Algorithm (RFC-005 Section 6)
// ============================================================================

fn zero_fill(
    data: &HashMap<Date, f64>,
    start: Date,
    end: Date,
    granularity: Granularity,
) -> Vec<Point> {
    let mut result = vec![];
    let mut current = align_to_granularity(start, granularity);

    while current <= end {
        let value = data.get(&current).copied().unwrap_or(0.0);
        result.push(Point {
            t: format_date(current),
            v: value,
        });
        current = next_bucket(current, granularity);
    }

    result
}

fn align_to_granularity(date: Date, granularity: Granularity) -> Date {
    match granularity {
        Granularity::Day => date,
        Granularity::Week => {
            // Align to Monday (weekday 0 = Monday in time crate)
            let weekday = date.weekday().number_days_from_monday();
            date - time::Duration::days(weekday as i64)
        }
        Granularity::Month => {
            Date::from_calendar_date(date.year(), date.month(), 1).unwrap_or(date)
        }
        Granularity::Year => {
            Date::from_calendar_date(date.year(), time::Month::January, 1).unwrap_or(date)
        }
    }
}

fn next_bucket(date: Date, granularity: Granularity) -> Date {
    match granularity {
        Granularity::Day => date + time::Duration::days(1),
        Granularity::Week => date + time::Duration::weeks(1),
        Granularity::Month => {
            let (year, month, _) = (date.year(), date.month(), date.day());
            if month == time::Month::December {
                Date::from_calendar_date(year + 1, time::Month::January, 1).unwrap_or(date)
            } else {
                Date::from_calendar_date(year, month.next(), 1).unwrap_or(date)
            }
        }
        Granularity::Year => {
            Date::from_calendar_date(date.year() + 1, time::Month::January, 1).unwrap_or(date)
        }
    }
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
async fn query(
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

    // Build response metadata
    let meta = QueryMeta {
        tokenizer_version: TOKENIZER_VERSION.to_string(),
        start: format_date(start),
        end: format_date(end),
        granularity: format!("{:?}", granularity).to_lowercase(),
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

        // Query ClickHouse
        let query_result = match granularity {
            Granularity::Day => state
                .clickhouse
                .query_ngrams(n, &[normalized.clone()], start, end)
                .await
                .map(|rows| {
                    rows.into_iter()
                        .map(|r| {
                            let value = if r.total_count > 0 {
                                r.count as f64 / r.total_count as f64
                            } else {
                                0.0
                            };
                            (r.bucket, value)
                        })
                        .collect::<HashMap<Date, f64>>()
                }),
            _ => state
                .clickhouse
                .query_ngrams_aggregated(n, &[normalized.clone()], start, end, granularity)
                .await
                .map(|rows| {
                    rows.into_iter()
                        .map(|r| {
                            let value = if r.sum_total > 0 {
                                r.sum_count as f64 / r.sum_total as f64
                            } else {
                                0.0
                            };
                            (r.bucket, value)
                        })
                        .collect::<HashMap<Date, f64>>()
                }),
        };

        match query_result {
            Ok(data) if data.is_empty() => {
                // No data found - phrase not indexed (or truly zero everywhere)
                series.push(Series {
                    phrase: phrase.to_string(),
                    normalized,
                    status: SeriesStatus::NotIndexed,
                    points: vec![],
                });
            }
            Ok(data) => {
                // Zero-fill and return complete time series
                let points = zero_fill(&data, start, end, granularity);
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

    (StatusCode::OK, Json(QueryResponse { series, meta })).into_response()
}
