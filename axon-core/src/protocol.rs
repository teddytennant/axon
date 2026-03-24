use crate::mcp::McpToolSchema;
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
    Ping {
        nonce: u64,
    },
    Pong {
        nonce: u64,
    },
    Announce(PeerInfo),
    Discover {
        capability: Capability,
    },
    DiscoverResponse {
        peers: Vec<PeerInfo>,
    },
    TaskRequest(TaskRequest),
    TaskResponse(TaskResponse),
    StateSync {
        key: String,
        data: Vec<u8>,
    },
    Gossip {
        peers: Vec<PeerInfo>,
    },
    /// A task forwarded from another node that couldn't handle it locally.
    /// Receiving nodes process this like TaskRequest but never forward again,
    /// preventing routing loops. Max one hop.
    ForwardedTask(TaskRequest),
    /// Advertise MCP tool schemas available on this node.
    /// Sent periodically via gossip and on initial peer connection.
    ToolCatalog {
        peer_id: Vec<u8>,
        tools: Vec<McpToolSchema>,
    },
    /// Query the mesh for MCP tools matching a search filter.
    ToolQuery {
        query: String,
        server_filter: Option<String>,
        limit: u32,
        /// Maximum token budget for results. 0 = unlimited.
        max_tokens: u32,
        /// Schema detail level: 0=Full, 1=Summary, 2=Compact.
        detail: u8,
    },
    /// Response to a ToolQuery with matching tools and relevance scores.
    ToolQueryResponse {
        tools: Vec<ToolQueryResult>,
        /// Total tokens consumed by returned tools.
        total_tokens: u32,
        /// Whether results were truncated due to budget or limit.
        truncated: bool,
    },
}

/// A single result from a ToolQuery, returned in ToolQueryResponse.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolQueryResult {
    pub tool: McpToolSchema,
    pub score: f64,
    pub peer_id: Vec<u8>,
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

    #[test]
    fn message_roundtrip_tool_catalog() {
        let tool = McpToolSchema::new(
            "read_file",
            "Read a file",
            serde_json::json!({"type": "object"}),
            "filesystem",
        );
        let msg = Message::ToolCatalog {
            peer_id: vec![0xAA, 0xBB],
            tools: vec![tool],
        };
        let encoded = msg.encode().unwrap();
        let decoded = Message::decode(&encoded).unwrap();
        match decoded {
            Message::ToolCatalog { peer_id, tools } => {
                assert_eq!(peer_id, vec![0xAA, 0xBB]);
                assert_eq!(tools.len(), 1);
                assert_eq!(tools[0].name, "read_file");
                assert_eq!(tools[0].server_name, "filesystem");
            }
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn message_roundtrip_tool_query() {
        let msg = Message::ToolQuery {
            query: "read file".to_string(),
            server_filter: Some("filesystem".to_string()),
            limit: 5,
            max_tokens: 2000,
            detail: 1,
        };
        let encoded = msg.encode().unwrap();
        let decoded = Message::decode(&encoded).unwrap();
        match decoded {
            Message::ToolQuery {
                query,
                server_filter,
                limit,
                max_tokens,
                detail,
            } => {
                assert_eq!(query, "read file");
                assert_eq!(server_filter, Some("filesystem".to_string()));
                assert_eq!(limit, 5);
                assert_eq!(max_tokens, 2000);
                assert_eq!(detail, 1);
            }
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn message_roundtrip_tool_query_no_filter() {
        let msg = Message::ToolQuery {
            query: "anything".to_string(),
            server_filter: None,
            limit: 10,
            max_tokens: 0,
            detail: 0,
        };
        let encoded = msg.encode().unwrap();
        let decoded = Message::decode(&encoded).unwrap();
        match decoded {
            Message::ToolQuery {
                server_filter,
                limit,
                max_tokens,
                detail,
                ..
            } => {
                assert!(server_filter.is_none());
                assert_eq!(limit, 10);
                assert_eq!(max_tokens, 0);
                assert_eq!(detail, 0);
            }
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn message_roundtrip_tool_query_response() {
        let tool = McpToolSchema::new(
            "search_code",
            "Search code",
            serde_json::json!({}),
            "github",
        );
        let result = ToolQueryResult {
            tool,
            score: 0.95,
            peer_id: vec![1, 2, 3],
        };
        let msg = Message::ToolQueryResponse {
            tools: vec![result],
            total_tokens: 150,
            truncated: true,
        };
        let encoded = msg.encode().unwrap();
        let decoded = Message::decode(&encoded).unwrap();
        match decoded {
            Message::ToolQueryResponse {
                tools,
                total_tokens,
                truncated,
            } => {
                assert_eq!(tools.len(), 1);
                assert_eq!(tools[0].tool.name, "search_code");
                assert!((tools[0].score - 0.95).abs() < f64::EPSILON);
                assert_eq!(tools[0].peer_id, vec![1, 2, 3]);
                assert_eq!(total_tokens, 150);
                assert!(truncated);
            }
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn message_roundtrip_empty_tool_catalog() {
        let msg = Message::ToolCatalog {
            peer_id: vec![0xFF],
            tools: vec![],
        };
        let encoded = msg.encode().unwrap();
        let decoded = Message::decode(&encoded).unwrap();
        match decoded {
            Message::ToolCatalog { tools, .. } => {
                assert!(tools.is_empty());
            }
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn tool_query_result_roundtrip() {
        let result = ToolQueryResult {
            tool: McpToolSchema::new("test", "Test tool", serde_json::json!({}), "test"),
            score: 0.42,
            peer_id: vec![0xDE, 0xAD],
        };
        let bytes = bincode::serialize(&result).unwrap();
        let decoded: ToolQueryResult = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.tool.name, "test");
        assert!((decoded.score - 0.42).abs() < f64::EPSILON);
    }
}
