//! The application core: owns the shared state (database, embedder, LLM and HTTP
//! clients, config) and the background tasks (ingest loop, embedding backfill).
//!
//! Built once via [`Core::start`] and handed to whichever front-end adapter runs
//! — today the TUI, later an HTTP server, MCP server, or external connectors. The
//! core knows nothing about its callers; adapters depend on it, not the reverse.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use async_openai::{Client, config::OpenAIConfig};
use sqlx::SqlitePool;
use tokio::sync::broadcast;
use tokio::sync::mpsc::{self, UnboundedReceiver};

pub mod config;
pub mod db;
pub mod embeddings;
pub mod error;
pub mod fetchers;
pub mod ingest;
pub mod llm;
pub mod repository;
pub mod types;

use config::AppConfig;
use embeddings::Embedder;
use llm::StreamEvent;
use types::ChatMessage;

use repository::{ArticleSummary, Repository};

/// How many ingest batches the broadcast channel buffers per subscriber before the
/// oldest are dropped. Alerts are allowed to be lossy (a slow connector lagging is
/// fine); the ingest loop must never block waiting on a subscriber, so we cap the
/// buffer rather than let it grow unbounded.
const INGEST_BROADCAST_CAPACITY: usize = 64;

/// Shared, cheaply-cloneable handle to everything an adapter needs. Clones share
/// the same connection pool, clients, and embedder.
#[derive(Debug, Clone)]
pub struct Core {
    pub http_client: reqwest::Client,
    pub config: Arc<AppConfig>,
    pub db_pool: SqlitePool,
    pub llm_client: Client<OpenAIConfig>,
    pub embedder: Arc<Embedder>,
    /// Publishes a batch of newly-ingested articles after each ingest tick. This is
    /// the sender end; adapters get a receiver via [`Core::subscribe_ingest`].
    /// Holding the sender in `Core` keeps the channel alive even when no one is
    /// currently subscribed (a fresh subscriber simply starts receiving the next
    /// batch). Cloning `Core` clones the sender, so every clone publishes/subscribes
    /// to the same channel.
    ingest_tx: broadcast::Sender<Arc<[ArticleSummary]>>,
}

impl Core {
    /// Build the core from a loaded config: open the database, construct the HTTP
    /// and LLM clients, load the embedding model, then spawn the background ingest
    /// and embedding-backfill tasks. Returns once the core is ready to serve; the
    /// background tasks keep running for the lifetime of the process.
    pub async fn start(config: AppConfig) -> Result<Self> {
        register_sqlite_vec();

        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_millis(config.toml_config.http.timeout_ms))
            .user_agent(&config.toml_config.http.user_agent)
            .build()?;

        let llm_config = OpenAIConfig::new()
            .with_api_key(&config.ai_api_key)
            .with_api_base(&config.ai_base_url);
        let llm_client = Client::with_config(llm_config);

        let db_pool = db::init_db().await?;

        // Load the embedding model off the async runtime; on first run this
        // downloads weights (~130 MB) and caches them on disk.
        tracing::info!("loading embedding model (first run downloads weights)…");
        let embedder = tokio::task::spawn_blocking(Embedder::load)
            .await
            .context("embedder load task panicked")??;

        // Create the ingest broadcast channel. We keep the sender in `Core` and
        // deliberately drop the initial receiver: subscribers are created on demand
        // via `subscribe_ingest`, and a broadcast channel with no receivers just
        // discards what it sends (which is exactly what we want before anyone is
        // listening).
        let (ingest_tx, _) = broadcast::channel(INGEST_BROADCAST_CAPACITY);

        let core = Self {
            http_client,
            config: Arc::new(config),
            db_pool,
            llm_client,
            embedder: Arc::new(embedder),
            ingest_tx,
        };

