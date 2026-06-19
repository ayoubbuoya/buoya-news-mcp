//! System routes: liveness and the not-yet-implemented MCP placeholder.

use actix_web::{HttpResponse, web};
use serde::Serialize;
use utoipa::ToSchema;

/// Liveness response for [`health`].
#[derive(Debug, Serialize, ToSchema)]
pub struct HealthResponse {
    /// Always `"ok"` when the server is reachable.
    #[schema(example = "ok")]
    pub status: String,
}

/// Liveness probe.
#[utoipa::path(
    get,
    path = "/health",
    tag = "system",
    responses(
        (status = 200, description = "Server is up", body = HealthResponse),
    )
)]
pub async fn health() -> HttpResponse {
    HttpResponse::Ok().json(HealthResponse {
        status: "ok".to_string(),
    })
}

/// Placeholder for the MCP-over-HTTP endpoint mounted here in a later step.
pub async fn mcp_stub() -> HttpResponse {
    HttpResponse::NotImplemented()
        .json(serde_json::json!({ "error": "MCP endpoint not yet implemented" }))
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/health", web::get().to(health))
        .route("/mcp", web::to(mcp_stub));
}
