use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Capability advertised by an agent.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Capability {
    pub namespace: String,
    pub name: String,
    pub version: u32,
}

impl Capability {
    pub fn new(namespace: impl Into<String>, name: impl Into<String>, version: u32) -> Self {
        Self {
            namespace: namespace.into(),
            name: name.into(),
            version,
        }
    }

    /// Check if this capability matches a request (namespace and name match, version >=).
    pub fn matches(&self, requested: &Capability) -> bool {
        self.namespace == requested.namespace
            && self.name == requested.name
            && self.version >= requested.version
    }

    /// Canonical string representation: "namespace:name:vN"
    pub fn tag(&self) -> String {
        format!("{}:{}:v{}", self.namespace, self.name, self.version)
    }
}

/// Status of a completed task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    Success,
    Error(String),
    Timeout,
    NoCapability,
}

/// A task request sent to an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRequest {
    pub id: Uuid,
    pub capability: Capability,
    pub payload: Vec<u8>,
    pub timeout_ms: u64,
}

/// A task response from an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResponse {
    pub request_id: Uuid,
    pub status: TaskStatus,
    pub payload: Vec<u8>,
    pub duration_ms: u64,
}

/// Peer information shared across the mesh.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerInfo {
    pub peer_id: Vec<u8>,
    pub addr: String,
    pub capabilities: Vec<Capability>,
    pub last_seen: u64,
}

/// Messages exchanged between mesh nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    Ping { nonce: u64 },
    Pong { nonce: u64 },
    Announce(PeerInfo),
    Discover { capability: Capability },
    DiscoverResponse { peers: Vec<PeerInfo> },
    TaskRequest(TaskRequest),
    TaskResponse(TaskResponse),
    StateSync { key: String, data: Vec<u8> },
    Gossip { peers: Vec<PeerInfo> },
    /// A task forwarded from another node that couldn't handle it locally.
    /// Receiving nodes process this like TaskRequest but never forward again,
    /// preventing routing loops. Max one hop.
    ForwardedTask(TaskRequest),
}

impl Message {
    /// Serialize to bytes using bincode.
    pub fn encode(&self) -> Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
    }

    /// Deserialize from bytes.
    pub fn decode(data: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_matches_exact() {
        let cap = Capability::new("llm", "chat", 1);
        let req = Capability::new("llm", "chat", 1);
        assert!(cap.matches(&req));
    }

    #[test]
    fn capability_matches_higher_version() {
        let cap = Capability::new("llm", "chat", 2);
        let req = Capability::new("llm", "chat", 1);
        assert!(cap.matches(&req));
    }

    #[test]
    fn capability_no_match_lower_version() {
        let cap = Capability::new("llm", "chat", 1);
        let req = Capability::new("llm", "chat", 2);
        assert!(!cap.matches(&req));
    }

    #[test]
    fn capability_no_match_different_name() {
        let cap = Capability::new("llm", "chat", 1);
        let req = Capability::new("llm", "embed", 1);
        assert!(!cap.matches(&req));
    }

    #[test]
    fn capability_no_match_different_namespace() {
        let cap = Capability::new("llm", "chat", 1);
        let req = Capability::new("code", "chat", 1);
        assert!(!cap.matches(&req));
    }

    #[test]
    fn capability_tag_format() {
        let cap = Capability::new("llm", "chat", 1);
        assert_eq!(cap.tag(), "llm:chat:v1");
    }

    #[test]
    fn message_roundtrip_ping() {
        let msg = Message::Ping { nonce: 42 };
        let encoded = msg.encode().unwrap();
        let decoded = Message::decode(&encoded).unwrap();
        match decoded {
            Message::Ping { nonce } => assert_eq!(nonce, 42),
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn message_roundtrip_task_request() {
        let id = Uuid::new_v4();
        let msg = Message::TaskRequest(TaskRequest {
            id,
            capability: Capability::new("llm", "chat", 1),
            payload: b"hello world".to_vec(),
            timeout_ms: 5000,
        });
        let encoded = msg.encode().unwrap();
        let decoded = Message::decode(&encoded).unwrap();
        match decoded {
            Message::TaskRequest(req) => {
                assert_eq!(req.id, id);
                assert_eq!(req.capability.namespace, "llm");
                assert_eq!(req.payload, b"hello world");
                assert_eq!(req.timeout_ms, 5000);
            }
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn message_roundtrip_task_response() {
        let id = Uuid::new_v4();
        let msg = Message::TaskResponse(TaskResponse {
            request_id: id,
            status: TaskStatus::Success,
            payload: b"response data".to_vec(),
            duration_ms: 123,
        });
        let encoded = msg.encode().unwrap();
        let decoded = Message::decode(&encoded).unwrap();
        match decoded {
            Message::TaskResponse(resp) => {
                assert_eq!(resp.request_id, id);
                assert_eq!(resp.status, TaskStatus::Success);
                assert_eq!(resp.duration_ms, 123);
            }
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn message_roundtrip_announce() {
        let msg = Message::Announce(PeerInfo {
            peer_id: vec![1, 2, 3],
            addr: "127.0.0.1:4242".to_string(),
            capabilities: vec![Capability::new("echo", "ping", 1)],
            last_seen: 1000,
        });
        let encoded = msg.encode().unwrap();
        let decoded = Message::decode(&encoded).unwrap();
        match decoded {
            Message::Announce(info) => {
                assert_eq!(info.peer_id, vec![1, 2, 3]);
                assert_eq!(info.capabilities.len(), 1);
            }
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn message_roundtrip_gossip() {
        let peers = vec![
            PeerInfo {
                peer_id: vec![1],
                addr: "10.0.0.1:4242".to_string(),
                capabilities: vec![],
                last_seen: 100,
            },
            PeerInfo {
                peer_id: vec![2],
                addr: "10.0.0.2:4242".to_string(),
                capabilities: vec![Capability::new("llm", "chat", 1)],
                last_seen: 200,
            },
        ];
        let msg = Message::Gossip { peers };
        let encoded = msg.encode().unwrap();
        let decoded = Message::decode(&encoded).unwrap();
        match decoded {
            Message::Gossip { peers } => assert_eq!(peers.len(), 2),
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn message_roundtrip_forwarded_task() {
        let id = Uuid::new_v4();
        let msg = Message::ForwardedTask(TaskRequest {
            id,
            capability: Capability::new("llm", "chat", 1),
            payload: b"forwarded payload".to_vec(),
            timeout_ms: 10000,
        });
        let encoded = msg.encode().unwrap();
        let decoded = Message::decode(&encoded).unwrap();
        match decoded {
            Message::ForwardedTask(req) => {
                assert_eq!(req.id, id);
                assert_eq!(req.capability.namespace, "llm");
                assert_eq!(req.payload, b"forwarded payload");
            }
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn message_decode_invalid_data() {
        let result = Message::decode(&[0xFF, 0xFF, 0xFF]);
        assert!(result.is_err());
    }

    #[test]
    fn task_status_error_contains_message() {
        let status = TaskStatus::Error("something broke".to_string());
        match status {
            TaskStatus::Error(msg) => assert_eq!(msg, "something broke"),
            _ => panic!("wrong status"),
        }
    }
}
