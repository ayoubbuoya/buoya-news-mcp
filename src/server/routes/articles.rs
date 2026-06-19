//! Article routes: list, search, and fetch-by-id over the local article store.

use actix_web::{HttpResponse, Result as ActixResult, web};
use serde::Deserialize;
use utoipa::IntoParams;

use crate::core::Core;
use crate::server::error::{internal, not_found};

/// Default number of articles a list/search route returns when unspecified.
const DEFAULT_LIMIT: i64 = 20;
/// Hard cap so one request cannot ask for an unbounded result set.
const MAX_LIMIT: i64 = 50;

/// Clamp a caller-provided limit into `1..=MAX_LIMIT`, defaulting when absent.
fn resolve_limit(limit: Option<i64>) -> i64 {
    limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListQuery {
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
        (status = 200, description = "Most recent articles", body = [crate::core::repository::ArticleSummary]),
        (status = 500, description = "Query failed", body = crate::server::error::ErrorResponse),
    )
)]
pub async fn list_articles(
    core: web::Data<Core>,
    query: web::Query<ListQuery>,
) -> ActixResult<HttpResponse> {
    let limit = resolve_limit(query.limit);
    let articles = core
        .repository()
        .list_recent(query.category.as_deref(), limit)
        .await
        .map_err(internal)?;
    Ok(HttpResponse::Ok().json(articles))
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SearchQuery {
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
        (status = 200, description = "Matching articles, most relevant first", body = [crate::core::repository::ArticleSummary]),
        (status = 500, description = "Search failed", body = crate::server::error::ErrorResponse),
    )
)]
pub async fn search_articles(
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
    .map_err(internal)?;
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
        (status = 200, description = "The full article", body = crate::core::repository::Article),
        (status = 404, description = "No article with that id", body = crate::server::error::ErrorResponse),
        (status = 500, description = "Lookup failed", body = crate::server::error::ErrorResponse),
    )
)]
pub async fn get_article(core: web::Data<Core>, path: web::Path<i64>) -> ActixResult<HttpResponse> {
    let id = path.into_inner();
    match core.repository().get_article(id).await.map_err(internal)? {
        Some(article) => Ok(HttpResponse::Ok().json(article)),
        None => Ok(not_found(format!("no article with id {id}"))),
    }
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/articles", web::get().to(list_articles))
        .route("/articles/search", web::get().to(search_articles))
        .route("/articles/{id}", web::get().to(get_article));
}