        core.spawn_background_tasks();
        Ok(core)
    }

    /// A read handle over the article store. Cheap to call; every adapter (agent
    /// tools, HTTP, MCP) should go through this rather than touching the pool.
    pub fn repository(&self) -> Repository {
        Repository::new(self.db_pool.clone(), self.embedder.clone())
    }

    /// Subscribe to newly-ingested articles. Each ingest tick that stores at least
    /// one new article publishes exactly one batch; the returned receiver yields
    /// those batches in order. Multiple subscribers each get their own copy (it's a
    /// fan-out broadcast), so connectors, a future SSE feed, etc. can all listen
    /// independently.
    ///
    /// The receiver is lossy under pressure: if a subscriber falls more than
    /// [`INGEST_BROADCAST_CAPACITY`] batches behind, `recv` returns a
    /// `RecvError::Lagged(n)` telling it how many it missed, then resumes. This is
    /// intentional — alerts may be dropped, but ingestion is never blocked.
    pub fn subscribe_ingest(&self) -> broadcast::Receiver<Arc<[ArticleSummary]>> {
        self.ingest_tx.subscribe()
    }

    /// Publish a batch of newly-ingested articles to all current subscribers. Called
    /// by the ingest loop after a tick. We convert the `Vec` into an `Arc<[_]>` so
    /// every subscriber shares one allocation instead of cloning the batch N times.
    ///
    /// `send` returns `Err` only when there are no subscribers; that's a normal,
    /// expected state (e.g. running without any connector), so we ignore it rather
    /// than treat it as a failure.
    pub(crate) fn publish_ingest(&self, articles: Vec<ArticleSummary>) {
        let _ = self.ingest_tx.send(Arc::from(articles));
    }

    /// Drive one assistant turn over `history`, returning a receiver of streamed
    /// events (tokens, tool-call notices, completion/error). The agent loop runs
    /// in a spawned task using this core's model, tools, and repository; adapters
    /// render the stream however they like (TUI widget, SSE, chat message edits).
    pub fn chat_stream(&self, history: Vec<ChatMessage>) -> UnboundedReceiver<StreamEvent> {
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(llm::prompt_stream(
            self.llm_client.clone(),
            history,
            self.config.ai_model.clone(),
            self.repository(),
            tx,
        ));
        rx
    }

    /// Spawn the long-lived background workers: a one-shot embedding backfill for
    /// articles indexed before semantic search existed, and the recurring ingest
    /// loop. Both run detached so adapters can start serving immediately.
    fn spawn_background_tasks(&self) {
        // Backfill embeddings for any articles indexed before semantic search
        // existed. Runs once in the background; no-op once caught up.
        let backfill_core = self.clone();
        tokio::spawn(async move {
            ingest::backfill_embeddings(&backfill_core).await;
        });

        // Refresh the news in the background, then keep re-ingesting on the
        // configured interval.
        let ingest_core = self.clone();
        let ingest_period =
            Duration::from_secs(ingest_core.config.toml_config.general.ingest_interval_secs);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(ingest_period);
            // Default `MissedTickBehavior::Burst` would fire back-to-back if a run
            // overruns the period; skip stale ticks so we resume on schedule.
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                // First tick completes immediately, so ingest runs once at startup.
                ticker.tick().await;
                let new_stored = ingest::run(&ingest_core).await;
                tracing::info!("Ingested {} new items", new_stored);
            }
        });
    }
}

/// Register the sqlite-vec extension so every SQLite connection sqlx opens has the
/// `vec0` virtual table and `vec_*` functions available. Must run before any
/// connection/pool is created.
fn register_sqlite_vec() {
    // SAFETY: `sqlite3_auto_extension` registers the sqlite-vec entrypoint for all
    // connections opened later by this process's SQLite. Called exactly once, before
    // the pool is built. The transmute adapts sqlite-vec's init fn to the C ABI
    // signature sqlite expects, which is the documented registration pattern.
    #[allow(unsafe_code)]
    unsafe {
        libsqlite3_sys::sqlite3_auto_extension(Some(std::mem::transmute::<
            *const (),
            unsafe extern "C" fn(
                *mut libsqlite3_sys::sqlite3,
                *mut *mut std::os::raw::c_char,
                *const libsqlite3_sys::sqlite3_api_routines,
            ) -> std::os::raw::c_int,
        >(sqlite_vec::sqlite3_vec_init as *const ())));
    }
}
