use axon_core::{
    Agent, AgentError, Capability, Identity, Message, PeerInfo, PeerTable, Runtime, TaskRequest,
    TaskResponse, TaskStatus, Transport,
};
use async_trait::async_trait;
use std::sync::Arc;
use uuid::Uuid;

struct EchoAgent;

#[async_trait]
impl Agent for EchoAgent {
    fn name(&self) -> &str {
        "echo"
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("echo", "ping", 1)]
    }

    async fn handle(&self, request: TaskRequest) -> Result<TaskResponse, AgentError> {
        Ok(TaskResponse {
            request_id: request.id,
            status: TaskStatus::Success,
            payload: request.payload,
            duration_ms: 0,
        })
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Full integration test: two nodes connect, announce, and exchange a task.
#[tokio::test]
async fn two_node_task_exchange() {
    // === Node 1: the requester ===
    let id1 = Identity::generate();
    let t1 = Transport::bind("127.0.0.1:0".parse().unwrap(), &id1)
        .await
        .unwrap();
    let t1_addr = t1.local_addr().unwrap();

    // === Node 2: the agent host ===
    let id2 = Identity::generate();
    let t2 = Transport::bind("127.0.0.1:0".parse().unwrap(), &id2)
        .await
        .unwrap();
    let t2_addr = t2.local_addr().unwrap();

    let runtime2 = Arc::new(Runtime::new());
    runtime2.register(Arc::new(EchoAgent)).await;

    // Node 2 listens for connections and handles messages
    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();

    let rt2 = runtime2.clone();
    let node2_handle = tokio::spawn(async move {
        let conn = t2.accept().await.unwrap();

        // Receive the announce
        let msg = Transport::recv(&conn).await.unwrap();
        // First message should be an Announce
        match &msg {
            Message::Announce(info) => {
                assert!(!info.peer_id.is_empty());
            }
            _ => panic!("expected Announce, got {:?}", msg),
        }

        // Receive task request
        let msg2 = Transport::recv(&conn).await.unwrap();
        let task_req = match msg2 {
            Message::TaskRequest(req) => req,
            _ => panic!("expected TaskRequest"),
        };

        // Dispatch to runtime
        let response = rt2.dispatch(task_req).await;

        // Send response back
        Transport::send(&conn, &Message::TaskResponse(response))
            .await
            .unwrap();

        // Wait for done signal
        let _ = done_rx.await;
    });

    // Node 1 connects to Node 2
    let conn = t1.connect(t2_addr).await.unwrap();

    // Send announce
    let announce = Message::Announce(PeerInfo {
        peer_id: id1.public_key_bytes(),
        addr: t1_addr.to_string(),
        capabilities: vec![],
        last_seen: now_secs(),
    });
    Transport::send(&conn, &announce).await.unwrap();

    // Send task request
    let task_id = Uuid::new_v4();
    let req = Message::TaskRequest(TaskRequest {
        id: task_id,
        capability: Capability::new("echo", "ping", 1),
        payload: b"hello mesh".to_vec(),
        timeout_ms: 5000,
    });
    Transport::send(&conn, &req).await.unwrap();

    // Receive response
    let resp = Transport::recv(&conn).await.unwrap();
    match resp {
        Message::TaskResponse(r) => {
            assert_eq!(r.request_id, task_id);
            assert_eq!(r.status, TaskStatus::Success);
            assert_eq!(r.payload, b"hello mesh");
        }
        _ => panic!("expected TaskResponse"),
    }

    // Cleanup
    let _ = done_tx.send(());
    let _ = node2_handle.await;
}

/// Test: peer table correctly tracks discovered peers.
#[test]
fn peer_table_full_workflow() {
    let local = PeerInfo {
        peer_id: vec![1, 1, 1, 1],
        addr: "127.0.0.1:4242".to_string(),
        capabilities: vec![Capability::new("echo", "ping", 1)],
        last_seen: now_secs(),
    };

    let mut table = PeerTable::new(local);

    // Discover peers via gossip
    let gossip_peers = vec![
        PeerInfo {
            peer_id: vec![2, 2, 2, 2],
            addr: "10.0.0.1:4242".to_string(),
            capabilities: vec![
                Capability::new("llm", "chat", 1),
                Capability::new("llm", "embed", 1),
            ],
            last_seen: now_secs(),
        },
        PeerInfo {
            peer_id: vec![3, 3, 3, 3],
            addr: "10.0.0.2:4242".to_string(),
            capabilities: vec![Capability::new("code", "review", 1)],
            last_seen: now_secs(),
        },
    ];

    let new_count = table.merge_gossip(gossip_peers);
    assert_eq!(new_count, 2);
    assert_eq!(table.len(), 2);

    // Find peers by capability
    let llm_peers = table.find_by_capability(&Capability::new("llm", "chat", 1));
    assert_eq!(llm_peers.len(), 1);
    assert_eq!(llm_peers[0].addr, "10.0.0.1:4242");

    // Evict stale peer
    let old_peer = PeerInfo {
        peer_id: vec![4, 4, 4, 4],
        addr: "10.0.0.3:4242".to_string(),
        capabilities: vec![],
        last_seen: 1000, // ancient
    };
    table.upsert(old_peer);
    assert_eq!(table.len(), 3);

    let evicted = table.evict_stale();
    assert_eq!(evicted.len(), 1);
    assert_eq!(table.len(), 2);
}

/// Test: runtime dispatch with multiple agents.
#[tokio::test]
async fn runtime_multi_agent_dispatch() {
    struct UppercaseAgent;

    #[async_trait]
    impl Agent for UppercaseAgent {
        fn name(&self) -> &str {
            "uppercase"
        }
        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::new("text", "uppercase", 1)]
        }
        async fn handle(&self, req: TaskRequest) -> Result<TaskResponse, AgentError> {
            let text = String::from_utf8(req.payload).unwrap();
            Ok(TaskResponse {
                request_id: req.id,
                status: TaskStatus::Success,
                payload: text.to_uppercase().into_bytes(),
                duration_ms: 0,
            })
        }
    }

    let rt = Runtime::new();
    rt.register(Arc::new(EchoAgent)).await;
    rt.register(Arc::new(UppercaseAgent)).await;

    assert_eq!(rt.agent_count().await, 2);
    assert_eq!(rt.all_capabilities().await.len(), 2);

    // Dispatch to echo
    let req1 = TaskRequest {
        id: Uuid::new_v4(),
        capability: Capability::new("echo", "ping", 1),
        payload: b"test".to_vec(),
        timeout_ms: 1000,
    };
    let resp1 = rt.dispatch(req1).await;
    assert_eq!(resp1.status, TaskStatus::Success);
    assert_eq!(resp1.payload, b"test");

    // Dispatch to uppercase
    let req2 = TaskRequest {
        id: Uuid::new_v4(),
        capability: Capability::new("text", "uppercase", 1),
        payload: b"hello world".to_vec(),
        timeout_ms: 1000,
    };
    let resp2 = rt.dispatch(req2).await;
    assert_eq!(resp2.status, TaskStatus::Success);
    assert_eq!(resp2.payload, b"HELLO WORLD");

    // Dispatch to non-existent capability
    let req3 = TaskRequest {
        id: Uuid::new_v4(),
        capability: Capability::new("nonexistent", "thing", 1),
        payload: vec![],
        timeout_ms: 1000,
    };
    let resp3 = rt.dispatch(req3).await;
    assert_eq!(resp3.status, TaskStatus::NoCapability);
}

