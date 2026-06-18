use std::str::FromStr;

use anyhow::{Context, Result};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use crate::types::{ChatMessage, ChatSession, Role};

pub async fn init_db() -> Result<SqlitePool> {
    let database_url =
        std::env::var("DATABASE_URL").context("DATABASE_URL must be set in environment")?;

    let opts = SqliteConnectOptions::from_str(&database_url)
        .context("invalid DATABASE_URL")?
        .create_if_missing(true);

    if let Some(parent) = opts.get_filename().parent() {
        std::fs::create_dir_all(parent)?;
    }

    let pool = SqlitePool::connect_with(opts).await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS articles (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            title       TEXT NOT NULL,
            url         TEXT NOT NULL UNIQUE,
            source      TEXT NOT NULL,
            category    TEXT NOT NULL,
            summary     TEXT,
            content     TEXT,
            published_at TEXT NOT NULL,
            created_at  TEXT NOT NULL DEFAULT (datetime('now'))
         );",
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS chat_sessions (
            id          TEXT PRIMARY KEY,
            title       TEXT NOT NULL,
            created_at  TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
         );",
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS chat_messages (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id  TEXT NOT NULL REFERENCES chat_sessions(id),
            role        TEXT NOT NULL,
            content     TEXT NOT NULL,
            tool_calls  TEXT,
            created_at  TEXT NOT NULL DEFAULT (datetime('now'))
         );",
    )
    .execute(&pool)
    .await?;

    // Backfill the column on databases created before `tool_calls` existed. The
    // ALTER errors with "duplicate column name" once the column is present, which
    // is the expected steady state, so the error is ignored.
    if let Err(e) = sqlx::query("ALTER TABLE chat_messages ADD COLUMN tool_calls TEXT")
        .execute(&pool)
        .await
    {
        tracing::debug!("tool_calls column migration skipped: {e}");
    }

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_chat_messages_session
         ON chat_messages(session_id, id);",
    )
    .execute(&pool)
    .await?;

    Ok(pool)
}

/// Create a new chat session with the given title and return it.
pub async fn create_session(pool: &SqlitePool, title: &str) -> Result<ChatSession> {
    let id = Uuid::new_v4().to_string();

    sqlx::query("INSERT INTO chat_sessions (id, title) VALUES (?, ?)")
        .bind(&id)
        .bind(title)
        .execute(pool)
        .await
        .context("failed to insert chat session")?;

    let row = sqlx::query(
        "SELECT id, title, created_at, updated_at FROM chat_sessions WHERE id = ?",
    )
    .bind(&id)
    .fetch_one(pool)
    .await
    .context("failed to read back created session")?;

    Ok(ChatSession {
        id: row.get("id"),
        title: row.get("title"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

/// List all sessions, most recently updated first.
pub async fn list_sessions(pool: &SqlitePool) -> Result<Vec<ChatSession>> {
    let rows = sqlx::query(
        "SELECT id, title, created_at, updated_at
         FROM chat_sessions
         ORDER BY updated_at DESC, created_at DESC",
    )
    .fetch_all(pool)
    .await
    .context("failed to list chat sessions")?;

    Ok(rows
        .into_iter()
        .map(|row| ChatSession {
            id: row.get("id"),
            title: row.get("title"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
        .collect())
}

/// Load every message of a session, oldest first.
pub async fn load_messages(pool: &SqlitePool, session_id: &str) -> Result<Vec<ChatMessage>> {
    let rows = sqlx::query(
        "SELECT id, session_id, role, content, tool_calls, created_at
         FROM chat_messages
         WHERE session_id = ?
         ORDER BY id ASC",
    )
    .bind(session_id)
    .fetch_all(pool)
    .await
    .context("failed to load chat messages")?;

    let mut messages = Vec::with_capacity(rows.len());
    for row in rows {
        let role_str: String = row.get("role");
        let role = Role::from_str(&role_str).map_err(|e| anyhow::anyhow!(e))?;
        let tools_used = row
            .get::<Option<String>, _>("tool_calls")
            .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
            .unwrap_or_default();
        messages.push(ChatMessage {
            id: row.get("id"),
            session_id: row.get("session_id"),
            role,
            content: row.get("content"),
            created_at: row.get("created_at"),
            tools_used,
        });
    }

    Ok(messages)
}

/// Insert a message into a session and return the stored row. `tools_used` are
/// the display labels for any tools the assistant invoked while producing this
/// message (empty for user messages); they are stored as a JSON array. Also
/// bumps the session's `updated_at` so recently-used sessions sort to the top.
pub async fn insert_message(
    pool: &SqlitePool,
    session_id: &str,
    role: Role,
    content: &str,
    tools_used: &[String],
) -> Result<ChatMessage> {
    let tool_calls_json = if tools_used.is_empty() {
        None
    } else {
        Some(serde_json::to_string(tools_used).context("failed to serialize tool labels")?)
    };

    let result = sqlx::query(
        "INSERT INTO chat_messages (session_id, role, content, tool_calls) VALUES (?, ?, ?, ?)",
    )
    .bind(session_id)
    .bind(role.as_str())
    .bind(content)
    .bind(&tool_calls_json)
    .execute(pool)
    .await
    .context("failed to insert chat message")?;

    let id = result.last_insert_rowid();

    touch_session(pool, session_id).await?;

    let row = sqlx::query(
        "SELECT created_at FROM chat_messages WHERE id = ?",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .context("failed to read back inserted message")?;

    Ok(ChatMessage {
        id,
        session_id: session_id.to_string(),
        role,
        content: content.to_string(),
        created_at: row.get("created_at"),
        tools_used: tools_used.to_vec(),
    })
}

/// Bump a session's `updated_at` timestamp to now.
pub async fn touch_session(pool: &SqlitePool, session_id: &str) -> Result<()> {
    sqlx::query("UPDATE chat_sessions SET updated_at = datetime('now') WHERE id = ?")
        .bind(session_id)
        .execute(pool)
        .await
        .context("failed to touch session")?;
    Ok(())
}

/// Rename a session (used to auto-title a session from its first user message).
pub async fn rename_session(pool: &SqlitePool, session_id: &str, title: &str) -> Result<()> {
    sqlx::query("UPDATE chat_sessions SET title = ? WHERE id = ?")
        .bind(title)
        .bind(session_id)
        .execute(pool)
        .await
        .context("failed to rename session")?;
    Ok(())
}
