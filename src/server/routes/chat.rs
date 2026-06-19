//! Chat routes: drive one agent turn and stream the reply as Server-Sent Events.
//!
//! Two flavours:
//!
//! * `POST /sessions/{id}/chat` — **persisted**. The server stores the user message,
//!   streams the reply, and persists the finished assistant message (with its tool
//!   labels) against the session — even if the client disconnects mid-stream. This is
//!   what a session-based frontend should use; it mirrors the TUI's send flow.
//! * `POST /chat` — **stateless**. The client supplies the full history each call and
//!   nothing is persisted. Kept for simple, session-less integrations.
//!
//! Both relay [`StreamEvent`]s as named SSE events: `token`, `tool`, `done`, `error`.

use std::convert::Infallible;
use std::pin::Pin;
use std::time::Duration;

use actix_web::{Either, HttpResponse, Result as ActixResult, web};
use actix_web_lab::sse::{self, Sse};
use futures::Stream;
use serde::Deserialize;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::UnboundedReceiverStream;
use utoipa::ToSchema;

use crate::core::Core;
use crate::core::db;
use crate::core::llm::StreamEvent;
use crate::core::types::{ChatMessage, Role};
use crate::server::error::{bad_request, internal, not_found};

/// Keep-alive comment cadence so idle connections aren't dropped by proxies.
const KEEP_ALIVE: Duration = Duration::from_secs(15);

/// A boxed SSE stream — named so handlers can share one return type. The stream is
/// infallible (`Infallible` error), which `Sse` still models as a `Result`.
type SseResponse = Sse<Pin<Box<dyn Stream<Item = Result<sse::Event, Infallible>> + Send>>>;

/// One message a client supplies in a stateless chat request. Only role + content
/// matter to the agent; the rest of [`ChatMessage`] is bookkeeping the server fills in.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ChatTurn {
    pub role: Role,
    /// The message text.
    pub content: String,
}

/// Body for the stateless `POST /chat`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ChatRequest {
    /// The conversation so far, oldest message first.
    pub messages: Vec<ChatTurn>,
}

/// Body for the persisted `POST /sessions/{id}/chat`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SessionChatRequest {
    /// The new user message to send.
    pub content: String,
}

/// Map a core stream event onto its named SSE event.
fn event_to_sse(event: StreamEvent) -> sse::Event {
    let data = match event {
        StreamEvent::Token(text) => sse::Data::new(text).event("token"),
        StreamEvent::ToolCall(label) => sse::Data::new(label).event("tool"),
        StreamEvent::Done => sse::Data::new("").event("done"),
        StreamEvent::Error(message) => sse::Data::new(message).event("error"),
    };
    sse::Event::Data(data)
}

/// Wrap a stream-event receiver as a keep-alive SSE response.
fn sse_response(rx: UnboundedReceiver<StreamEvent>) -> SseResponse {
    let stream = UnboundedReceiverStream::new(rx).map(|event| Ok(event_to_sse(event)));
    let boxed: Pin<Box<dyn Stream<Item = Result<sse::Event, Infallible>> + Send>> =
        Box::pin(stream);
    Sse::from_stream(boxed).with_keep_alive(KEEP_ALIVE)
}

/// `POST /chat` — drive one agent turn over a client-supplied history and stream the
/// reply as SSE. Stateless: nothing is persisted.
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
pub async fn chat(core: web::Data<Core>, body: web::Json<ChatRequest>) -> SseResponse {
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

    sse_response(core.chat_stream(history))
}

/// `POST /sessions/{id}/chat` — persist the user message, stream the reply, and
/// persist the finished assistant message against the session.
#[utoipa::path(
    post,
    path = "/sessions/{id}/chat",
    tag = "chat",
    params(("id" = String, Path, description = "Session id (UUID)")),
    request_body = SessionChatRequest,
    responses(
        (
            status = 200,
            description = "SSE stream of `token`/`tool`/`done`/`error` events",
            content_type = "text/event-stream",
            body = String,
        ),
        (status = 400, description = "Empty message", body = crate::server::error::ErrorResponse),
        (status = 404, description = "No session with that id", body = crate::server::error::ErrorResponse),
        (status = 500, description = "Failed to start the turn", body = crate::server::error::ErrorResponse),
    )
)]
pub async fn session_chat(
    core: web::Data<Core>,
    path: web::Path<String>,
    body: web::Json<SessionChatRequest>,
) -> ActixResult<Either<HttpResponse, SseResponse>> {
    let session_id = path.into_inner();
    let content = body.into_inner().content.trim().to_string();
    if content.is_empty() {
        return Ok(Either::Left(bad_request("message content must not be empty")));
    }

    // 404 rather than silently creating an orphan if the session is unknown.
    if db::get_session(&core.db_pool, &session_id)
        .await
        .map_err(internal)?
        .is_none()
    {
        return Ok(Either::Left(not_found(format!(
            "no session with id {session_id}"
        ))));
    }

    // Persist the user turn, then auto-title a still-unnamed session from its first
    // message (mirrors the TUI).
    let existing = db::load_messages(&core.db_pool, &session_id)
        .await
        .map_err(internal)?;
    let is_first = existing.is_empty();

    let user_msg = db::insert_message(&core.db_pool, &session_id, Role::User, &content, &[])
        .await
        .map_err(internal)?;

    if is_first {
        let title = db::title_from(&content);
        db::rename_session(&core.db_pool, &session_id, &title)
            .await
            .map_err(internal)?;
    }

    let mut history = existing;
    history.push(user_msg);

    // Relay events to the client while accumulating the reply, so the assistant
    // message is persisted when the turn ends regardless of the client's fate.
    let core_rx = core.chat_stream(history);
    let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(relay_and_persist(
        core.get_ref().clone(),
        session_id,
        core_rx,
        out_tx,
    ));

    Ok(Either::Right(sse_response(out_rx)))
}

/// Forward stream events to the SSE client while accumulating the assistant's reply,
/// persisting it once the turn finishes (on `Done` or `Error`). Runs detached so it
/// completes — and persists — even if the client hangs up.
async fn relay_and_persist(
    core: Core,
    session_id: String,
    mut rx: UnboundedReceiver<StreamEvent>,
    out_tx: UnboundedSender<StreamEvent>,
) {
    let mut partial = String::new();
    let mut tools: Vec<String> = Vec::new();

    while let Some(event) = rx.recv().await {
        match &event {
            StreamEvent::Token(text) => partial.push_str(text),
            StreamEvent::ToolCall(label) => tools.push(label.clone()),
            StreamEvent::Done | StreamEvent::Error(_) => {
                if !partial.trim().is_empty()
                    && let Err(e) = db::insert_message(
                        &core.db_pool,
                        &session_id,
                        Role::Assistant,
                        &partial,
                        &tools,
                    )
                    .await
                {
                    tracing::error!("failed to persist assistant message: {e:#}");
                }
            }
        }
        // Forward to the SSE client; ignore the error if it has already hung up.
        let _ = out_tx.send(event);
    }
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/chat", web::post().to(chat))
        .route("/sessions/{id}/chat", web::post().to(session_chat));
}
