//! HTTP backend adapter: an actix-web daemon that exposes the core to a frontend.
//!
//! Two kinds of routes: plain JSON data routes over [`Core::repository`] (what a
//! UI needs without involving the LLM), and a streaming `POST /chat` that drives
//! the agent via [`Core::chat_stream`] and relays [`StreamEvent`]s as Server-Sent
//! Events. The shared [`Core`] lives in `web::Data` so every worker shares one
//! pool, embedder, and client.
//!
//! `GET /mcp` is a placeholder until the MCP adapter is mounted here (step 5).
//!
//! Interactive API docs are served by Swagger UI at `/swagger-ui/`, backed by the
//! OpenAPI document generated from the [`utoipa::path`] annotations below and
//! served as JSON at `/api-docs/openapi.json`.

use std::time::Duration;

use actix_web::error::ErrorInternalServerError;
use actix_web::{App, HttpResponse, HttpServer, Result as ActixResult, web};
use actix_web_lab::sse::{self, Sse};
use serde::{Deserialize, Serialize};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::UnboundedReceiverStream;
use utoipa::{IntoParams, OpenApi, ToSchema};
use utoipa_swagger_ui::SwaggerUi;

use crate::connectors::telegram;
use crate::core::Core;
use crate::core::llm::StreamEvent;
use crate::core::repository::{Article, ArticleSummary, SnapshotItem};
use crate::core::types::{ChatMessage, Role};

/// OpenAPI document for the HTTP backend. Aggregates every annotated route and the
/// schemas they reference; rendered by Swagger UI and exposed as JSON.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "Buoya News Agent API",
        description = "HTTP backend for the Buoya news agent: article browsing and \
                       search over the local store, daily market snapshots, and a \
                       streaming chat endpoint that drives the LLM agent.",
        version = env!("CARGO_PKG_VERSION"),
    ),
    paths(
        health,
        list_articles,
        search_articles,
        get_article,
        market_snapshot,
        chat,
    ),
    components(schemas(
        ArticleSummary,
        Article,
        SnapshotItem,
        HealthResponse,
        ErrorResponse,
        ChatRequest,
        ChatTurn,
        Role,
    )),
    tags(
        (name = "articles", description = "Browse and search stored articles."),
        (name = "market", description = "Daily market-condition snapshots."),
        (name = "chat", description = "Streaming agent chat."),
        (name = "system", description = "Health and operational endpoints."),
    )
)]
pub struct ApiDoc;

/// Default number of articles a list/search route returns when unspecified.
const DEFAULT_LIMIT: i64 = 20;
/// Hard cap so one request cannot ask for an unbounded result set.
const MAX_LIMIT: i64 = 50;

/// `serve` subcommand options.
#[derive(Debug, Clone, clap::Args)]
pub struct ServeArgs {
    /// Address to bind the HTTP server to.
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,
    /// Port to listen on.
    #[arg(long, default_value_t = 8080)]
    pub port: u16,
}

/// Run the HTTP backend until shut down.
pub async fn run(core: Core, args: ServeArgs) -> anyhow::Result<()> {
    // Start outbound connectors before serving HTTP. They're pure background
    // consumers of the core (they subscribe to the ingest broadcast), so they run as
    // detached tasks alongside the web server. `resolve` returns `Some` only when the
    // connector is enabled and fully configured; otherwise we don't spawn anything.
    if let Some(tg) = telegram::TelegramConfig::resolve(&core.config) {
        tokio::spawn(telegram::run(core.clone(), tg));
    }

    let data = web::Data::new(core);
    let (host, port) = (args.host.clone(), args.port);
    tracing::info!("starting HTTP server on http://{host}:{port}");
    tracing::info!("API docs available at http://{host}:{port}/swagger-ui/");

    HttpServer::new(move || {
        App::new()
            .app_data(data.clone())
            .route("/health", web::get().to(health))
            .route("/articles", web::get().to(list_articles))
            .route("/articles/search", web::get().to(search_articles))
            .route("/articles/{id}", web::get().to(get_article))
            .route("/market/snapshot", web::get().to(market_snapshot))
            .route("/chat", web::post().to(chat))
            .route("/mcp", web::to(mcp_stub))
            .service(
                SwaggerUi::new("/swagger-ui/{_:.*}")
                    .url("/api-docs/openapi.json", ApiDoc::openapi()),
            )
    })
    .bind((host.as_str(), port))?
    .run()
    .await?;

    Ok(())
}

