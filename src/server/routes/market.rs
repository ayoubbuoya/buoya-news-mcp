//! Market routes: the latest daily snapshot per market source.

use actix_web::{HttpResponse, Result as ActixResult, web};

use crate::core::Core;
use crate::server::error::internal;

/// Latest daily snapshot per market source (Fear & Greed, market overview, DeFi TVL).
#[utoipa::path(
    get,
    path = "/market/snapshot",
    tag = "market",
    responses(
        (status = 200, description = "Newest snapshot per source", body = [crate::core::repository::SnapshotItem]),
        (status = 500, description = "Query failed", body = crate::server::error::ErrorResponse),
    )
)]
pub async fn market_snapshot(core: web::Data<Core>) -> ActixResult<HttpResponse> {
    let snapshots = core.repository().market_snapshot().await.map_err(internal)?;
    Ok(HttpResponse::Ok().json(snapshots))
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/market/snapshot", web::get().to(market_snapshot));
}
