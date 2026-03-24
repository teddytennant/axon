pub mod crdt;
pub mod discovery;
pub mod gossip;
pub mod identity;
pub mod mcp;
pub mod mdns;
pub mod negotiate;
pub mod protocol;
pub mod router;
pub mod runtime;
pub mod taskqueue;
pub mod transport;
pub mod trust;

pub use crdt::{GCounter, LWWRegister, ORSet};
pub use discovery::PeerTable;
pub use gossip::{broadcast_tool_catalog, send_tool_catalog, GossipConfig, LocalToolCatalog};
pub use identity::Identity;
pub use mcp::{
    BudgetSearchResult, CompactToolSchema, McpBridge, McpBridgeAgent, McpClient, McpClientError,
    McpServerConfig, McpStdioServer, McpToolSchema, MeshContext, SchemaDetail, SummaryToolSchema,
    ToolFilter, ToolRegistry, ToolSearchResult,
};
pub use mdns::{DiscoveryEvent, MdnsDiscovery};
pub use negotiate::{
    ActiveNegotiation, BidScoring, BiddingStrategy, EagerBidder, LoadAwareBidder, NegotiationState,
    Negotiator, ReceivedBid,
};
pub use protocol::{
    Capability, Message, PeerInfo, TaskRequest, TaskResponse, TaskStatus, ToolQueryResult,
    TrustGossipEntry,
};
pub use router::{Router, Strategy};
pub use runtime::{Agent, AgentError, Runtime};
pub use taskqueue::{QueueError, QueueStats, TaskQueue, TaskQueueConfig, TaskRecord, TaskState};
pub use transport::{extract_peer_cert_pubkey, Transport};
pub use trust::{
    SharedTrustObservation, TaskOutcome, TrustGossipProcessor, TrustObservation, TrustRecord,
    TrustScore, TrustScorer, TrustStore, TrustWeightedScoring,
};
