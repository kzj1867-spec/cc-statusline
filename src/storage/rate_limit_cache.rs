//! Rate limit file cache
//!
//! Provides file-based caching for GLM API usage statistics.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::api::UsageStats;

/// Cached entry with timestamp
#[derive(Debug, Serialize, Deserialize)]
pub struct CachedRateLimit {
    /// The cached usage stats
    pub stats: UsageStats,
    /// When this was cached (seconds since epoch)
    pub cached_at: u64,
}

impl CachedRateLimit {
    /// Create a new cache entry
    #[must_use]
    pub fn new(stats: UsageStats) -> Self {
        let cached_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self { stats, cached_at }
    }

    /// Check if this cache entry has expired
    #[must_use]
    pub fn is_expired(&self, ttl_secs: u64) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        now.saturating_sub(self.cached_at) > ttl_secs
    }
}

/// Resolve the cache file path based on the storage directory
fn cache_file_path() -> Option<PathBuf> {
    let base = std::env::var("STATUSLINE_STORAGE_PATH")
        .ok()
        .map(PathBuf::from)
        .or_else(|| crate::utils::home_dir().map(|home| home.join(".claude")))?;

    Some(base.join("statusline-pro").join("rate_limit_cache.json"))
}

/// Read cached rate limit data from file
///
/// Returns `None` if the file doesn't exist or can't be parsed.
#[must_use]
pub fn read_cache() -> Option<CachedRateLimit> {
    let path = cache_file_path()?;

    if !path.exists() {
        return None;
    }

    let content = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Write rate limit data to cache file
///
/// # Errors
/// Returns an error if the cache path cannot be resolved, the parent directory
/// cannot be created, or the file cannot be written.
pub fn write_cache(entry: &CachedRateLimit) -> Result<()> {
    let path = cache_file_path().context("Cannot resolve rate limit cache path")?;

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create cache dir: {}", parent.display()))?;
        }
    }

    let content = serde_json::to_string(entry).context("Failed to serialize cache entry")?;
    fs::write(&path, content)
        .with_context(|| format!("Failed to write cache file: {}", path.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{QuotaUsage, UsageStats};

    fn make_test_stats() -> UsageStats {
        UsageStats {
            token_usage: Some(QuotaUsage {
                used: 320,
                limit: 1000,
                percentage: 32,
                time_window: "5h".to_string(),
                reset_at: None,
            }),
            mcp_usage: Some(QuotaUsage {
                used: 15,
                limit: 100,
                percentage: 15,
                time_window: "30d".to_string(),
                reset_at: None,
            }),
            weekly_token_usage: None,
        }
    }

    #[test]
    fn test_cached_entry_new_has_timestamp() {
        let stats = make_test_stats();
        let entry = CachedRateLimit::new(stats);
        assert!(entry.cached_at > 0, "Timestamp should be positive");
    }

    #[test]
    fn test_cached_entry_not_expired() {
        let stats = make_test_stats();
        let entry = CachedRateLimit::new(stats);
        assert!(!entry.is_expired(300), "Fresh entry should not be expired");
    }

    #[test]
    fn test_cached_entry_is_expired() {
        let stats = make_test_stats();
        let mut entry = CachedRateLimit::new(stats);
        // Set cached_at to 0 → always expired
        entry.cached_at = 0;
        assert!(entry.is_expired(300), "Old entry should be expired");
    }

    #[test]
    fn test_cached_entry_serialization_roundtrip() {
        let stats = make_test_stats();
        let entry = CachedRateLimit::new(stats);
        let json = serde_json::to_string(&entry).ok();
        assert!(json.is_some(), "Serialization should succeed");
        let deserialized: CachedRateLimit =
            serde_json::from_str(json.as_deref().unwrap_or_default())
                .ok()
                .unwrap_or(CachedRateLimit {
                    stats: UsageStats {
                        token_usage: None,
                        mcp_usage: None,
                        weekly_token_usage: None,
                    },
                    cached_at: 0,
                });
        assert_eq!(
            deserialized
                .stats
                .token_usage
                .as_ref()
                .map_or(0, |u| u.percentage),
            32
        );
        assert_eq!(
            deserialized.stats.mcp_usage.as_ref().map_or(0, |u| u.used),
            15
        );
        assert_eq!(deserialized.cached_at, entry.cached_at);
    }
}
