//! Core domain types shared across fetchers, pipeline, and tools.
//!
//! Only the types needed so far are defined here; the full set (RawItem,
//! NewsItem, Severity, ScoreBreakdown, Signals — §5 of the spec) is filled in
//! as the tasks that use them land.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Crypto,
    Ai,
    Security,
    Market,
}
