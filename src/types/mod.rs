//! Core domain types shared across fetchers, pipeline, and tools.

use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Crypto,
    Ai,
    Security,
    Market,
}

/// A humantime duration string (`"12h"`, `"30m"`) parsed eagerly at load.
///
/// Wraps `std::time::Duration` so config structs can hold real durations while
/// still deserializing from the human-friendly TOML string form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HumanDuration(pub Duration);

impl<'de> Deserialize<'de> for HumanDuration {
    fn deserialize<D>(de: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(de)?;
        let d = humantime::parse_duration(&s).map_err(serde::de::Error::custom)?;
        Ok(HumanDuration(d))
    }
}

impl HumanDuration {
    pub fn as_duration(self) -> Duration {
        self.0
    }
}
