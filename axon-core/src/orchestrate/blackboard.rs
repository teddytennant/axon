use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

use crate::crdt::LWWRegister;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Shared blackboard for agent collaboration.
///
/// Backed by `LWWRegister<Vec<u8>>` (last-writer-wins) for merge semantics.
/// All entries are JSON-serializable values stored as raw bytes.
///
/// Integrates with the gossip loop via `"bb:"` key prefix on `StateSync` messages:
/// the gossip handler calls `merge()` when it receives state from remote peers,
/// and the periodic sync loop calls `export()` to share local state.
pub struct Blackboard {
    entries: RwLock<HashMap<String, LWWRegister<Vec<u8>>>>,
    node_id: String,
}

impl Blackboard {
    pub fn new(node_id: impl Into<String>) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            node_id: node_id.into(),
        }
    }

    /// Write raw bytes under a key. Uses wall-clock milliseconds for LWW ordering.
    pub async fn write(&self, key: &str, value: Vec<u8>) {
        let ts = now_ms();
        let mut entries = self.entries.write().await;
        let reg = entries.entry(key.to_string()).or_default();
        reg.set(value, ts);
    }

    /// Write a JSON-serializable value.
    pub async fn write_json<T: serde::Serialize>(
        &self,
        key: &str,
        value: &T,
    ) -> Result<(), serde_json::Error> {
        let bytes = serde_json::to_vec(value)?;
        self.write(key, bytes).await;
        Ok(())
    }

    /// Read raw bytes for a key.
    pub async fn read(&self, key: &str) -> Option<Vec<u8>> {
        self.entries
            .read()
            .await
            .get(key)
            .and_then(|r| r.get().cloned())
    }

    /// Read and deserialize a JSON value.
    pub async fn read_json<T: serde::de::DeserializeOwned>(
        &self,
        key: &str,
    ) -> Option<T> {
        let bytes = self.read(key).await?;
        serde_json::from_slice(&bytes).ok()
    }

    /// List all keys currently in the blackboard.
    pub async fn keys(&self) -> Vec<String> {
        self.entries
            .read()
            .await
            .iter()
            .filter(|(_, r)| r.get().is_some())
            .map(|(k, _)| k.clone())
            .collect()
    }

    /// Merge a remote register into a key (called by gossip loop).
    pub async fn merge(&self, key: &str, remote: &LWWRegister<Vec<u8>>) {
        let mut entries = self.entries.write().await;
        let local = entries.entry(key.to_string()).or_default();
        local.merge(remote);
    }

    /// Export all entries for state sync (gossip loop calls this).
    /// Keys are prefixed with `"bb:"` to namespace them in the mesh.
    pub async fn export(&self) -> Vec<(String, LWWRegister<Vec<u8>>)> {
        self.entries
            .read()
            .await
            .iter()
            .map(|(k, r)| (format!("bb:{k}"), r.clone()))
            .collect()
    }

    /// Snapshot of all entries for display (key, value_preview, timestamp).
    pub async fn snapshot(&self) -> Vec<(String, String, u64)> {
        self.entries
            .read()
            .await
            .iter()
            .filter_map(|(k, r)| {
                let bytes = r.get()?;
                let preview = if bytes.len() > 80 {
                    let s = String::from_utf8_lossy(&bytes[..80]).into_owned();
                    format!("{s}…")
                } else {
                    String::from_utf8_lossy(bytes).into_owned()
                };
                Some((k.clone(), preview, r.timestamp()))
            })
            .collect()
    }

    pub fn node_id(&self) -> &str {
        &self.node_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn blackboard_write_and_read() {
        let bb = Blackboard::new("node1");
        bb.write("key1", b"hello".to_vec()).await;
        let val = bb.read("key1").await.unwrap();
        assert_eq!(val, b"hello");
    }

    #[tokio::test]
    async fn blackboard_read_missing_returns_none() {
        let bb = Blackboard::new("node1");
        assert!(bb.read("nonexistent").await.is_none());
    }

    #[tokio::test]
    async fn blackboard_write_json_read_json() {
        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct Point {
            x: i32,
            y: i32,
        }
        let bb = Blackboard::new("node1");
        let p = Point { x: 3, y: 7 };
        bb.write_json("pos", &p).await.unwrap();
        let out: Point = bb.read_json("pos").await.unwrap();
        assert_eq!(out, p);
    }

    #[tokio::test]
    async fn blackboard_keys_lists_all() {
        let bb = Blackboard::new("node1");
        bb.write("a", vec![1]).await;
        bb.write("b", vec![2]).await;
        bb.write("c", vec![3]).await;
        let mut keys = bb.keys().await;
        keys.sort();
        assert_eq!(keys, vec!["a", "b", "c"]);
    }

    #[tokio::test]
    async fn blackboard_export_returns_all() {
        let bb = Blackboard::new("node1");
        bb.write("x", vec![1]).await;
        bb.write("y", vec![2]).await;
        let exported = bb.export().await;
        assert_eq!(exported.len(), 2);
        // Keys are prefixed with "bb:"
        let mut exported_keys: Vec<String> = exported.iter().map(|(k, _)| k.clone()).collect();
        exported_keys.sort();
        assert_eq!(exported_keys, vec!["bb:x", "bb:y"]);
    }

    #[tokio::test]
    async fn blackboard_merge_newer_wins() {
        let bb = Blackboard::new("node1");
        bb.write("k", b"local".to_vec()).await;

        // Remote has a newer timestamp
        let mut remote_reg: LWWRegister<Vec<u8>> = LWWRegister::new();
        remote_reg.set(b"remote".to_vec(), now_ms() + 10_000);
        bb.merge("k", &remote_reg).await;

        let val = bb.read("k").await.unwrap();
        assert_eq!(val, b"remote");
    }

    #[tokio::test]
    async fn blackboard_merge_older_ignored() {
        let bb = Blackboard::new("node1");
        // Write at a future timestamp
        {
            let mut entries = bb.entries.write().await;
            let reg = entries.entry("k".to_string()).or_default();
            reg.set(b"local-fresh".to_vec(), now_ms() + 100_000);
        }

        // Remote has an older timestamp
        let mut remote_reg: LWWRegister<Vec<u8>> = LWWRegister::new();
        remote_reg.set(b"stale-remote".to_vec(), 1); // timestamp=1
        bb.merge("k", &remote_reg).await;

        let val = bb.read("k").await.unwrap();
        assert_eq!(val, b"local-fresh");
    }

    #[tokio::test]
    async fn blackboard_overwrite_same_key() {
        let bb = Blackboard::new("node1");
        bb.write("k", b"first".to_vec()).await;
        // Small sleep to ensure monotonically increasing wall-clock
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        bb.write("k", b"second".to_vec()).await;
        let val = bb.read("k").await.unwrap();
        assert_eq!(val, b"second");
    }

    #[tokio::test]
    async fn blackboard_node_id_accessible() {
        let bb = Blackboard::new("my-node");
        assert_eq!(bb.node_id(), "my-node");
    }

    #[tokio::test]
    async fn blackboard_snapshot_shows_preview() {
        let bb = Blackboard::new("n");
        bb.write("greet", b"hello world".to_vec()).await;
        let snap = bb.snapshot().await;
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].0, "greet");
        assert_eq!(snap[0].1, "hello world");
    }

    #[tokio::test]
    async fn blackboard_snapshot_truncates_long_values() {
        let bb = Blackboard::new("n");
        let long_val = vec![b'x'; 200];
        bb.write("big", long_val).await;
        let snap = bb.snapshot().await;
        // Preview should be truncated at 80 chars + ellipsis
        assert!(snap[0].1.len() <= 85);
        assert!(snap[0].1.ends_with('…'));
    }

    #[tokio::test]
    async fn blackboard_export_keys_have_bb_prefix() {
        let bb = Blackboard::new("n");
        bb.write("status", b"ok".to_vec()).await;
        let exported = bb.export().await;
        assert_eq!(exported.len(), 1);
        assert!(exported[0].0.starts_with("bb:"));
    }
}
