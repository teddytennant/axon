use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Grow-only counter — each node increments its own slot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GCounter {
    counts: HashMap<String, u64>,
}

impl GCounter {
    pub fn new() -> Self {
        Self {
            counts: HashMap::new(),
        }
    }

    /// Increment the counter for a specific node.
    pub fn increment(&mut self, node_id: &str) {
        let entry = self.counts.entry(node_id.to_string()).or_insert(0);
        *entry = entry.saturating_add(1);
    }

    /// Increment by a specific amount.
    pub fn increment_by(&mut self, node_id: &str, amount: u64) {
        let entry = self.counts.entry(node_id.to_string()).or_insert(0);
        *entry = entry.saturating_add(amount);
    }

    /// Get the total count across all nodes.
    pub fn value(&self) -> u64 {
        self.counts.values().fold(0u64, |acc, &v| acc.saturating_add(v))
    }

    /// Get the count for a specific node.
    pub fn node_value(&self, node_id: &str) -> u64 {
        self.counts.get(node_id).copied().unwrap_or(0)
    }

    /// Merge with another GCounter (take max per node).
    pub fn merge(&mut self, other: &GCounter) {
        for (node, &count) in &other.counts {
            let entry = self.counts.entry(node.clone()).or_insert(0);
            *entry = (*entry).max(count);
        }
    }
}

impl Default for GCounter {
    fn default() -> Self {
        Self::new()
    }
}

/// Last-Writer-Wins Register — stores a value with a logical timestamp.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LWWRegister<T: Clone + PartialEq> {
    value: Option<T>,
    timestamp: u64,
}

impl<T: Clone + PartialEq> LWWRegister<T> {
    pub fn new() -> Self {
        Self {
            value: None,
            timestamp: 0,
        }
    }

    /// Set the value with a given timestamp.
    pub fn set(&mut self, value: T, timestamp: u64) {
        if timestamp > self.timestamp {
            self.value = Some(value);
            self.timestamp = timestamp;
        }
    }

    /// Get the current value.
    pub fn get(&self) -> Option<&T> {
        self.value.as_ref()
    }

    /// Get the current timestamp.
    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    /// Merge with another register (higher timestamp wins).
    pub fn merge(&mut self, other: &LWWRegister<T>) {
        if other.timestamp > self.timestamp {
            self.value = other.value.clone();
            self.timestamp = other.timestamp;
        }
    }
}

impl<T: Clone + PartialEq> Default for LWWRegister<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Observed-Remove Set — supports both add and remove with unique tags.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ORSet<T: Clone + Eq + std::hash::Hash> {
    /// Map from element to set of unique tags (node_id, counter)
    elements: HashMap<T, HashSet<(String, u64)>>,
    /// Tombstone tags — removed elements
    tombstones: HashSet<(String, u64)>,
    /// Per-node counters for generating unique tags
    counters: HashMap<String, u64>,
}

impl<T: Clone + Eq + std::hash::Hash> ORSet<T> {
    pub fn new() -> Self {
        Self {
            elements: HashMap::new(),
            tombstones: HashSet::new(),
            counters: HashMap::new(),
        }
    }

    /// Add an element tagged with the given node_id.
    pub fn add(&mut self, node_id: &str, element: T) {
        let counter = self.counters.entry(node_id.to_string()).or_insert(0);
        *counter = counter.saturating_add(1);
        let tag = (node_id.to_string(), *counter);

        self.elements
            .entry(element)
            .or_default()
            .insert(tag);
    }

    /// Remove an element by tombstoning all its current tags.
    pub fn remove(&mut self, element: &T) {
        if let Some(tags) = self.elements.remove(element) {
            for tag in tags {
                self.tombstones.insert(tag);
            }
        }
    }

    /// Check if the set contains an element.
    pub fn contains(&self, element: &T) -> bool {
        self.elements
            .get(element)
            .map(|tags| !tags.is_empty())
            .unwrap_or(false)
    }

    /// Get all elements in the set.
    pub fn elements(&self) -> Vec<&T> {
        self.elements
            .iter()
            .filter(|(_, tags)| !tags.is_empty())
            .map(|(elem, _)| elem)
            .collect()
    }

    /// Number of elements in the set.
    pub fn len(&self) -> usize {
        self.elements
            .iter()
            .filter(|(_, tags)| !tags.is_empty())
            .count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Merge with another ORSet.
    pub fn merge(&mut self, other: &ORSet<T>) {
        // Merge tombstones
        for tag in &other.tombstones {
            self.tombstones.insert(tag.clone());
        }

        // Merge elements: union of tags, minus tombstones
        for (elem, other_tags) in &other.elements {
            let tags = self.elements.entry(elem.clone()).or_default();
            for tag in other_tags {
                if !self.tombstones.contains(tag) {
                    tags.insert(tag.clone());
                }
            }
        }

        // Remove tombstoned tags from existing elements
        for (_elem, tags) in self.elements.iter_mut() {
            tags.retain(|tag| !self.tombstones.contains(tag));
        }

        // Merge counters (take max)
        for (node, &counter) in &other.counters {
            let entry = self.counters.entry(node.clone()).or_insert(0);
            *entry = (*entry).max(counter);
        }
    }
}

impl<T: Clone + Eq + std::hash::Hash> Default for ORSet<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== GCounter Tests =====

