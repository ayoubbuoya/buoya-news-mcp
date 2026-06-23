use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Crypto,
    Ai,
    Security,
    Market,
    Defi,
}

/// Author of a chat message. Persisted as a lowercase string in `chat_messages.role`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }
}

impl FromStr for Role {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "system" => Ok(Role::System),
            "user" => Ok(Role::User),
            "assistant" => Ok(Role::Assistant),
            other => Err(format!("unknown role: {other}")),
        }
    }
}

/// A saved chat conversation. One row in `chat_sessions`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ChatSession {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
}

/// A single message within a session. One row in `chat_messages`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ChatMessage {
    pub id: i64,
    pub session_id: String,
    pub role: Role,
    pub content: String,
    pub created_at: String,
    /// Human-readable labels for any tools the assistant invoked while producing
    /// this message. Shown in the UI for transparency and persisted to the DB
    /// (as a JSON array in `chat_messages.tool_calls`).
    pub tools_used: Vec<String>,
}

/// A point-in-time derivatives reading for one perpetual-futures symbol, as
/// returned by the Binance-futures fetcher. Numeric, not article-shaped — stored
/// in its own `derivatives` table. Every metric is optional so a partial endpoint
/// failure still records what we did get.
#[derive(Debug, Clone, PartialEq)]
pub struct DerivativesSnapshot {
    /// Exchange symbol, e.g. `"HBARUSDT"`.
    pub symbol: String,
    /// Open interest in base-asset units (contracts).
    pub open_interest: Option<f64>,
    /// Open interest in USD notional (`open_interest * mark_price`), computed when
    /// both inputs are present.
    pub open_interest_usd: Option<f64>,
    /// Latest funding rate as a fraction (e.g. `0.0001` = 0.01%).
    pub funding_rate: Option<f64>,
    /// Mark price in USD.
    pub mark_price: Option<f64>,
    /// Global long/short account ratio (longAccount / shortAccount). >1 = more
    /// accounts net long. This is the broad retail crowd.
    pub long_short_ratio: Option<f64>,
    /// Fraction of all accounts net long (0..1).
    pub long_account: Option<f64>,
    /// Fraction of all accounts net short (0..1).
    pub short_account: Option<f64>,
    /// Taker buy/sell volume ratio: aggressive buy volume / aggressive sell volume
    /// over the period. >1 = takers lifting offers (net buying pressure).
    pub taker_buy_sell_ratio: Option<f64>,
    /// Taker (aggressive) buy volume over the period, base-asset units.
    pub taker_buy_vol: Option<f64>,
    /// Taker (aggressive) sell volume over the period, base-asset units.
    pub taker_sell_vol: Option<f64>,
    /// Top-trader long/short ratio by **position** — how the largest accounts are
    /// positioned (the "smart money" view, vs the retail `long_short_ratio`).
    pub top_trader_long_short_ratio: Option<f64>,
    /// Fraction of top-trader positions net long (0..1).
    pub top_trader_long_account: Option<f64>,
    /// Fraction of top-trader positions net short (0..1).
    pub top_trader_short_account: Option<f64>,
    /// Next funding settlement time, RFC 3339.
    pub next_funding_time: Option<DateTime<Utc>>,
}

/// What a fetcher returns: minimal, source-shaped, not yet scored or stored.
#[derive(Debug, Clone)]
pub struct RawItem {
    pub title: String,
    pub url: String,
    pub source: String,
    pub category: Category,
    pub summary: Option<String>,
    pub content: Option<String>,
    pub published_at: DateTime<Utc>,
}
