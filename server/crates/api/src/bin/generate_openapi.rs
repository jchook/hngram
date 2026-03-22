//! Generate OpenAPI spec from Rust types (RFC-005)
//!
//! Usage: cargo run -p api --bin generate_openapi > ../openapi.json

use api::ApiDoc;
use utoipa::OpenApi;

fn main() {
    let spec = ApiDoc::openapi()
        .to_pretty_json()
        .expect("Failed to serialize OpenAPI spec");
    print!("{}", spec);
}
