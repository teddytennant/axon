use crate::state::SharedWebState;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::time::{interval, Duration};

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
            let metrics = serde_json::json!({
                "type": "metrics",
                "data": {
                    "uptime_secs": ws.uptime_secs,
                    "tasks_total": ws.tasks_total,
                    "tasks_failed": ws.tasks_failed,
                    "messages_received": ws.messages_received,
                    "messages_sent": ws.messages_sent,
                    "throughput": ws.throughput_history.iter().copied().collect::<Vec<_>>(),
                }
            });

            if sender
                .send(Message::Text(metrics.to_string().into()))
                .await
                .is_err()
            {
                return;
            }

            // Peers
            let peers = push_state.peer_table.read().await.all_peers_owned();
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let peers_json: Vec<serde_json::Value> = peers
                .iter()
                .map(|p| {
                    let diff = now.saturating_sub(p.last_seen);
                    serde_json::json!({
                        "peer_id": hex::encode(&p.peer_id),
                        "addr": p.addr,
                        "capabilities": p.capabilities.iter().map(|c| c.tag()).collect::<Vec<_>>(),
                        "last_seen": p.last_seen,
                        "last_seen_ago": format!("{}s ago", diff),
                    })
                })
                .collect();

            let peers_msg = serde_json::json!({
                "type": "peers",
                "data": peers_json,
            });
            if sender
                .send(Message::Text(peers_msg.to_string().into()))
                .await
                .is_err()
            {
                return;
            }

            // Agents
            let agents_msg = serde_json::json!({
                "type": "agents",
                "data": ws.agent_info,
            });
            if sender
                .send(Message::Text(agents_msg.to_string().into()))
                .await
                .is_err()
            {
                return;
            }

            // Tasks
            let stats = push_state.task_queue.stats().unwrap_or_default();
            let tasks_msg = serde_json::json!({
                "type": "tasks",
                "data": {
                    "stats": {
                        "pending": stats.pending,
                        "running": stats.running,
                        "completed": stats.completed,
                        "failed": stats.failed,
                        "timed_out": stats.timed_out,
                    },
                    "recent": ws.task_log,
                }
            });
            if sender
                .send(Message::Text(tasks_msg.to_string().into()))
                .await
                .is_err()
            {
                return;
            }

            // New log lines since last push
            let current_log_count = ws.logs.len();
            if current_log_count > last_log_count {
                let new_logs: Vec<&String> = ws.logs.iter().skip(last_log_count).collect();
                for log in new_logs {
                    let log_msg = serde_json::json!({
                        "type": "log",
                        "data": log,
                    });
                    if sender
                        .send(Message::Text(log_msg.to_string().into()))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                last_log_count = current_log_count;
            }

            // Workflows
            let workflows_msg = serde_json::json!({
                "type": "workflows",
                "data": {
                    "active": ws.active_workflows,
                    "completed": ws.completed_workflows.iter().collect::<Vec<_>>(),
                }
            });
            if sender
                .send(Message::Text(workflows_msg.to_string().into()))
                .await
                .is_err()
            {
                return;
            }

            // Blackboard
            let bb_msg = serde_json::json!({
                "type": "blackboard",
                "data": ws.blackboard_entries,
            });
            if sender
                .send(Message::Text(bb_msg.to_string().into()))
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
            let trust_entries: Vec<serde_json::Value> = ranked
                .into_iter()
                .map(|(peer_id, score)| {
                    serde_json::json!({
                        "peer_id": hex::encode(&peer_id),
                        "overall": score.overall,
                        "reliability": score.reliability,
                        "confidence": score.confidence,
                        "observation_count": score.observation_count,
                    })
                })
                .collect();
            let trust_msg = serde_json::json!({
                "type": "trust",
                "data": trust_entries,
            });
            if sender
                .send(Message::Text(trust_msg.to_string().into()))
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
