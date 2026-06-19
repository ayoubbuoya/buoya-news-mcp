//! The OpenAPI document for the HTTP backend.
//!
//! Aggregates every annotated route and the schemas they reference into one
//! [`ApiDoc`], rendered by Swagger UI and exposed as JSON. Adding a route means
//! adding its handler to `paths(...)` and any new types to `components(schemas(...))`.

use utoipa::OpenApi;

use crate::core::repository::{Article, ArticleSummary, SnapshotItem};
use crate::core::types::{ChatMessage, ChatSession, Role};
use crate::server::error::ErrorResponse;
use crate::server::routes::chat::{ChatRequest, ChatTurn, SessionChatRequest};
use crate::server::routes::sessions::{CreateSessionRequest, RenameSessionRequest};
use crate::server::routes::system::HealthResponse;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Buoya News Agent API",
        description = "HTTP backend for the Buoya news agent: chat sessions with a \
                       streaming agent (mirroring the terminal UI), article browsing \
                       and search over the local store, and daily market snapshots.",
        version = env!("CARGO_PKG_VERSION"),
    ),
    paths(
        crate::server::routes::system::health,
        crate::server::routes::articles::list_articles,
        crate::server::routes::articles::search_articles,
        crate::server::routes::articles::get_article,
        crate::server::routes::market::market_snapshot,
        crate::server::routes::sessions::list_sessions,
        crate::server::routes::sessions::create_session,
        crate::server::routes::sessions::list_messages,
        crate::server::routes::sessions::rename_session,
        crate::server::routes::sessions::delete_session,
        crate::server::routes::chat::chat,
        crate::server::routes::chat::session_chat,
    ),
    components(schemas(
        ArticleSummary,
        Article,
        SnapshotItem,
        ChatSession,
        ChatMessage,
        Role,
        HealthResponse,
        ErrorResponse,
        ChatRequest,
        ChatTurn,
        SessionChatRequest,
        CreateSessionRequest,
        RenameSessionRequest,
    )),
    tags(
        (name = "sessions", description = "Chat sessions and their message history."),
        (name = "chat", description = "Streaming agent chat (SSE)."),
        (name = "articles", description = "Browse and search stored articles."),
        (name = "market", description = "Daily market-condition snapshots."),
        (name = "system", description = "Health and operational endpoints."),
    )
)]
pub struct ApiDoc;
