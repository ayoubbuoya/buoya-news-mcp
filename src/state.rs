use std::sync::Arc;

use crate::config::AppConfig;

#[derive(Debug, Clone)]
pub struct AppState {
    pub http_client: reqwest::Client,
    pub config: Arc<AppConfig>,
}