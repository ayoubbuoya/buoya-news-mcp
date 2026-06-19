//! HTTP route handlers, one module per resource. Each module owns its handlers,
//! request/response DTOs, and a `configure` fn that registers its routes; this
//! module aggregates them into a single `configure` the app builder mounts.

pub mod articles;
pub mod chat;
pub mod market;
pub mod sessions;
pub mod system;

use actix_web::web;

/// Register every route group on the app's service config.
pub fn configure(cfg: &mut web::ServiceConfig) {
    system::configure(cfg);
    articles::configure(cfg);
    market::configure(cfg);
    sessions::configure(cfg);
    chat::configure(cfg);
}
