//! Session routes: list/create/rename/delete chat sessions and read their messages.
//!
//! These expose the same session store the TUI uses (`core::db`), so a web client
//! and the terminal UI share one persisted history. The streaming send endpoint
//! that drives a turn lives in [`super::chat`] (`POST /sessions/{id}/chat`).

use actix_web::{HttpResponse, Result as ActixResult, web};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::core::Core;
use crate::core::db;
use crate::server::error::{ErrorResponse, bad_request, internal, not_found};

/// Body for `POST /sessions`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateSessionRequest {
    /// Optional initial title. Defaults to `"New chat"` when omitted or blank.
    pub title: Option<String>,
}

/// Body for `PATCH /sessions/{id}`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct RenameSessionRequest {
    /// The new session title. Must not be blank.
    pub title: String,
}

/// List all chat sessions, most-recently-updated first.
#[utoipa::path(
    get,
    path = "/sessions",
    tag = "sessions",
    responses(
        (status = 200, description = "All sessions, newest first", body = [crate::core::types::ChatSession]),
        (status = 500, description = "Query failed", body = ErrorResponse),
    )
)]
pub async fn list_sessions(core: web::Data<Core>) -> ActixResult<HttpResponse> {
    let sessions = db::list_sessions(&core.db_pool).await.map_err(internal)?;
    Ok(HttpResponse::Ok().json(sessions))
}

/// Create a new chat session.
#[utoipa::path(
    post,
    path = "/sessions",
    tag = "sessions",
    request_body = CreateSessionRequest,
    responses(
        (status = 201, description = "The created session", body = crate::core::types::ChatSession),
        (status = 500, description = "Creation failed", body = ErrorResponse),
    )
)]
pub async fn create_session(
    core: web::Data<Core>,
    body: web::Json<CreateSessionRequest>,
) -> ActixResult<HttpResponse> {
    let title = body
        .into_inner()
        .title
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| db::DEFAULT_SESSION_TITLE.to_string());
    let session = db::create_session(&core.db_pool, &title)
        .await
        .map_err(internal)?;
    Ok(HttpResponse::Created().json(session))
}

/// List every message in a session, oldest first.
#[utoipa::path(
    get,
    path = "/sessions/{id}/messages",
    tag = "sessions",
    params(("id" = String, Path, description = "Session id (UUID)")),
    responses(
        (status = 200, description = "The session's messages", body = [crate::core::types::ChatMessage]),
        (status = 404, description = "No session with that id", body = ErrorResponse),
        (status = 500, description = "Query failed", body = ErrorResponse),
    )
)]
pub async fn list_messages(
    core: web::Data<Core>,
    path: web::Path<String>,
) -> ActixResult<HttpResponse> {
    let id = path.into_inner();
    if db::get_session(&core.db_pool, &id)
        .await
        .map_err(internal)?
        .is_none()
    {
        return Ok(not_found(format!("no session with id {id}")));
    }
    let messages = db::load_messages(&core.db_pool, &id)
        .await
        .map_err(internal)?;
    Ok(HttpResponse::Ok().json(messages))
}

/// Rename a session.
#[utoipa::path(
    patch,
    path = "/sessions/{id}",
    tag = "sessions",
    params(("id" = String, Path, description = "Session id (UUID)")),
    request_body = RenameSessionRequest,
    responses(
        (status = 200, description = "The updated session", body = crate::core::types::ChatSession),
        (status = 400, description = "Title is blank", body = ErrorResponse),
        (status = 404, description = "No session with that id", body = ErrorResponse),
        (status = 500, description = "Update failed", body = ErrorResponse),
    )
)]
pub async fn rename_session(
    core: web::Data<Core>,
    path: web::Path<String>,
    body: web::Json<RenameSessionRequest>,
) -> ActixResult<HttpResponse> {
    let id = path.into_inner();
    let title = body.into_inner().title.trim().to_string();
    if title.is_empty() {
        return Ok(bad_request("title must not be blank"));
    }
    if db::get_session(&core.db_pool, &id)
        .await
        .map_err(internal)?
        .is_none()
    {
        return Ok(not_found(format!("no session with id {id}")));
    }
    db::rename_session(&core.db_pool, &id, &title)
        .await
        .map_err(internal)?;
    match db::get_session(&core.db_pool, &id).await.map_err(internal)? {
        Some(session) => Ok(HttpResponse::Ok().json(session)),
        None => Ok(not_found(format!("no session with id {id}"))),
    }
}

/// Delete a session and all of its messages. Idempotent: deleting a missing session
/// still returns 204.
#[utoipa::path(
    delete,
    path = "/sessions/{id}",
    tag = "sessions",
    params(("id" = String, Path, description = "Session id (UUID)")),
    responses(
        (status = 204, description = "Deleted (or already absent)"),
        (status = 500, description = "Delete failed", body = ErrorResponse),
    )
)]
pub async fn delete_session(
    core: web::Data<Core>,
    path: web::Path<String>,
) -> ActixResult<HttpResponse> {
    let id = path.into_inner();
    db::delete_session(&core.db_pool, &id)
        .await
        .map_err(internal)?;
    Ok(HttpResponse::NoContent().finish())
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/sessions", web::get().to(list_sessions))
        .route("/sessions", web::post().to(create_session))
        .route("/sessions/{id}/messages", web::get().to(list_messages))
        .route("/sessions/{id}", web::patch().to(rename_session))
        .route("/sessions/{id}", web::delete().to(delete_session));
}