/// Test: CRDT convergence across simulated nodes.
#[test]
fn crdt_convergence_simulation() {
    use axon_core::crdt::{GCounter, ORSet};

    // Simulate 3 nodes each maintaining their own CRDT state
    let mut node1_counter = GCounter::new();
    let mut node2_counter = GCounter::new();
    let mut node3_counter = GCounter::new();

    // Each node increments independently
    node1_counter.increment("node1");
    node1_counter.increment("node1");
    node2_counter.increment("node2");
    node2_counter.increment("node2");
    node2_counter.increment("node2");
    node3_counter.increment("node3");

    // Simulate gossip: node1 receives from node2 and node3
    node1_counter.merge(&node2_counter);
    node1_counter.merge(&node3_counter);

    // node2 receives from node1 (which already has node3's data)
    node2_counter.merge(&node1_counter);

    // node3 receives from node2 (which already has everyone's data)
    node3_counter.merge(&node2_counter);

    // All should converge to the same value
    assert_eq!(node1_counter.value(), 6);
    assert_eq!(node2_counter.value(), 6);
    assert_eq!(node3_counter.value(), 6);

    // ORSet convergence
    let mut set1: ORSet<String> = ORSet::new();
    let mut set2: ORSet<String> = ORSet::new();

    set1.add("n1", "apple".to_string());
    set1.add("n1", "banana".to_string());
    set2.add("n2", "cherry".to_string());
    set2.add("n2", "banana".to_string());

    // Node 2 removes banana (its own copy)
    set2.remove(&"banana".to_string());

    // Merge: node1's banana should survive (concurrent add wins)
    set1.merge(&set2);
    assert!(set1.contains(&"apple".to_string()));
    assert!(set1.contains(&"banana".to_string())); // node1's add survives
    assert!(set1.contains(&"cherry".to_string()));
}
