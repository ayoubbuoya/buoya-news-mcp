use std::sync::Arc;

use async_openai::config::OpenAIConfig;

use crate::config::AppConfig;

#[derive(Debug, Clone)]
pub struct AppState {
    pub http_client: reqwest::Client,
    pub config: Arc<AppConfig>,
    pub llm_client: async_openai::Client<OpenAIConfig>,
}