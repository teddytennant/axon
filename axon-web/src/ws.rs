use crate::api::mesh::PeerResponse;
use crate::api::trust::TrustEntry;
use crate::state::{
    AgentInfo, BlackboardEntry, SharedWebState, TaskLogEntry, WorkflowSnapshot,
};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use std::sync::Arc;
use tokio::time::{interval, Duration};

/// A single push frame sent over the `/api/ws/live` websocket.
///
/// Serialised as `{ "type": "<tag>", "data": <payload> }` so the TypeScript
/// client can discriminate on `type` and statically narrow `data`.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "lowercase")]
pub enum WsEvent {
    Metrics(WsMetrics),
    Peers(Vec<PeerResponse>),
    Agents(Vec<AgentInfo>),
    Tasks(WsTasks),
    Trust(Vec<TrustEntry>),
    Log(String),
    Workflows(WsWorkflows),
    Blackboard(Vec<BlackboardEntry>),
}

#[derive(Debug, Clone, Serialize)]
pub struct WsMetrics {
    pub uptime_secs: u64,
    pub tasks_total: u64,
    pub tasks_failed: u64,
    pub messages_received: u64,
    pub messages_sent: u64,
    pub throughput: Vec<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WsTaskStats {
    pub pending: usize,
    pub running: usize,
    pub completed: usize,
    pub failed: usize,
    pub timed_out: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct WsTasks {
    pub stats: WsTaskStats,
    pub recent: Vec<TaskLogEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WsWorkflows {
    pub active: Vec<WorkflowSnapshot>,
    pub completed: Vec<WorkflowSnapshot>,
}

/// Encode an event as a websocket text frame. Infallible in practice since
/// all variants are plain `Serialize` structs over owned data.
fn encode(event: &WsEvent) -> Message {
    let text = serde_json::to_string(event).unwrap_or_else(|_| String::from("{}"));
    Message::Text(text.into())
}

pub async fn ws_live(
    ws: WebSocketUpgrade,
    State(state): State<Arc<SharedWebState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(socket: WebSocket, state: Arc<SharedWebState>) {
    let (mut sender, mut receiver) = socket.split();

    // Spawn a task that pushes data every second
    let push_state = state.clone();
    let mut push_task = tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(1));
        let mut last_log_count = 0usize;

        loop {
            tick.tick().await;

            // Gather snapshot data
            let ws = push_state.web_state.read().await;

            // Status/metrics
            let metrics = WsEvent::Metrics(WsMetrics {
                uptime_secs: ws.uptime_secs,
                tasks_total: ws.tasks_total,
                tasks_failed: ws.tasks_failed,
                messages_received: ws.messages_received,
                messages_sent: ws.messages_sent,
                throughput: ws.throughput_history.iter().copied().collect(),
            });

            if sender.send(encode(&metrics)).await.is_err() {
                return;
            }

            // Peers
            let peers = push_state.peer_table.read().await.all_peers_owned();
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let peers_typed: Vec<PeerResponse> = peers
                .iter()
                .map(|p| {
                    let diff = now.saturating_sub(p.last_seen);
                    PeerResponse {
                        peer_id: hex::encode(&p.peer_id),
                        addr: p.addr.clone(),
                        capabilities: p.capabilities.iter().map(|c| c.tag()).collect(),
                        last_seen: p.last_seen,
                        last_seen_ago: format!("{}s ago", diff),
                    }
                })
                .collect();

            if sender
                .send(encode(&WsEvent::Peers(peers_typed)))
                .await
                .is_err()
            {
                return;
            }

            // Agents
            if sender
                .send(encode(&WsEvent::Agents(ws.agent_info.clone())))
                .await
                .is_err()
            {
                return;
            }

            // Tasks
            let stats = push_state.task_queue.stats().unwrap_or_default();
            let tasks = WsEvent::Tasks(WsTasks {
                stats: WsTaskStats {
                    pending: stats.pending,
                    running: stats.running,
                    completed: stats.completed,
                    failed: stats.failed,
                    timed_out: stats.timed_out,
                },
                recent: ws.task_log.clone(),
            });
            if sender.send(encode(&tasks)).await.is_err() {
                return;
            }

            // New log lines since last push
            let current_log_count = ws.logs.len();
            if current_log_count > last_log_count {
                let new_logs: Vec<String> = ws
                    .logs
                    .iter()
                    .skip(last_log_count)
                    .cloned()
                    .collect();
                for log in new_logs {
                    if sender.send(encode(&WsEvent::Log(log))).await.is_err() {
                        return;
                    }
                }
                last_log_count = current_log_count;
            }

            // Workflows
            let workflows = WsEvent::Workflows(WsWorkflows {
                active: ws.active_workflows.clone(),
                completed: ws.completed_workflows.iter().cloned().collect(),
            });
            if sender.send(encode(&workflows)).await.is_err() {
                return;
            }

            // Blackboard
            if sender
                .send(encode(&WsEvent::Blackboard(ws.blackboard_entries.clone())))
                .await
                .is_err()
            {
                return;
            }

            drop(ws);

            // Trust scores (less frequent — every push, but the data is small)
            let ts = push_state.trust_store.lock().await;
            let ranked = ts.ranked_peers();
            drop(ts);
            let trust_entries: Vec<TrustEntry> = ranked
                .into_iter()
                .map(|(peer_id, score)| TrustEntry {
                    peer_id: hex::encode(&peer_id),
                    reliability: score.reliability,
                    accuracy: score.accuracy,
                    availability: score.availability,
                    quality: score.quality,
                    overall: score.overall,
                    confidence: score.confidence,
                    observation_count: score.observation_count,
                })
                .collect();
            if sender
                .send(encode(&WsEvent::Trust(trust_entries)))
                .await
                .is_err()
            {
                return;
            }
        }
    });

    // Listen for incoming messages (pings, close frames)
    let recv_task = tokio::spawn(async move {
        while let Some(msg) = receiver.next().await {
            if msg.is_err() {
                break;
            }
        }
    });

    // Wait for either task to finish
    tokio::select! {
        _ = &mut push_task => {},
        _ = recv_task => {
            push_task.abort();
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn ws_event_metrics_round_trip() {
        let event = WsEvent::Metrics(WsMetrics {
            uptime_secs: 42,
            tasks_total: 10,
            tasks_failed: 1,
            messages_received: 5,
            messages_sent: 6,
            throughput: vec![1, 2, 3],
        });
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "metrics");
        assert_eq!(json["data"]["uptime_secs"], 42);
        assert_eq!(json["data"]["throughput"], Value::Array(vec![1.into(), 2.into(), 3.into()]));
    }

    #[test]
    fn ws_event_log_tagged_correctly() {
        let event = WsEvent::Log("hello".into());
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "log");
        assert_eq!(json["data"], "hello");
    }

    #[test]
    fn ws_event_tasks_shape() {
        let event = WsEvent::Tasks(WsTasks {
            stats: WsTaskStats {
                pending: 1,
                running: 2,
                completed: 3,
                failed: 4,
                timed_out: 5,
            },
            recent: vec![],
        });
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "tasks");
        assert_eq!(json["data"]["stats"]["pending"], 1);
        assert_eq!(json["data"]["stats"]["timed_out"], 5);
        assert!(json["data"]["recent"].is_array());
    }

    #[test]
    fn ws_event_peers_empty() {
        let event = WsEvent::Peers(vec![]);
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "peers");
        assert!(json["data"].is_array());
    }
}
