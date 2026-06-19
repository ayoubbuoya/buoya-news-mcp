//! Shared error helpers and the JSON error envelope used across routes.
//!
//! Every route reports failures the same way — `{ "error": "<message>" }` with an
//! appropriate status — so the frontend can handle errors uniformly. Keeping the
//! envelope and the mapping helpers here (rather than repeated per handler) is the
//! single source of truth for that contract.

use std::fmt::Display;

use actix_web::HttpResponse;
use actix_web::error::ErrorInternalServerError;
use serde::Serialize;
use utoipa::ToSchema;

/// Generic error envelope returned by failing routes: `{ "error": "..." }`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorResponse {
    /// Human-readable description of what went wrong.
    pub error: String,
}

impl ErrorResponse {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            error: message.into(),
        }
    }
}

/// Map any error into a 500 response, rendering the full `anyhow` cause chain
/// (`{:#}`) into the message. Use as `.map_err(internal)?` in route handlers.
pub fn internal(err: impl Display) -> actix_web::Error {
    ErrorInternalServerError(format!("{err:#}"))
}

/// Build a 404 response with the standard error envelope.
pub fn not_found(message: impl Into<String>) -> HttpResponse {
    HttpResponse::NotFound().json(ErrorResponse::new(message))
}

/// Build a 400 response with the standard error envelope.
pub fn bad_request(message: impl Into<String>) -> HttpResponse {
    HttpResponse::BadRequest().json(ErrorResponse::new(message))
}
