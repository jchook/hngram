use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use hn_clickhouse::{Granularity, HnClickHouse};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use time::macros::format_description;
use time::Date;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use utoipa::{IntoParams, OpenApi, ToSchema};

#[derive(OpenApi)]
#[openapi(
    paths(health, query),
    components(schemas(HealthResponse, QueryParams, QueryResponse, SeriesData, DataPoint, PhraseStatus))
)]
struct ApiDoc;

struct AppState {
    clickhouse: HnClickHouse,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let state = Arc::new(AppState {
        clickhouse: HnClickHouse::from_env(),
    });

    let app = Router::new()
        .route("/health", get(health))
        .route("/query", get(query))
        .merge(utoipa_swagger_ui::SwaggerUi::new("/swagger-ui").url("/api-doc/openapi.json", ApiDoc::openapi()))
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

#[derive(Serialize, ToSchema)]
struct HealthResponse {
    status: String,
    version: String,
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
    })
}

#[derive(Deserialize, ToSchema, IntoParams)]
struct QueryParams {
    /// Comma-separated phrases to query
    #[serde(rename = "phrases")]
    phrases: String,
    /// Start date (YYYY-MM-DD)
    start: Option<String>,
    /// End date (YYYY-MM-DD)
    end: Option<String>,
    /// Time granularity: day, week, month, year
    granularity: Option<String>,
    /// Smoothing window size (not yet implemented)
    smoothing: Option<u32>,
}

#[derive(Serialize, ToSchema)]
struct QueryResponse {
    series: Vec<SeriesData>,
}

#[derive(Serialize, ToSchema)]
struct SeriesData {
    phrase: String,
    status: PhraseStatus,
    points: Vec<DataPoint>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
enum PhraseStatus {
    Indexed,
    NotIndexed,
    Invalid,
}

#[derive(Serialize, ToSchema)]
struct DataPoint {
    t: String,
    value: f64,
}

/// Parse date string in YYYY-MM-DD format
fn parse_date(s: &str) -> Option<Date> {
    let format = format_description!("[year]-[month]-[day]");
    Date::parse(s, format).ok()
}

/// Determine n-gram order from token count
fn ngram_order(tokens: &[String]) -> Option<u8> {
    match tokens.len() {
        1 => Some(1),
        2 => Some(2),
        3 => Some(3),
        _ => None,
    }
}

#[utoipa::path(
    get,
    path = "/query",
    params(QueryParams),
    responses(
        (status = 200, description = "Query results", body = QueryResponse),
        (status = 400, description = "Invalid query parameters")
    )
)]
async fn query(
    State(state): State<Arc<AppState>>,
    Query(params): Query<QueryParams>,
) -> impl IntoResponse {
    // Parse phrases
    let phrases: Vec<&str> = params.phrases.split(',').map(|s| s.trim()).collect();
    if phrases.is_empty() || phrases.iter().any(|p| p.is_empty()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "No phrases provided" })),
        )
            .into_response();
    }

    // Parse date range (use defaults if not provided)
    let start = params
        .start
        .as_deref()
        .and_then(parse_date)
        .unwrap_or_else(|| Date::from_calendar_date(2006, time::Month::October, 1).unwrap());

    let end = params
        .end
        .as_deref()
        .and_then(parse_date)
        .unwrap_or_else(|| {
            // Default to today
            let now = time::OffsetDateTime::now_utc();
            now.date()
        });

    // Parse granularity
    let granularity = params
        .granularity
        .as_deref()
        .and_then(Granularity::from_str)
        .unwrap_or_default();

    // Smoothing is parsed but not yet implemented
    let _smoothing = params.smoothing.unwrap_or(0);

    // Process each phrase
    let mut series = Vec::with_capacity(phrases.len());

    for phrase in phrases {
        // Tokenize the phrase
        let tokens = tokenizer::tokenize(phrase);

        // Check if it's a valid n-gram (1, 2, or 3 tokens)
        let n = match ngram_order(&tokens) {
            Some(n) => n,
            None => {
                // Invalid: too many tokens or empty
                series.push(SeriesData {
                    phrase: phrase.to_string(),
                    status: PhraseStatus::Invalid,
                    points: vec![],
                });
                continue;
            }
        };

        // Build the normalized n-gram string
        let ngram = tokens.join(" ");

        // Query ClickHouse - handle both query types
        let points = match granularity {
            Granularity::Day => {
                match state.clickhouse.query_ngrams(n, &[ngram.clone()], start, end).await {
                    Ok(rows) => rows
                        .iter()
                        .map(|row| DataPoint {
                            t: row.bucket.to_string(),
                            value: if row.total_count > 0 {
                                row.count as f64 / row.total_count as f64
                            } else {
                                0.0
                            },
                        })
                        .collect(),
                    Err(e) => {
                        tracing::warn!("ClickHouse query failed for '{}': {}", phrase, e);
                        vec![]
                    }
                }
            }
            _ => {
                match state.clickhouse.query_ngrams_aggregated(n, &[ngram.clone()], start, end, granularity).await {
                    Ok(rows) => rows
                        .iter()
                        .map(|row| DataPoint {
                            t: row.bucket.to_string(),
                            value: if row.sum_total > 0 {
                                row.sum_count as f64 / row.sum_total as f64
                            } else {
                                0.0
                            },
                        })
                        .collect(),
                    Err(e) => {
                        tracing::warn!("ClickHouse aggregated query failed for '{}': {}", phrase, e);
                        vec![]
                    }
                }
            }
        };

        // Determine status based on whether we got data
        let status = if points.is_empty() {
            PhraseStatus::NotIndexed
        } else {
            PhraseStatus::Indexed
        };

        series.push(SeriesData {
            phrase: phrase.to_string(),
            status,
            points,
        });
    }

    (StatusCode::OK, Json(QueryResponse { series })).into_response()
}
