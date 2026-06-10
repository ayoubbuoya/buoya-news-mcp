//! Configuration: serde structs mirroring `config.default.toml` (§9 of the spec).
//!
//! Loading order: parse `config.default.toml`, deep-merge `config.toml` on top of
//! it if present, then deserialize and `validate()`. Every field carries a serde
//! default so an entirely missing user file still yields a valid config; a partial
//! user file overrides only the keys it sets.

use std::path::Path;
use std::time::Duration;

use serde::Deserialize;

use crate::domain::Category;
use crate::error::ConfigError;

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

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    #[serde(default)]
    pub intervals: Intervals,
    #[serde(default)]
    pub staleness: Staleness,
    #[serde(default)]
    pub sources: Sources,
    #[serde(default)]
    pub scoring: Scoring,
    #[serde(default)]
    pub thresholds: Thresholds,
    #[serde(default)]
    pub dedup: Dedup,
    #[serde(default)]
    pub general: General,
    #[serde(default)]
    pub http: Http,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Intervals {
    pub news: HumanDuration,
    pub market: HumanDuration,
    pub security: HumanDuration,
}

impl Default for Intervals {
    fn default() -> Self {
        Self {
            news: HumanDuration(Duration::from_secs(12 * 3600)),
            market: HumanDuration(Duration::from_secs(12 * 3600)),
            security: HumanDuration(Duration::from_secs(12 * 3600)),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Staleness {
    pub news: HumanDuration,
    pub market: HumanDuration,
    pub security: HumanDuration,
    pub max_wait_ms: u64,
}

impl Default for Staleness {
    fn default() -> Self {
        Self {
            news: HumanDuration(Duration::from_secs(6 * 3600)),
            market: HumanDuration(Duration::from_secs(3600)),
            security: HumanDuration(Duration::from_secs(6 * 3600)),
            max_wait_ms: 8000,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sources {
    pub rss: Vec<RssSource>,
    pub hn: HnSource,
    pub defillama: DefillamaSource,
    pub coingecko: CoingeckoSource,
    pub cryptopanic: CryptopanicSource,
    pub reddit: RedditSource,
    pub arxiv: ArxivSource,
    pub huggingface: HuggingfaceSource,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RssSource {
    pub name: String,
    pub url: String,
    pub category: Category,
    #[serde(default = "default_authority")]
    pub authority: f64,
}

fn default_authority() -> f64 {
    0.5
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HnSource {
    pub enabled: bool,
    pub min_points: u32,
    pub keywords: Vec<String>,
}

impl Default for HnSource {
    fn default() -> Self {
        Self {
            enabled: true,
            min_points: 80,
            keywords: vec![
                "ai".into(),
                "llm".into(),
                "gpt".into(),
                "claude".into(),
                "model".into(),
            ],
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DefillamaSource {
    pub enabled: bool,
}

impl Default for DefillamaSource {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CoingeckoSource {
    pub enabled: bool,
    pub top_n: u32,
}

impl Default for CoingeckoSource {
    fn default() -> Self {
        Self {
            enabled: true,
            top_n: 100,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CryptopanicSource {
    pub enabled: bool,
    pub api_key: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RedditSource {
    pub enabled: bool,
    pub subreddits: Vec<String>,
    pub min_upvotes: u32,
}

impl Default for RedditSource {
    fn default() -> Self {
        Self {
            enabled: true,
            subreddits: vec![
                "CryptoCurrency".into(),
                "MachineLearning".into(),
                "LocalLLaMA".into(),
                "ethereum".into(),
            ],
            min_upvotes: 200,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ArxivSource {
    pub enabled: bool,
    pub categories: Vec<String>,
    pub max_per_run: u32,
}

impl Default for ArxivSource {
    fn default() -> Self {
        Self {
            enabled: true,
            categories: vec!["cs.AI".into(), "cs.LG".into(), "cs.CL".into()],
            max_per_run: 25,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HuggingfaceSource {
    pub enabled: bool,
}

impl Default for HuggingfaceSource {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Scoring {
    pub half_life_hours: f64,
    pub watchlist_multiplier: f64,
    pub weights: Weights,
    pub keywords: Vec<Keyword>,
}

impl Default for Scoring {
    fn default() -> Self {
        Self {
            half_life_hours: 48.0,
            watchlist_multiplier: 1.5,
            weights: Weights::default(),
            keywords: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Weights {
    pub coverage: f64,
    pub community: f64,
    pub keywords: f64,
    pub impact: f64,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            coverage: 3.0,
            community: 2.0,
            keywords: 1.5,
            impact: 4.0,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keyword {
    pub term: String,
    pub weight: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Thresholds {
    pub critical_loss_usd: f64,
    pub notable_loss_usd: f64,
    pub critical_pct: f64,
    pub notable_pct: f64,
    pub critical_coverage: u32,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            critical_loss_usd: 10_000_000.0,
            notable_loss_usd: 1_000_000.0,
            critical_pct: 15.0,
            notable_pct: 8.0,
            critical_coverage: 4,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Dedup {
    pub jaccard_threshold: f64,
    pub window_hours: u32,
}

impl Default for Dedup {
    fn default() -> Self {
        Self {
            jaccard_threshold: 0.6,
            window_hours: 48,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct General {
    pub watchlist: Vec<String>,
    pub retention_days: u32,
}

impl Default for General {
    fn default() -> Self {
        Self {
            watchlist: vec![
                "hedera".into(),
                "hbar".into(),
                "stellar".into(),
                "xlm".into(),
                "ethereum".into(),
            ],
            retention_days: 90,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Http {
    pub timeout_ms: u64,
    pub user_agent: String,
}

impl Default for Http {
    fn default() -> Self {
        Self {
            timeout_ms: 15000,
            user_agent: "buoya-news-mcp/1.0 (personal aggregator)".into(),
        }
    }
}

impl AppConfig {
    /// Load defaults from `default_path`, deep-merge `user_path` on top if it
    /// exists, deserialize, and validate.
    pub fn load(default_path: &Path, user_path: &Path) -> Result<Self, ConfigError> {
        let default_str = std::fs::read_to_string(default_path).map_err(|e| ConfigError::Read {
            path: default_path.display().to_string(),
            source: e,
        })?;
        let mut merged: toml::Value =
            toml::from_str(&default_str).map_err(|e| ConfigError::Parse {
                path: default_path.display().to_string(),
                source: e,
            })?;

        if user_path.exists() {
            let user_str = std::fs::read_to_string(user_path).map_err(|e| ConfigError::Read {
                path: user_path.display().to_string(),
                source: e,
            })?;
            let user: toml::Value = toml::from_str(&user_str).map_err(|e| ConfigError::Parse {
                path: user_path.display().to_string(),
                source: e,
            })?;
            merge(&mut merged, user);
        }

        let cfg: AppConfig = merged.try_into().map_err(|e| ConfigError::Parse {
            path: user_path.display().to_string(),
            source: e,
        })?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Deserialize from a single TOML string (no merge). Used by tests and any
    /// caller that already holds the merged source.
    pub fn from_str(toml_str: &str) -> Result<Self, ConfigError> {
        let cfg: AppConfig = toml::from_str(toml_str).map_err(|e| ConfigError::Parse {
            path: "<string>".into(),
            source: e,
        })?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Reject configurations that parse but cannot produce sensible behavior.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let inv = |m: String| ConfigError::Invalid(m);

        if self.scoring.half_life_hours <= 0.0 {
            return Err(inv(format!(
                "scoring.half_life_hours must be > 0, got {}",
                self.scoring.half_life_hours
            )));
        }
        if self.thresholds.critical_loss_usd <= 0.0
            || self.thresholds.notable_loss_usd <= 0.0
            || self.thresholds.critical_pct <= 0.0
            || self.thresholds.notable_pct <= 0.0
        {
            return Err(inv("all [thresholds] values must be > 0".into()));
        }
        if self.thresholds.notable_loss_usd > self.thresholds.critical_loss_usd {
            return Err(inv(
                "thresholds.notable_loss_usd must be <= critical_loss_usd".into(),
            ));
        }
        if self.thresholds.notable_pct > self.thresholds.critical_pct {
            return Err(inv("thresholds.notable_pct must be <= critical_pct".into()));
        }
        if !(0.0..=1.0).contains(&self.dedup.jaccard_threshold) {
            return Err(inv(format!(
                "dedup.jaccard_threshold must be in 0.0..=1.0, got {}",
                self.dedup.jaccard_threshold
            )));
        }
        for s in &self.sources.rss {
            if !(0.0..=1.0).contains(&s.authority) {
                return Err(inv(format!(
                    "sources.rss `{}` authority must be in 0.0..=1.0, got {}",
                    s.name, s.authority
                )));
            }
        }
        if self.sources.cryptopanic.enabled && self.sources.cryptopanic.api_key.trim().is_empty() {
            return Err(inv(
                "sources.cryptopanic is enabled but api_key is empty".into()
            ));
        }
        if !self.any_source_enabled() {
            return Err(inv("at least one source must be enabled".into()));
        }
        Ok(())
    }

    fn any_source_enabled(&self) -> bool {
        !self.sources.rss.is_empty()
            || self.sources.hn.enabled
            || self.sources.defillama.enabled
            || self.sources.coingecko.enabled
            || self.sources.cryptopanic.enabled
            || self.sources.reddit.enabled
            || self.sources.arxiv.enabled
            || self.sources.huggingface.enabled
    }
}

/// Deep-merge `overlay` into `base`. Tables merge key-by-key; every other value
/// type (including arrays) replaces wholesale. This matches the intent of a user
/// config: set a single scalar without restating its whole parent table, but
/// replace an array (e.g. the RSS feed list) entirely when you provide one.
fn merge(base: &mut toml::Value, overlay: toml::Value) {
    match (base, overlay) {
        (toml::Value::Table(base_tbl), toml::Value::Table(overlay_tbl)) => {
            for (k, v) in overlay_tbl {
                match base_tbl.get_mut(&k) {
                    Some(existing) => merge(existing, v),
                    None => {
                        base_tbl.insert(k, v);
                    }
                }
            }
        }
        (base_slot, overlay_val) => *base_slot = overlay_val,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const DEFAULT_TOML: &str = include_str!("../config.default.toml");

    #[test]
    fn defaults_parse_and_validate() {
        let cfg = AppConfig::from_str(DEFAULT_TOML).expect("default config must be valid");
        assert_eq!(cfg.intervals.news.as_duration().as_secs(), 12 * 3600);
        assert_eq!(cfg.staleness.market.as_duration().as_secs(), 3600);
        assert_eq!(cfg.sources.rss.len(), 4);
        assert_eq!(cfg.scoring.weights.impact, 4.0);
        assert_eq!(cfg.scoring.keywords.len(), 8);
        assert_eq!(cfg.thresholds.critical_loss_usd, 10_000_000.0);
        assert!(cfg.general.watchlist.contains(&"hedera".to_string()));
    }

    #[test]
    fn missing_user_file_uses_defaults() {
        let dir = std::env::temp_dir().join("bnm_cfg_missing");
        std::fs::create_dir_all(&dir).unwrap();
        let default_path = dir.join("config.default.toml");
        std::fs::write(&default_path, DEFAULT_TOML).unwrap();
        let user_path = dir.join("does_not_exist.toml");

        let cfg = AppConfig::load(&default_path, &user_path).expect("should load with defaults");
        assert_eq!(cfg.scoring.weights.coverage, 3.0);
    }

    #[test]
    fn user_file_overrides_only_provided_keys() {
        let dir = std::env::temp_dir().join("bnm_cfg_override");
        std::fs::create_dir_all(&dir).unwrap();
        let default_path = dir.join("config.default.toml");
        std::fs::write(&default_path, DEFAULT_TOML).unwrap();
        let user_path = dir.join("config.toml");
        std::fs::write(
            &user_path,
            r#"
[scoring.weights]
impact = 9.0

[intervals]
market = "30m"
"#,
        )
        .unwrap();

        let cfg = AppConfig::load(&default_path, &user_path).expect("should merge");
        // overridden
        assert_eq!(cfg.scoring.weights.impact, 9.0);
        assert_eq!(cfg.intervals.market.as_duration().as_secs(), 30 * 60);
        // sibling keys in the same tables retain defaults
        assert_eq!(cfg.scoring.weights.coverage, 3.0);
        assert_eq!(cfg.intervals.news.as_duration().as_secs(), 12 * 3600);
    }

    /// Build a config by merging an override TOML over the defaults, exercising
    /// the real `load` merge path. Returns the validation result.
    fn load_with_override(override_toml: &str) -> Result<AppConfig, ConfigError> {
        let dir = std::env::temp_dir().join(format!("bnm_cfg_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let default_path = dir.join("config.default.toml");
        std::fs::write(&default_path, DEFAULT_TOML).unwrap();
        let user_path = dir.join("config.toml");
        std::fs::write(&user_path, override_toml).unwrap();
        AppConfig::load(&default_path, &user_path)
    }

    #[test]
    fn invalid_half_life_is_rejected() {
        let err = load_with_override("[scoring]\nhalf_life_hours = 0.0\n").unwrap_err();
        assert!(matches!(err, ConfigError::Invalid(_)));
    }

    #[test]
    fn cryptopanic_enabled_without_key_is_rejected() {
        let err = load_with_override("[sources.cryptopanic]\nenabled = true\napi_key = \"\"\n")
            .unwrap_err();
        match err {
            ConfigError::Invalid(m) => assert!(m.contains("cryptopanic")),
            other => panic!("expected Invalid, got {other:?}"),
        }
    }
}
