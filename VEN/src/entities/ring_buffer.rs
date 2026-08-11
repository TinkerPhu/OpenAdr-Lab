//! R-46 — shared bounded ring container: push-and-evict-oldest-at-capacity.
//! Extracted from three near-identical hand-rolled implementations
//! (`state/mod.rs`'s notification ring, `state/event_log.rs`, and
//! `state/report_submissions.rs`). Zero-dependency generic Domain type — see
//! `openspec/changes/reactive-correction-notifications/design.md` D4/D5.

use std::collections::VecDeque;

/// A fixed-capacity FIFO ring: `push` evicts the single oldest entry once
/// length would exceed `capacity` before pushing the new one. Infallible —
/// no `DomainError` boundary is crossed (D5).
#[derive(Debug, Clone)]
pub struct RingBuffer<T> {
    capacity: usize,
    items: VecDeque<T>,
}

impl<T> RingBuffer<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            items: VecDeque::new(),
        }
    }

    /// Push a new item, evicting the oldest entry first if already at
    /// capacity. A capacity of 0 means every push evicts immediately (the
    /// buffer never holds anything).
    pub fn push(&mut self, item: T) {
        if self.capacity == 0 {
            return; // never holds anything
        }
        if self.items.len() >= self.capacity {
            self.items.pop_front();
        }
        self.items.push_back(item);
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Oldest-first iterator.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &T> {
        self.items.iter()
    }

    /// Oldest-first mutable iterator (needed by dedup-bump-in-place callers).
    pub fn iter_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut T> {
        self.items.iter_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_below_capacity_keeps_all_entries_oldest_first() {
        let mut ring: RingBuffer<i32> = RingBuffer::new(3);
        ring.push(1);
        ring.push(2);
        assert_eq!(ring.len(), 2);
        assert_eq!(ring.iter().copied().collect::<Vec<_>>(), vec![1, 2]);
    }

    #[test]
    fn push_past_capacity_evicts_single_oldest_entry() {
        let mut ring: RingBuffer<i32> = RingBuffer::new(3);
        for i in 0..5 {
            ring.push(i);
        }
        assert_eq!(ring.len(), 3);
        assert_eq!(ring.iter().copied().collect::<Vec<_>>(), vec![2, 3, 4]);
    }

    #[test]
    fn capacity_zero_never_retains_anything() {
        let mut ring: RingBuffer<i32> = RingBuffer::new(0);
        ring.push(1);
        ring.push(2);
        assert_eq!(ring.len(), 0);
        assert!(ring.is_empty());
        assert_eq!(ring.iter().count(), 0);
    }

    #[test]
    fn iter_mut_allows_bumping_entries_in_place() {
        let mut ring: RingBuffer<i32> = RingBuffer::new(3);
        ring.push(1);
        ring.push(2);
        for item in ring.iter_mut() {
            *item += 10;
        }
        assert_eq!(ring.iter().copied().collect::<Vec<_>>(), vec![11, 12]);
    }
}