/// Liveness response for [`health`].
#[derive(Debug, Serialize, ToSchema)]
struct HealthResponse {
    /// Always `"ok"` when the server is reachable.
    #[schema(example = "ok")]
    status: String,
}

/// Generic error envelope returned by failing routes.
#[derive(Debug, Serialize, ToSchema)]
struct ErrorResponse {
    /// Human-readable description of what went wrong.
    error: String,
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
async fn health() -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({ "status": "ok" }))
}

/// Clamp a caller-provided limit into `1..=MAX_LIMIT`, defaulting when absent.
fn resolve_limit(limit: Option<i64>) -> i64 {
    limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
struct ListQuery {
    /// Restrict to one category (e.g. `crypto`, `ai`, `security`, `market`, `defi`).
    category: Option<String>,
    /// Maximum number of articles to return. Clamped to `1..=50`; defaults to 20.
    limit: Option<i64>,
}

/// List the most recent articles, optionally filtered by category.
#[utoipa::path(
    get,
    path = "/articles",
    tag = "articles",
    params(ListQuery),
    responses(
        (status = 200, description = "Most recent articles", body = [ArticleSummary]),
        (status = 500, description = "Query failed", body = ErrorResponse),
    )
)]
async fn list_articles(
    core: web::Data<Core>,
    query: web::Query<ListQuery>,
) -> ActixResult<HttpResponse> {
    let limit = resolve_limit(query.limit);
    let articles = core
        .repository()
        .list_recent(query.category.as_deref(), limit)
        .await
        .map_err(|e| ErrorInternalServerError(format!("{e:#}")))?;
    Ok(HttpResponse::Ok().json(articles))
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
struct SearchQuery {
    /// Search query text.
    q: String,
    /// When true, run meaning-based vector search; otherwise exact keyword search.
    #[serde(default)]
    semantic: bool,
    /// Maximum number of results to return. Clamped to `1..=50`; defaults to 20.
    limit: Option<i64>,
}

/// Search articles by keyword or semantic (vector) similarity.
#[utoipa::path(
    get,
    path = "/articles/search",
    tag = "articles",
    params(SearchQuery),
    responses(
        (status = 200, description = "Matching articles, most relevant first", body = [ArticleSummary]),
        (status = 500, description = "Search failed", body = ErrorResponse),
    )
)]
async fn search_articles(
    core: web::Data<Core>,
    query: web::Query<SearchQuery>,
) -> ActixResult<HttpResponse> {
    let limit = resolve_limit(query.limit);
    let repo = core.repository();
    let articles = if query.semantic {
        repo.search_semantic(&query.q, limit).await
    } else {
        repo.search_keyword(&query.q, limit).await
    }
    .map_err(|e| ErrorInternalServerError(format!("{e:#}")))?;
    Ok(HttpResponse::Ok().json(articles))
}

/// Fetch a single article by id, including its full body.
#[utoipa::path(
    get,
    path = "/articles/{id}",
    tag = "articles",
    params(
        ("id" = i64, Path, description = "Article id"),
    ),
    responses(
        (status = 200, description = "The full article", body = Article),
        (status = 404, description = "No article with that id", body = ErrorResponse),
        (status = 500, description = "Lookup failed", body = ErrorResponse),
    )
)]
async fn get_article(core: web::Data<Core>, path: web::Path<i64>) -> ActixResult<HttpResponse> {
    let id = path.into_inner();
    match core
        .repository()
        .get_article(id)
        .await
        .map_err(|e| ErrorInternalServerError(format!("{e:#}")))?
    {
        Some(article) => Ok(HttpResponse::Ok().json(article)),
        None => Ok(HttpResponse::NotFound()
            .json(serde_json::json!({ "error": format!("no article with id {id}") }))),
    }
}