    #[test]
    fn gcounter_new_is_zero() {
        let c = GCounter::new();
        assert_eq!(c.value(), 0);
    }

    #[test]
    fn gcounter_single_node_increment() {
        let mut c = GCounter::new();
        c.increment("node1");
        c.increment("node1");
        c.increment("node1");
        assert_eq!(c.value(), 3);
        assert_eq!(c.node_value("node1"), 3);
    }

    #[test]
    fn gcounter_multi_node() {
        let mut c = GCounter::new();
        c.increment("node1");
        c.increment("node1");
        c.increment("node2");
        assert_eq!(c.value(), 3);
        assert_eq!(c.node_value("node1"), 2);
        assert_eq!(c.node_value("node2"), 1);
    }

    #[test]
    fn gcounter_increment_by() {
        let mut c = GCounter::new();
        c.increment_by("node1", 10);
        c.increment_by("node1", 5);
        assert_eq!(c.value(), 15);
    }

    #[test]
    fn gcounter_node_value_missing() {
        let c = GCounter::new();
        assert_eq!(c.node_value("nonexistent"), 0);
    }

    #[test]
    fn gcounter_merge_takes_max() {
        let mut c1 = GCounter::new();
        c1.increment_by("node1", 5);
        c1.increment_by("node2", 3);

        let mut c2 = GCounter::new();
        c2.increment_by("node1", 3);
        c2.increment_by("node2", 7);
        c2.increment_by("node3", 2);

        c1.merge(&c2);
        assert_eq!(c1.node_value("node1"), 5); // max(5, 3)
        assert_eq!(c1.node_value("node2"), 7); // max(3, 7)
        assert_eq!(c1.node_value("node3"), 2); // new from c2
        assert_eq!(c1.value(), 14);
    }

    #[test]
    fn gcounter_merge_is_commutative() {
        let mut c1 = GCounter::new();
        c1.increment_by("a", 5);
        let mut c2 = GCounter::new();
        c2.increment_by("b", 3);

        let mut r1 = c1.clone();
        r1.merge(&c2);

        let mut r2 = c2.clone();
        r2.merge(&c1);

        assert_eq!(r1.value(), r2.value());
    }

    #[test]
    fn gcounter_merge_is_idempotent() {
        let mut c1 = GCounter::new();
        c1.increment_by("a", 5);
        let mut c2 = GCounter::new();
        c2.increment_by("b", 3);

        c1.merge(&c2);
        let v1 = c1.value();
        c1.merge(&c2);
        assert_eq!(c1.value(), v1);
    }

    #[test]
    fn gcounter_saturating_increment() {
        let mut c = GCounter::new();
        c.increment_by("node1", u64::MAX);
        c.increment("node1");
        assert_eq!(c.node_value("node1"), u64::MAX);
    }

    #[test]
    fn gcounter_value_saturating_across_nodes() {
        let mut c = GCounter::new();
        c.increment_by("node1", u64::MAX);
        c.increment_by("node2", 1);
        assert_eq!(c.value(), u64::MAX);
    }

    // ===== LWWRegister Tests =====

    #[test]
    fn lww_register_new_is_none() {
        let r: LWWRegister<String> = LWWRegister::new();
        assert!(r.get().is_none());
        assert_eq!(r.timestamp(), 0);
    }

    #[test]
    fn lww_register_set_and_get() {
        let mut r = LWWRegister::new();
        r.set("hello".to_string(), 1);
        assert_eq!(r.get(), Some(&"hello".to_string()));
        assert_eq!(r.timestamp(), 1);
    }

    #[test]
    fn lww_register_later_timestamp_wins() {
        let mut r = LWWRegister::new();
        r.set("first".to_string(), 1);
        r.set("second".to_string(), 2);
        assert_eq!(r.get(), Some(&"second".to_string()));
    }

    #[test]
    fn lww_register_earlier_timestamp_ignored() {
        let mut r = LWWRegister::new();
        r.set("first".to_string(), 5);
        r.set("second".to_string(), 3);
        assert_eq!(r.get(), Some(&"first".to_string()));
    }

    #[test]
    fn lww_register_equal_timestamp_ignored() {
        let mut r = LWWRegister::new();
        r.set("first".to_string(), 5);
        r.set("second".to_string(), 5);
        assert_eq!(r.get(), Some(&"first".to_string()));
    }

