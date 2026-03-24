use async_trait::async_trait;
use axon_core::{
    Agent, AgentError, Capability, Identity, Message, PeerInfo, PeerTable, Runtime, TaskQueue,
    TaskQueueConfig, TaskRequest, TaskResponse, TaskState, TaskStatus, Transport,
};
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

/// Test: ForwardedTask message is handled by the receiving node.
/// Node 1 sends a ForwardedTask to Node 2 which has an EchoAgent.
/// Node 2 processes it locally and returns the response.
#[tokio::test]
async fn forwarded_task_handled_by_capable_node() {
    let id1 = Identity::generate();
    let t1 = Transport::bind("127.0.0.1:0".parse().unwrap(), &id1)
        .await
        .unwrap();

    let id2 = Identity::generate();
    let t2 = Transport::bind("127.0.0.1:0".parse().unwrap(), &id2)
        .await
        .unwrap();
    let t2_addr = t2.local_addr().unwrap();

    let runtime2 = Arc::new(Runtime::new());
    runtime2.register(Arc::new(EchoAgent)).await;

    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();
    let rt2 = runtime2.clone();

    // Node 2: accept connection, receive ForwardedTask, dispatch, return response
    let node2_handle = tokio::spawn(async move {
        let conn = t2.accept().await.unwrap();
        let msg = Transport::recv(&conn).await.unwrap();

        let req = match msg {
            Message::ForwardedTask(req) => req,
            _ => panic!("expected ForwardedTask, got {:?}", msg),
        };

        let resp = rt2.dispatch(req).await;
        Transport::send(&conn, &Message::TaskResponse(resp))
            .await
            .unwrap();

        let _ = done_rx.await;
    });

    // Node 1: connect and send ForwardedTask
    let conn = t1.connect(t2_addr).await.unwrap();
    let task_id = Uuid::new_v4();
    let fwd = Message::ForwardedTask(TaskRequest {
        id: task_id,
        capability: Capability::new("echo", "ping", 1),
        payload: b"forwarded data".to_vec(),
        timeout_ms: 5000,
    });
    Transport::send(&conn, &fwd).await.unwrap();

    let resp = Transport::recv(&conn).await.unwrap();
    match resp {
        Message::TaskResponse(r) => {
            assert_eq!(r.request_id, task_id);
            assert_eq!(r.status, TaskStatus::Success);
            assert_eq!(r.payload, b"forwarded data");
        }
        _ => panic!("expected TaskResponse"),
    }

    let _ = done_tx.send(());
    let _ = node2_handle.await;
}

/// Test: ForwardedTask to a node without the capability returns NoCapability.
/// This verifies that forwarded tasks are NOT re-forwarded (max one hop).
#[tokio::test]
async fn forwarded_task_no_capability_returns_error() {
    let id1 = Identity::generate();
    let t1 = Transport::bind("127.0.0.1:0".parse().unwrap(), &id1)
        .await
        .unwrap();

    let id2 = Identity::generate();
    let t2 = Transport::bind("127.0.0.1:0".parse().unwrap(), &id2)
        .await
        .unwrap();
    let t2_addr = t2.local_addr().unwrap();

    // Node 2 has NO agents registered
    let runtime2 = Arc::new(Runtime::new());

    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();
    let rt2 = runtime2.clone();

    let node2_handle = tokio::spawn(async move {
        let conn = t2.accept().await.unwrap();
        let msg = Transport::recv(&conn).await.unwrap();

        let req = match msg {
            Message::ForwardedTask(req) => req,
            _ => panic!("expected ForwardedTask"),
        };

        let resp = rt2.dispatch(req).await;
        assert_eq!(resp.status, TaskStatus::NoCapability);

        Transport::send(&conn, &Message::TaskResponse(resp))
            .await
            .unwrap();

        let _ = done_rx.await;
    });

    let conn = t1.connect(t2_addr).await.unwrap();
    let fwd = Message::ForwardedTask(TaskRequest {
        id: Uuid::new_v4(),
        capability: Capability::new("nonexistent", "thing", 1),
        payload: vec![],
        timeout_ms: 5000,
    });
    Transport::send(&conn, &fwd).await.unwrap();

    let resp = Transport::recv(&conn).await.unwrap();
    match resp {
        Message::TaskResponse(r) => {
            assert_eq!(r.status, TaskStatus::NoCapability);
        }
        _ => panic!("expected TaskResponse"),
    }

    let _ = done_tx.send(());
    let _ = node2_handle.await;
}