/// Latest daily snapshot per market source (Fear & Greed, market overview, DeFi TVL).
#[utoipa::path(
    get,
    path = "/market/snapshot",
    tag = "market",
    responses(
        (status = 200, description = "Newest snapshot per source", body = [SnapshotItem]),
        (status = 500, description = "Query failed", body = ErrorResponse),
    )
)]
async fn market_snapshot(core: web::Data<Core>) -> ActixResult<HttpResponse> {
    let snapshots = core
        .repository()
        .market_snapshot()
        .await
        .map_err(|e| ErrorInternalServerError(format!("{e:#}")))?;
    Ok(HttpResponse::Ok().json(snapshots))
}

/// One message a client supplies in a chat request. Only role + content matter to
/// the agent; the rest of [`ChatMessage`] is DB bookkeeping the server fills in.
#[derive(Debug, Deserialize, ToSchema)]
struct ChatTurn {
    role: Role,
    /// The message text.
    content: String,
}

/// Body for `POST /chat`: the conversation so far, oldest message first.
#[derive(Debug, Deserialize, ToSchema)]
struct ChatRequest {
    messages: Vec<ChatTurn>,
}

/// Drive one agent turn and stream the reply as Server-Sent Events.
///
/// The response is a `text/event-stream`. Event names the client can switch on:
/// `token` (a text chunk to append), `tool` (a human-readable tool-call label),
/// `done` (turn finished), `error` (failure message). The stream closes after
/// `done` or `error`.
#[utoipa::path(
    post,
    path = "/chat",
    tag = "chat",
    request_body = ChatRequest,
    responses(
        (
            status = 200,
            description = "SSE stream of `token`/`tool`/`done`/`error` events",
            content_type = "text/event-stream",
            body = String,
        ),
    )
)]
async fn chat(core: web::Data<Core>, body: web::Json<ChatRequest>) -> impl actix_web::Responder {
    let history: Vec<ChatMessage> = body
        .into_inner()
        .messages
        .into_iter()
        .map(|turn| ChatMessage {
            id: 0,
            session_id: String::new(),
            role: turn.role,
            content: turn.content,
            created_at: String::new(),
            tools_used: Vec::new(),
        })
        .collect();

    let stream = UnboundedReceiverStream::new(core.chat_stream(history)).map(|event| {
        let data = match event {
            StreamEvent::Token(text) => sse::Data::new(text).event("token"),
            StreamEvent::ToolCall(label) => sse::Data::new(label).event("tool"),
            StreamEvent::Done => sse::Data::new("").event("done"),
            StreamEvent::Error(message) => sse::Data::new(message).event("error"),
        };
        sse::Event::Data(data)
    });

    Sse::from_infallible_stream(stream).with_keep_alive(Duration::from_secs(15))
}

/// Placeholder for the MCP-over-HTTP endpoint mounted here in step 5.
async fn mcp_stub() -> HttpResponse {
    HttpResponse::NotImplemented()
        .json(serde_json::json!({ "error": "MCP endpoint not yet implemented" }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The generated OpenAPI document covers every documented route and resolves
    /// the schemas they reference, so Swagger UI renders without dangling `$ref`s.
    #[test]
    fn openapi_document_lists_all_routes() {
        let doc = ApiDoc::openapi();
        let paths = &doc.paths.paths;

        for expected in [
            "/health",
            "/articles",
            "/articles/search",
            "/articles/{id}",
            "/market/snapshot",
            "/chat",
        ] {
            assert!(paths.contains_key(expected), "missing path: {expected}");
        }

        let components = doc.components.as_ref();
        assert!(components.is_some(), "components block should be present");
        if let Some(components) = components {
            for schema in ["ArticleSummary", "Article", "SnapshotItem", "ChatRequest"] {
                assert!(
                    components.schemas.contains_key(schema),
                    "missing schema: {schema}"
                );
            }
        }
    }
}
