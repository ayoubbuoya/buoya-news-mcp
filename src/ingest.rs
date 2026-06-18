use chrono::Utc;

use crate::{fetchers, state::AppState};

/// Fetch all enabled RSS feeds and persist them into the db. Returns the number of
/// newly-stored items.
pub async fn run(app_state: &AppState) -> usize {
    let mut new_stored: usize = 0;

    for source in &app_state.config.toml_config.sources.rss {
        match fetchers::rss::fetch_rss_source(&app_state.http_client, source).await {
            Ok(raw_items) => {
                for item in &raw_items {
                    let category = format!("{:?}", item.category).to_lowercase();
                    let published_at = item.published_at.to_rfc3339();
                    
                    let result = sqlx::query(
                        "INSERT OR IGNORE INTO articles (title, url, source, category, summary, content, published_at)
                         VALUES (?, ?, ?, ?, ?, ?, ?)",
                    )
                    .bind(&item.title)
                    .bind(&item.url)
                    .bind(&item.source)
                    .bind(&category)
                    .bind(&item.summary)
                    .bind(&item.content)
                    .bind(&published_at)
                    .execute(&app_state.db_pool)
                    .await;

                    match result {
                        Ok(r) if r.rows_affected() > 0 => new_stored += 1,
                        Ok(_) => {}
                        Err(e) => tracing::error!("Failed to insert article {}: {}", item.url, e),
                    }
                }
            }
            Err(e) => {
                tracing::error!(
                    "Failed to fetch rss source for source {} at {}: {}",
                    source.name,
                    Utc::now(),
                    e
                );
            }
        }
    }

    new_stored
}
