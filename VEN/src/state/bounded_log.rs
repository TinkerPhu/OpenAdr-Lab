//! A shared, concurrently-readable bounded log.
//!
//! `entities::ring_buffer::RingBuffer` (R-46) already unified the *container*.
//! What stayed duplicated was everything around it: `AppState` carried four
//! `Arc<RwLock<RingBuffer<T>>>` fields — notifications, the event log, report
//! submissions and flexibility history — and each grew its own cap constant,
//! its own `record`/`drain` accessor pair and its own
//! `ring_evicts_oldest_past_cap` test. Three of the four module docs said so
//! outright ("mirrors the notification ring pattern"), which is a copy
//! acknowledged rather than avoided.
//!
//! `BoundedLog<T>` is that shape named once. A new bounded diagnostic feed is
//! a field and a cap, not a module: the eviction behaviour below is tested
//! here and applies to every current and future one by construction.
//!
//! Ordering is the caller's choice at the read, not a property baked into the
//! field — `oldest_first` for a time series, `newest_first` for a log view.
//! These used to disagree per feed (`flexibility_history.rs` carried a comment
//! explaining it was the odd one out), which only read as an inconsistency
//! because the choice had no name.

use std::sync::Arc;
use tokio::sync::RwLock;

use crate::entities::ring_buffer::RingBuffer;

#[derive(Clone)]
pub struct BoundedLog<T> {
    ring: Arc<RwLock<RingBuffer<T>>>,
}

impl<T: Clone> BoundedLog<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            ring: Arc::new(RwLock::new(RingBuffer::new(capacity))),
        }
    }

    /// Append one entry, evicting the oldest once at capacity.
    pub async fn record(&self, item: T) {
        self.ring.write().await.push(item);
    }

    /// Every entry, oldest first — time-series order.
    pub async fn oldest_first(&self) -> Vec<T> {
        self.ring.read().await.iter().cloned().collect()
    }

    /// Every entry, newest first — log-view order.
    pub async fn newest_first(&self) -> Vec<T> {
        self.ring.read().await.iter().rev().cloned().collect()
    }

    /// Entries matching `pred`, oldest first.
    pub async fn oldest_first_where(&self, pred: impl Fn(&T) -> bool) -> Vec<T> {
        self.ring
            .read()
            .await
            .iter()
            .filter(|t| pred(t))
            .cloned()
            .collect()
    }

    /// Mutate the newest entry satisfying `pred` in place, returning a clone of
    /// the result. The one read-modify-write the feeds need (notification
    /// dedup bumps a matching entry's count rather than appending), kept here
    /// so it happens under a single write lock rather than by handing the ring
    /// out.
    pub async fn bump_newest_where(
        &self,
        pred: impl Fn(&T) -> bool,
        bump: impl FnOnce(&mut T),
    ) -> Option<T> {
        let mut ring = self.ring.write().await;
        let hit = ring.iter_mut().rev().find(|t| pred(t))?;
        bump(hit);
        Some(hit.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAP: usize = 4;

    /// The assertion that used to exist four times, once per feed, as
    /// `ring_evicts_oldest_past_cap`. It is a property of `BoundedLog`, so it
    /// is tested once and every feed inherits it.
    #[tokio::test]
    async fn record_evicts_oldest_past_cap() {
        let log = BoundedLog::new(CAP);
        for i in 0..CAP + 3 {
            log.record(i).await;
        }
        assert_eq!(log.oldest_first().await, vec![3, 4, 5, 6]);
    }

    #[tokio::test]
    async fn newest_first_reverses_oldest_first() {
        let log = BoundedLog::new(CAP);
        for i in 0..3 {
            log.record(i).await;
        }
        assert_eq!(log.oldest_first().await, vec![0, 1, 2]);
        assert_eq!(log.newest_first().await, vec![2, 1, 0]);
    }

    #[tokio::test]
    async fn oldest_first_where_filters_in_order() {
        let log = BoundedLog::new(CAP);
        for i in 0..4 {
            log.record(i).await;
        }
        assert_eq!(log.oldest_first_where(|i| i % 2 == 0).await, vec![0, 2]);
    }

    #[tokio::test]
    async fn bump_newest_where_hits_the_newest_match_only() {
        let log = BoundedLog::new(CAP);
        for i in [10, 20, 10] {
            log.record(i).await;
        }
        let bumped = log.bump_newest_where(|v| *v == 10, |v| *v += 1).await;
        assert_eq!(bumped, Some(11));
        assert_eq!(log.oldest_first().await, vec![10, 20, 11]);
    }

    #[tokio::test]
    async fn bump_newest_where_returns_none_with_no_match() {
        let log = BoundedLog::new(CAP);
        log.record(1).await;
        assert_eq!(log.bump_newest_where(|v| *v == 99, |v| *v += 1).await, None);
    }

    /// A clone shares the log — `AppState` is cloned per request handler, so a
    /// write through one handle must be visible through another.
    #[tokio::test]
    async fn clones_share_one_log() {
        let log = BoundedLog::new(CAP);
        let other = log.clone();
        log.record(7).await;
        assert_eq!(other.oldest_first().await, vec![7]);
    }
}