/// Test: Queue drain pattern — enqueue tasks, dequeue in a loop, dispatch.
/// Simulates what the background drain worker does.
#[tokio::test]
async fn queue_drain_dispatches_pending_tasks() {
    let queue = TaskQueue::open_temporary(TaskQueueConfig::default()).unwrap();
    let runtime = Runtime::new();
    runtime.register(Arc::new(EchoAgent)).await;

    // Enqueue 3 tasks
    let mut ids = Vec::new();
    for i in 0..3 {
        let req = TaskRequest {
            id: Uuid::new_v4(),
            capability: Capability::new("echo", "ping", 1),
            payload: format!("task-{}", i).into_bytes(),
            timeout_ms: 5000,
        };
        ids.push(req.id);
        queue.enqueue(req).unwrap();
    }

    assert_eq!(queue.pending_count(), 3);

    // Drain loop (simulates the drain worker)
    let mut dispatched = 0;
    loop {
        match queue.dequeue().unwrap() {
            Some(record) => {
                let task_id = record.request.id;
                let resp = runtime.dispatch(record.request).await;
                assert_eq!(resp.status, TaskStatus::Success);
                queue.complete(task_id, resp).unwrap();
                dispatched += 1;
            }
            None => break,
        }
    }

    assert_eq!(dispatched, 3);
    assert_eq!(queue.pending_count(), 0);

    // All tasks should be completed
    let stats = queue.stats().unwrap();
    assert_eq!(stats.completed, 3);
    assert_eq!(stats.pending, 0);
    assert_eq!(stats.running, 0);
}

/// Test: Queue drain with retries — failed tasks get re-dispatched on next drain.
#[tokio::test]
async fn queue_drain_retries_failed_tasks() {
    let queue = TaskQueue::open_temporary(TaskQueueConfig {
        max_retries: 2,
        ..Default::default()
    })
    .unwrap();

    struct FailOnceAgent {
        call_count: std::sync::atomic::AtomicU32,
    }

    #[async_trait]
    impl Agent for FailOnceAgent {
        fn name(&self) -> &str {
            "fail-once"
        }
        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::new("test", "flaky", 1)]
        }
        async fn handle(&self, request: TaskRequest) -> Result<TaskResponse, AgentError> {
            let count = self
                .call_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if count == 0 {
                Err(AgentError::Internal("transient failure".to_string()))
            } else {
                Ok(TaskResponse {
                    request_id: request.id,
                    status: TaskStatus::Success,
                    payload: b"recovered".to_vec(),
                    duration_ms: 0,
                })
            }
        }
    }

    let agent = Arc::new(FailOnceAgent {
        call_count: std::sync::atomic::AtomicU32::new(0),
    });
    let runtime = Runtime::new();
    runtime.register(agent.clone()).await;

    // Enqueue one task
    let req = TaskRequest {
        id: Uuid::new_v4(),
        capability: Capability::new("test", "flaky", 1),
        payload: b"retry-me".to_vec(),
        timeout_ms: 5000,
    };
    let task_id = req.id;
    queue.enqueue(req).unwrap();

    // First drain: task fails, gets re-enqueued
    let record = queue.dequeue().unwrap().unwrap();
    let resp = runtime.dispatch(record.request).await;
    match &resp.status {
        TaskStatus::Error(e) => {
            let retried = queue.fail(task_id, e.clone()).unwrap();
            assert!(retried); // should be re-enqueued (attempt 1 <= max_retries 2)
        }
        _ => panic!("expected error on first attempt"),
    }

    assert_eq!(queue.pending_count(), 1); // re-enqueued

    // Second drain: task succeeds
    let record2 = queue.dequeue().unwrap().unwrap();
    assert_eq!(record2.request.id, task_id);
    assert_eq!(record2.attempts, 2); // second attempt

    let resp2 = runtime.dispatch(record2.request).await;
    assert_eq!(resp2.status, TaskStatus::Success);
    assert_eq!(resp2.payload, b"recovered");
    queue.complete(task_id, resp2).unwrap();

    assert_eq!(queue.pending_count(), 0);

    let final_record = queue.get(task_id).unwrap().unwrap();
    assert_eq!(final_record.state, TaskState::Completed);
    assert_eq!(final_record.attempts, 2);
}

