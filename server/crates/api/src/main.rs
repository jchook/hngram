use axum::{
    extract::Query,
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use utoipa::{IntoParams, OpenApi, ToSchema};

#[derive(OpenApi)]
#[openapi(
    paths(health, query),
    components(schemas(HealthResponse, QueryParams, QueryResponse, SeriesData, DataPoint, PhraseStatus))
)]
struct ApiDoc;

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let app = Router::new()
        .route("/health", get(health))
        .route("/query", get(query))
        .merge(utoipa_swagger_ui::SwaggerUi::new("/swagger-ui").url("/api-doc/openapi.json", ApiDoc::openapi()))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

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
    /// Smoothing window size
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

#[utoipa::path(
    get,
    path = "/query",
    params(QueryParams),
    responses(
        (status = 200, description = "Query results", body = QueryResponse),
        (status = 400, description = "Invalid query parameters")
    )
)]
async fn query(Query(params): Query<QueryParams>) -> impl IntoResponse {
    let phrases: Vec<&str> = params.phrases.split(',').map(|s| s.trim()).collect();

    if phrases.is_empty() || phrases.iter().any(|p| p.is_empty()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "No phrases provided" })),
        )
            .into_response();
    }

    // TODO: Implement actual ClickHouse query per RFC-005
    // For now, return stub data
    let series: Vec<SeriesData> = phrases
        .iter()
        .map(|phrase| SeriesData {
            phrase: phrase.to_string(),
            status: PhraseStatus::NotIndexed,
            points: vec![],
        })
        .collect();

    (StatusCode::OK, Json(QueryResponse { series })).into_response()
}
