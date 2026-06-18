//! buoya-news-mcp entry point.
//!
//! For now this only loads and validates configuration; tracing init, DB
//! migration, the fetch scheduler, and the stdio MCP server are wired in by
//! later tasks (BNM-3, BNM-9, BNM-10).

// The lints table denies unwrap/expect crate-wide; tests are allowed to use them.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
// Config fields and helpers are defined ahead of their consumers (scheduler,
// fetchers, scoring — BNM-3/6/7/10). Lift this once those tasks wire them in.
#![allow(dead_code)]

mod config;
mod error;
mod types;

use std::path::Path;
use std::process::ExitCode;

use config::AppConfig;

fn main() -> ExitCode {
    let config_path = Path::new("config.default.toml");

    match AppConfig::load(config_path) {
        Ok(cfg) => {
            // Logging goes to stderr; stdout is reserved for the MCP protocol.
            eprintln!(
                "config loaded: {} RSS feed(s), news interval {:?}, watchlist of {} term(s)",
                cfg.sources.rss.len(),
                5,
                cfg.general.watchlist.len(),
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
