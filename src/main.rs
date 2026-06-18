//! buoya-news-mcp entry point.
//!

// The lints table denies unwrap/expect crate-wide; tests are allowed to use them.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
// Config fields and helpers are defined ahead of their consumers (scheduler,
// fetchers, scoring — BNM-3/6/7/10). Lift this once those tasks wire them in.
#![allow(dead_code)]

mod config;
mod error;
mod fetchers;
mod state;
mod types;

use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use config::AppConfig;

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("{e:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<()> {
    let cfg = AppConfig::load(Path::new("config.default.toml"))?;

    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_millis(cfg.http.timeout_ms))
        .user_agent(&cfg.http.user_agent)
        .build()?;

    let app_state = state::AppState {
        http_client,
        config: Arc::new(cfg),
    };

    println!("App state: {app_state:?}");

    Ok(())
}
