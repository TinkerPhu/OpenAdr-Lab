use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

pub struct TtlCache {
    entries: RwLock<HashMap<String, (serde_json::Value, Instant, Duration)>>,
}

impl TtlCache {
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
        }
    }

    pub async fn get(&self, key: &str) -> Option<serde_json::Value> {
        let entries = self.entries.read().await;
        if let Some((value, created, ttl)) = entries.get(key) {
            if created.elapsed() < *ttl {
                return Some(value.clone());
            }
        }
        None
    }

    pub async fn set(&self, key: String, value: serde_json::Value, ttl: Duration) {
        let mut entries = self.entries.write().await;
        entries.insert(key, (value, Instant::now(), ttl));
    }

    pub async fn invalidate(&self, key: &str) {
        let mut entries = self.entries.write().await;
        entries.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn get_returns_value_within_ttl() {
        let cache = TtlCache::new();
        cache
            .set("k".into(), json!({"a": 1}), Duration::from_secs(60))
            .await;
        assert_eq!(cache.get("k").await, Some(json!({"a": 1})));
    }

    #[tokio::test]
    async fn get_returns_none_after_ttl_expiry() {
        let cache = TtlCache::new();
        // Zero TTL: `created.elapsed() < ttl` is false immediately — expired on first read.
        cache.set("k".into(), json!(42), Duration::ZERO).await;
        assert_eq!(cache.get("k").await, None);
    }

    #[tokio::test]
    async fn get_returns_none_for_missing_key() {
        let cache = TtlCache::new();
        assert_eq!(cache.get("absent").await, None);
    }

    #[tokio::test]
    async fn invalidate_removes_entry() {
        let cache = TtlCache::new();
        cache
            .set("k".into(), json!("v"), Duration::from_secs(60))
            .await;
        cache.invalidate("k").await;
        assert_eq!(cache.get("k").await, None);
    }

    #[tokio::test]
    async fn set_overwrites_existing_entry() {
        let cache = TtlCache::new();
        cache
            .set("k".into(), json!(1), Duration::from_secs(60))
            .await;
        cache
            .set("k".into(), json!(2), Duration::from_secs(60))
            .await;
        assert_eq!(cache.get("k").await, Some(json!(2)));
    }
}
