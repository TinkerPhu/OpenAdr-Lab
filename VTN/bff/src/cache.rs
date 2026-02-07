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