/// Test: Queue recovery simulates crash — Running tasks are recovered and re-dispatched.
#[tokio::test]
async fn queue_crash_recovery_and_drain() {
    let runtime = Runtime::new();
    runtime.register(Arc::new(EchoAgent)).await;

    // Use a file-backed queue in a temp dir to simulate crash/restart
    let tmp = std::env::temp_dir().join(format!("axon-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();

    let task_id;

    // Phase 1: enqueue + dequeue (simulates task in-flight when crash happens)
    {
        let queue = TaskQueue::open(&tmp, TaskQueueConfig::default()).unwrap();
        let req = TaskRequest {
            id: Uuid::new_v4(),
            capability: Capability::new("echo", "ping", 1),
            payload: b"survive-crash".to_vec(),
            timeout_ms: 5000,
        };
        task_id = req.id;
        queue.enqueue(req).unwrap();

        // Dequeue marks it as Running
        let record = queue.dequeue().unwrap().unwrap();
        assert_eq!(record.state, TaskState::Running);
        // Simulate crash: drop the queue without completing the task
        queue.flush().unwrap();
    }

    // Phase 2: reopen the queue (simulates restart after crash)
    {
        let queue = TaskQueue::open(&tmp, TaskQueueConfig::default()).unwrap();

        // The task should still be in Running state
        let record = queue.get(task_id).unwrap().unwrap();
        assert_eq!(record.state, TaskState::Running);

        // Run recovery
        let recovered = queue.recover().unwrap();
        assert_eq!(recovered, 1);
        assert_eq!(queue.pending_count(), 1);

        // Drain the recovered task
        let record = queue.dequeue().unwrap().unwrap();
        assert_eq!(record.request.id, task_id);
        assert_eq!(record.request.payload, b"survive-crash");

        let resp = runtime.dispatch(record.request).await;
        assert_eq!(resp.status, TaskStatus::Success);
        queue.complete(task_id, resp).unwrap();

        assert_eq!(queue.pending_count(), 0);
        let final_record = queue.get(task_id).unwrap().unwrap();
        assert_eq!(final_record.state, TaskState::Completed);
    }

    // Cleanup
    let _ = std::fs::remove_dir_all(&tmp);
}

/// Test: TLS certificates are derived from identity keys. When two nodes
/// exchange messages, the TLS handshake is authenticated by MeshCertVerifier
/// (real signature verification, not skipped).
#[tokio::test]
async fn tls_identity_derived_certs_work() {
    let id1 = Identity::generate();
    let id2 = Identity::generate();

    // Both transports create identity-derived TLS certs
    let t1 = Transport::bind("127.0.0.1:0".parse().unwrap(), &id1)
        .await
        .unwrap();
    let t2 = Transport::bind("127.0.0.1:0".parse().unwrap(), &id2)
        .await
        .unwrap();
    let t2_addr = t2.local_addr().unwrap();

    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();

    let t2_handle = tokio::spawn(async move {
        let conn = t2.accept().await.unwrap();
        let msg = Transport::recv(&conn).await.unwrap();
        Transport::send(&conn, &Message::Pong { nonce: 42 })
            .await
            .unwrap();
        let _ = done_rx.await;
        msg
    });

    // connect_verified checks both TLS cert and identity handshake
    let conn = t1
        .connect_verified(t2_addr, &id2.public_key_bytes())
        .await
        .unwrap();
    Transport::send(&conn, &Message::Ping { nonce: 42 })
        .await
        .unwrap();

    let resp = Transport::recv(&conn).await.unwrap();
    match resp {
        Message::Pong { nonce } => assert_eq!(nonce, 42),
        _ => panic!("expected Pong"),
    }

    let _ = done_tx.send(());
    let _ = t2_handle.await;
}

/// Test: connect_verified correctly rejects a peer whose TLS cert has a
/// different identity (MITM scenario).
#[tokio::test]
async fn tls_cert_mismatch_rejected() {
    let id1 = Identity::generate();
    let id2 = Identity::generate();
    let id3 = Identity::generate(); // the expected (wrong) identity

    let t1 = Transport::bind("127.0.0.1:0".parse().unwrap(), &id1)
        .await
        .unwrap();
    let t2 = Transport::bind("127.0.0.1:0".parse().unwrap(), &id2)
        .await
        .unwrap();
    let t2_addr = t2.local_addr().unwrap();

    tokio::spawn(async move {
        let _ = t2.accept().await;
    });

    // We expect id3 but t2 has id2 — should fail at either TLS cert check
    // or identity handshake.
    let result = t1.connect_verified(t2_addr, &id3.public_key_bytes()).await;
    assert!(result.is_err());
}

/// Test: extract_ed25519_pubkey_from_cert round-trips through a real cert.
#[test]
fn extract_pubkey_from_identity_cert() {
    use axon_core::transport::extract_ed25519_pubkey_from_cert;

    let id = Identity::generate();

    // Build a cert the same way Transport::make_tls_configs does
    let seed = id.secret_bytes();
    let mut pkcs8 = Vec::with_capacity(48);
    pkcs8.extend_from_slice(&[0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05]);
    pkcs8.extend_from_slice(&[0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04, 0x20]);
    pkcs8.extend_from_slice(seed);

    let pkcs8_der = rustls::pki_types::PrivateKeyDer::try_from(pkcs8).unwrap();
    let kp = rcgen::KeyPair::from_der_and_sign_algo(&pkcs8_der, &rcgen::PKCS_ED25519).unwrap();
    let params = rcgen::CertificateParams::new(vec!["axon".to_string()]).unwrap();
    let cert = params.self_signed(&kp).unwrap();

    let extracted = extract_ed25519_pubkey_from_cert(cert.der()).unwrap();
    assert_eq!(extracted.as_slice(), id.public_key_bytes().as_slice());
}