    #[test]
    fn lww_register_merge_higher_wins() {
        let mut r1 = LWWRegister::new();
        r1.set("old".to_string(), 1);

        let mut r2 = LWWRegister::new();
        r2.set("new".to_string(), 2);

        r1.merge(&r2);
        assert_eq!(r1.get(), Some(&"new".to_string()));
    }

    #[test]
    fn lww_register_merge_lower_ignored() {
        let mut r1 = LWWRegister::new();
        r1.set("latest".to_string(), 5);

        let mut r2 = LWWRegister::new();
        r2.set("old".to_string(), 2);

        r1.merge(&r2);
        assert_eq!(r1.get(), Some(&"latest".to_string()));
    }

    #[test]
    fn lww_register_merge_commutative() {
        let mut r1 = LWWRegister::new();
        r1.set("a".to_string(), 1);
        let mut r2 = LWWRegister::new();
        r2.set("b".to_string(), 2);

        let mut m1 = r1.clone();
        m1.merge(&r2);
        let mut m2 = r2.clone();
        m2.merge(&r1);

        assert_eq!(m1.get(), m2.get());
    }

    // ===== ORSet Tests =====

    #[test]
    fn orset_new_is_empty() {
        let s: ORSet<String> = ORSet::new();
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
    }

    #[test]
    fn orset_add_and_contains() {
        let mut s = ORSet::new();
        s.add("node1", "hello".to_string());
        assert!(s.contains(&"hello".to_string()));
        assert!(!s.contains(&"world".to_string()));
    }

    #[test]
    fn orset_add_multiple() {
        let mut s = ORSet::new();
        s.add("node1", "a".to_string());
        s.add("node1", "b".to_string());
        s.add("node2", "c".to_string());
        assert_eq!(s.len(), 3);
    }

    #[test]
    fn orset_add_duplicate_still_one_element() {
        let mut s = ORSet::new();
        s.add("node1", "a".to_string());
        s.add("node1", "a".to_string());
        assert_eq!(s.len(), 1);
        assert!(s.contains(&"a".to_string()));
    }

    #[test]
    fn orset_remove() {
        let mut s = ORSet::new();
        s.add("node1", "a".to_string());
        s.add("node1", "b".to_string());
        s.remove(&"a".to_string());
        assert!(!s.contains(&"a".to_string()));
        assert!(s.contains(&"b".to_string()));
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn orset_remove_nonexistent_is_noop() {
        let mut s = ORSet::new();
        s.add("node1", "a".to_string());
        s.remove(&"b".to_string());
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn orset_add_after_remove() {
        let mut s = ORSet::new();
        s.add("node1", "a".to_string());
        s.remove(&"a".to_string());
        assert!(!s.contains(&"a".to_string()));

        s.add("node1", "a".to_string());
        assert!(s.contains(&"a".to_string()));
    }

    #[test]
    fn orset_merge_union() {
        let mut s1 = ORSet::new();
        s1.add("node1", "a".to_string());

        let mut s2 = ORSet::new();
        s2.add("node2", "b".to_string());

        s1.merge(&s2);
        assert!(s1.contains(&"a".to_string()));
        assert!(s1.contains(&"b".to_string()));
        assert_eq!(s1.len(), 2);
    }

    #[test]
    fn orset_merge_concurrent_add_remove() {
        // node1 adds "x", node2 also adds "x" then removes it
        let mut s1 = ORSet::new();
        s1.add("node1", "x".to_string());

        let mut s2 = ORSet::new();
        s2.add("node2", "x".to_string());
        s2.remove(&"x".to_string());

        // After merge, node1's add should survive (add wins over concurrent remove)
        s1.merge(&s2);
        assert!(s1.contains(&"x".to_string()));
    }

    #[test]
    fn orset_merge_remove_propagates() {
        let mut s1 = ORSet::new();
        s1.add("node1", "x".to_string());

        // s2 is a copy where the same element is removed
        let mut s2 = s1.clone();
        s2.remove(&"x".to_string());

        s1.merge(&s2);
        assert!(!s1.contains(&"x".to_string()));
    }

    #[test]
    fn orset_elements_returns_all() {
        let mut s = ORSet::new();
        s.add("n", "a".to_string());
        s.add("n", "b".to_string());
        s.add("n", "c".to_string());

        let mut elems: Vec<String> = s.elements().into_iter().cloned().collect();
        elems.sort();
        assert_eq!(elems, vec!["a", "b", "c"]);
    }

    #[test]
    fn orset_merge_idempotent() {
        let mut s1 = ORSet::new();
        s1.add("n1", "a".to_string());
        let mut s2 = ORSet::new();
        s2.add("n2", "b".to_string());

        s1.merge(&s2);
        let len1 = s1.len();
        s1.merge(&s2);
        assert_eq!(s1.len(), len1);
    }
}
