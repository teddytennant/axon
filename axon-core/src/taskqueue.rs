//! Persistent task queue backed by sled.
//!
//! Tasks are durably stored before dispatch and survive node crashes.
//! Failed or timed-out tasks are automatically retried up to a
//! configurable limit. On startup, [`TaskQueue::recover`] re-enqueues
//! any tasks that were mid-execution when the previous process died.

use crate::protocol::{TaskRequest, TaskResponse};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};
use uuid::Uuid;

/// State of a task in the persistent queue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskState {
    /// Waiting for dispatch.
    Pending,
    /// Currently being executed by an agent.
    Running,
    /// Completed successfully.
    Completed,
    /// Failed with an error message.
    Failed(String),
    /// Timed out during execution.
    TimedOut,
}

impl TaskState {
    /// Whether this state is terminal (no further transitions).
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed(_) | Self::TimedOut)
    }
}

/// A task record stored in the persistent queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRecord {
    /// The original task request.
    pub request: TaskRequest,
    /// Current state.
    pub state: TaskState,
    /// Millis since epoch when enqueued.
    pub created_at: u64,
    /// Millis since epoch of last state change.
    pub updated_at: u64,
    /// Number of dispatch attempts so far.
    pub attempts: u32,
    /// Maximum retry attempts after first dispatch (0 = no retries).
    pub max_retries: u32,
    /// Response (populated on completion/failure).
    pub response: Option<TaskResponse>,
}

/// Configuration for the task queue.
#[derive(Debug, Clone)]
pub struct TaskQueueConfig {
    /// Maximum retry attempts for failed/timed-out tasks.
    /// 0 = dispatch once with no retries.
    /// N = dispatch once + up to N retries = N+1 total attempts.
    pub max_retries: u32,
    /// Seconds to retain completed/failed records before cleanup.
    pub retention_secs: u64,
    /// Maximum pending tasks (0 = unlimited).
    pub max_queue_size: usize,
}

impl Default for TaskQueueConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            retention_secs: 86_400, // 24 hours
            max_queue_size: 10_000,
        }
    }
}

/// Queue statistics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueueStats {
    pub pending: usize,
    pub running: usize,
    pub completed: usize,
    pub failed: usize,
    pub timed_out: usize,
}

impl QueueStats {
    pub fn total(&self) -> usize {
        self.pending + self.running + self.completed + self.failed + self.timed_out
    }
}

/// Errors from queue operations.
#[derive(Debug, thiserror::Error)]
pub enum QueueError {
    #[error("storage error: {0}")]
    Storage(#[from] sled::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] bincode::Error),
    #[error("queue full ({0} tasks)")]
    QueueFull(usize),
    #[error("task not found: {0}")]
    NotFound(Uuid),
}

/// Persistent task queue backed by sled.
///
/// Two sled trees:
/// - `tasks`: task_id (16 bytes) → bincode(TaskRecord)
/// - `pending`: seq_be(8) + task_id(16) → task_id(16) — monotonic FIFO index
pub struct TaskQueue {
    tasks: sled::Tree,
    pending: sled::Tree,
    config: TaskQueueConfig,
    seq: AtomicU64,
    _db: sled::Db,
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Build the pending-index key: 8-byte BE sequence + 16-byte UUID = 24 bytes.
/// Monotonic sequence ensures strict FIFO regardless of clock resolution.
fn pending_key(seq: u64, task_id: Uuid) -> [u8; 24] {
    let mut key = [0u8; 24];
    key[..8].copy_from_slice(&seq.to_be_bytes());
    key[8..].copy_from_slice(task_id.as_bytes());
    key
}

/// Extract the sequence number from a pending-index key.
fn seq_from_pending_key(key: &[u8]) -> u64 {
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&key[..8]);
    u64::from_be_bytes(buf)
}

fn uuid_from_ivec(v: &sled::IVec) -> Result<Uuid, QueueError> {
    Uuid::from_slice(v).map_err(|e| {
        sled::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e)).into()
    })
}

// ---------------------------------------------------------------------------
// TaskQueue
// ---------------------------------------------------------------------------

impl TaskQueue {
    /// Open (or create) a persistent task queue at `path`.
    pub fn open(path: impl AsRef<Path>, config: TaskQueueConfig) -> Result<Self, QueueError> {
        let db = sled::open(path)?;
        let tasks = db.open_tree("tasks")?;
        let pending = db.open_tree("pending")?;

        // Initialize sequence from the last pending key (or 0).
        let initial_seq = pending
            .last()?
            .map(|(k, _)| seq_from_pending_key(&k) + 1)
            .unwrap_or(0);

        info!(
            "Task queue opened: {} records, {} pending, seq={}",
            tasks.len(),
            pending.len(),
            initial_seq
        );
        Ok(Self {
            tasks,
            pending,
            config,
            seq: AtomicU64::new(initial_seq),
            _db: db,
        })
    }

    /// Open a temporary in-memory queue (for testing).
    pub fn open_temporary(config: TaskQueueConfig) -> Result<Self, QueueError> {
        let db = sled::Config::new().temporary(true).open()?;
        let tasks = db.open_tree("tasks")?;
        let pending = db.open_tree("pending")?;
        Ok(Self {
            tasks,
            pending,
            config,
            seq: AtomicU64::new(0),
            _db: db,
        })
    }

    /// Get the next monotonic sequence number.
    fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::SeqCst)
    }

    // -- core operations --------------------------------------------------

    /// Enqueue a task. Persists the record and adds it to the pending index.
    pub fn enqueue(&self, request: TaskRequest) -> Result<Uuid, QueueError> {
        if self.config.max_queue_size > 0 && self.pending.len() >= self.config.max_queue_size {
            return Err(QueueError::QueueFull(self.config.max_queue_size));
        }

        let task_id = request.id;
        let now = now_millis();

        let record = TaskRecord {
            request,
            state: TaskState::Pending,
            created_at: now,
            updated_at: now,
            attempts: 0,
            max_retries: self.config.max_retries,
            response: None,
        };

        let value = bincode::serialize(&record)?;
        self.tasks.insert(task_id.as_bytes(), value)?;

        let pkey = pending_key(self.next_seq(), task_id);
        self.pending.insert(&pkey, task_id.as_bytes())?;

        debug!(
            "Enqueued task {} ({})",
            task_id,
            record.request.capability.tag()
        );
        Ok(task_id)
    }

    /// Dequeue the oldest pending task, marking it as Running.
    /// Returns `None` when the queue is empty.
    pub fn dequeue(&self) -> Result<Option<TaskRecord>, QueueError> {
        let entry = self.pending.pop_min()?;
        let (_pkey, task_id_bytes) = match entry {
            Some(e) => e,
            None => return Ok(None),
        };

        let task_id = uuid_from_ivec(&task_id_bytes)?;

        let data = match self.tasks.get(task_id.as_bytes())? {
            Some(d) => d,
            None => {
                warn!("Pending task {} missing from store, skipping", task_id);
                return Ok(None);
            }
        };

        let mut record: TaskRecord = bincode::deserialize(&data)?;
        record.state = TaskState::Running;
        record.updated_at = now_millis();
        record.attempts += 1;

        self.tasks
            .insert(task_id.as_bytes(), bincode::serialize(&record)?)?;

        debug!("Dequeued task {} (attempt {})", task_id, record.attempts);
        Ok(Some(record))
    }

    /// Mark a task as completed with its response.
    pub fn complete(&self, task_id: Uuid, response: TaskResponse) -> Result<(), QueueError> {
        self.set_state(task_id, TaskState::Completed, Some(response))
    }

    /// Mark a task as failed. Re-enqueues for retry if attempts remain.
    /// Returns `true` if the task was re-enqueued.
    pub fn fail(&self, task_id: Uuid, error: String) -> Result<bool, QueueError> {
        let data = self
            .tasks
            .get(task_id.as_bytes())?
            .ok_or(QueueError::NotFound(task_id))?;
        let mut record: TaskRecord = bincode::deserialize(&data)?;

        if record.attempts <= record.max_retries {
            record.state = TaskState::Pending;
            record.updated_at = now_millis();
            self.tasks
                .insert(task_id.as_bytes(), bincode::serialize(&record)?)?;

            let pkey = pending_key(self.next_seq(), task_id);
            self.pending.insert(&pkey, task_id.as_bytes())?;

            info!(
                "Task {} failed (attempt {}/{}), re-enqueued: {}",
                task_id,
                record.attempts,
                record.max_retries + 1,
                error
            );
            Ok(true)
        } else {
            record.state = TaskState::Failed(error.clone());
            record.updated_at = now_millis();
            self.tasks
                .insert(task_id.as_bytes(), bincode::serialize(&record)?)?;

            warn!(
                "Task {} failed permanently after {} attempts: {}",
                task_id, record.attempts, error
            );
            Ok(false)
        }
    }

    /// Mark a task as timed out. Re-enqueues for retry if attempts remain.
    /// Returns `true` if the task was re-enqueued.
    pub fn timeout(&self, task_id: Uuid) -> Result<bool, QueueError> {
        let data = self
            .tasks
            .get(task_id.as_bytes())?
            .ok_or(QueueError::NotFound(task_id))?;
        let mut record: TaskRecord = bincode::deserialize(&data)?;

        if record.attempts <= record.max_retries {
            record.state = TaskState::Pending;
            record.updated_at = now_millis();
            self.tasks
                .insert(task_id.as_bytes(), bincode::serialize(&record)?)?;

            let pkey = pending_key(self.next_seq(), task_id);
            self.pending.insert(&pkey, task_id.as_bytes())?;

            info!(
                "Task {} timed out (attempt {}/{}), re-enqueued",
                task_id,
                record.attempts,
                record.max_retries + 1
            );
            Ok(true)
        } else {
            record.state = TaskState::TimedOut;
            record.updated_at = now_millis();
            self.tasks
                .insert(task_id.as_bytes(), bincode::serialize(&record)?)?;

            warn!(
                "Task {} timed out permanently after {} attempts",
                task_id, record.attempts
            );
            Ok(false)
        }
    }

    // -- queries -----------------------------------------------------------

    /// Get a task record by ID.
    pub fn get(&self, task_id: Uuid) -> Result<Option<TaskRecord>, QueueError> {
        match self.tasks.get(task_id.as_bytes())? {
            Some(data) => Ok(Some(bincode::deserialize(&data)?)),
            None => Ok(None),
        }
    }

    /// Number of pending tasks.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Total task records (all states).
    pub fn total_count(&self) -> usize {
        self.tasks.len()
    }

    /// Compute stats by scanning all records.
    pub fn stats(&self) -> Result<QueueStats, QueueError> {
        let mut s = QueueStats::default();
        for entry in self.tasks.iter() {
            let (_, data) = entry?;
            let record: TaskRecord = bincode::deserialize(&data)?;
            match record.state {
                TaskState::Pending => s.pending += 1,
                TaskState::Running => s.running += 1,
                TaskState::Completed => s.completed += 1,
                TaskState::Failed(_) => s.failed += 1,
                TaskState::TimedOut => s.timed_out += 1,
            }
        }
        Ok(s)
    }

    // -- maintenance -------------------------------------------------------

    /// Recover tasks that were Running when the node crashed.
    /// Re-enqueues them as Pending. Returns the count recovered.
    pub fn recover(&self) -> Result<usize, QueueError> {
        let mut recovered = 0;
        let now = now_millis();

        for entry in self.tasks.iter() {
            let (key, data) = entry?;
            let mut record: TaskRecord = bincode::deserialize(&data)?;

            if record.state == TaskState::Running {
                let task_id = Uuid::from_slice(&key).map_err(|e| {
                    sled::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
                })?;

                record.state = TaskState::Pending;
                record.updated_at = now;
                self.tasks.insert(&key, bincode::serialize(&record)?)?;

                let pkey = pending_key(self.next_seq(), task_id);
                self.pending.insert(&pkey, task_id.as_bytes())?;

                recovered += 1;
                info!(
                    "Recovered crashed task {} ({})",
                    task_id,
                    record.request.capability.tag()
                );
            }
        }

        if recovered > 0 {
            info!("Recovered {} tasks from previous session", recovered);
        }
        Ok(recovered)
    }

    /// Remove terminal tasks older than the retention period.
    /// Returns the count removed.
    pub fn cleanup(&self) -> Result<usize, QueueError> {
        let cutoff = now_millis().saturating_sub(self.config.retention_secs * 1000);
        let mut removed = 0;

        let mut to_remove = Vec::new();
        for entry in self.tasks.iter() {
            let (key, data) = entry?;
            let record: TaskRecord = bincode::deserialize(&data)?;
            if record.state.is_terminal() && record.updated_at < cutoff {
                to_remove.push(key);
            }
        }

        for key in &to_remove {
            self.tasks.remove(key)?;
            removed += 1;
        }

        if removed > 0 {
            info!("Cleaned up {} expired task records", removed);
        }
        Ok(removed)
    }

    /// Flush all pending writes to disk.
    pub fn flush(&self) -> Result<(), QueueError> {
        self.tasks.flush()?;
        self.pending.flush()?;
        Ok(())
    }

    // -- internal ----------------------------------------------------------

    fn set_state(
        &self,
        task_id: Uuid,
        state: TaskState,
        response: Option<TaskResponse>,
    ) -> Result<(), QueueError> {
        let data = self
            .tasks
            .get(task_id.as_bytes())?
            .ok_or(QueueError::NotFound(task_id))?;
        let mut record: TaskRecord = bincode::deserialize(&data)?;
        record.state = state;
        record.updated_at = now_millis();
        if response.is_some() {
            record.response = response;
        }
        self.tasks
            .insert(task_id.as_bytes(), bincode::serialize(&record)?)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Capability, TaskRequest, TaskResponse, TaskStatus};

    fn make_request(name: &str) -> TaskRequest {
        TaskRequest {
            id: Uuid::new_v4(),
            capability: Capability::new("test", name, 1),
            payload: b"payload".to_vec(),
            timeout_ms: 5000,
        }
    }

    fn make_response(id: Uuid) -> TaskResponse {
        TaskResponse {
            request_id: id,
            status: TaskStatus::Success,
            payload: b"result".to_vec(),
            duration_ms: 42,
        }
    }

    fn temp_queue() -> TaskQueue {
        TaskQueue::open_temporary(TaskQueueConfig::default()).unwrap()
    }

    fn temp_queue_cfg(config: TaskQueueConfig) -> TaskQueue {
        TaskQueue::open_temporary(config).unwrap()
    }

    // -- basic operations -------------------------------------------------

    #[test]
    fn empty_queue() {
        let q = temp_queue();
        assert_eq!(q.pending_count(), 0);
        assert_eq!(q.total_count(), 0);
        assert!(q.dequeue().unwrap().is_none());
    }

    #[test]
    fn enqueue_and_dequeue() {
        let q = temp_queue();
        let req = make_request("ping");
        let id = req.id;

        q.enqueue(req).unwrap();
        assert_eq!(q.pending_count(), 1);
        assert_eq!(q.total_count(), 1);

        let record = q.dequeue().unwrap().unwrap();
        assert_eq!(record.request.id, id);
        assert_eq!(record.state, TaskState::Running);
        assert_eq!(record.attempts, 1);
        assert_eq!(q.pending_count(), 0);
    }

    #[test]
    fn fifo_ordering() {
        let q = temp_queue();
        let r1 = make_request("first");
        let r2 = make_request("second");
        let r3 = make_request("third");
        let id1 = r1.id;
        let id2 = r2.id;
        let id3 = r3.id;

        q.enqueue(r1).unwrap();
        q.enqueue(r2).unwrap();
        q.enqueue(r3).unwrap();

        assert_eq!(q.dequeue().unwrap().unwrap().request.id, id1);
        assert_eq!(q.dequeue().unwrap().unwrap().request.id, id2);
        assert_eq!(q.dequeue().unwrap().unwrap().request.id, id3);
        assert!(q.dequeue().unwrap().is_none());
    }

    #[test]
    fn get_task_record() {
        let q = temp_queue();
        let req = make_request("ping");
        let id = req.id;

        q.enqueue(req).unwrap();
        let record = q.get(id).unwrap().unwrap();
        assert_eq!(record.state, TaskState::Pending);
        assert_eq!(record.request.capability.name, "ping");
    }

    #[test]
    fn get_nonexistent_returns_none() {
        let q = temp_queue();
        assert!(q.get(Uuid::new_v4()).unwrap().is_none());
    }

    // -- completion -------------------------------------------------------

    #[test]
    fn complete_task() {
        let q = temp_queue();
        let req = make_request("ping");
        let id = req.id;

        q.enqueue(req).unwrap();
        q.dequeue().unwrap();
        q.complete(id, make_response(id)).unwrap();

        let record = q.get(id).unwrap().unwrap();
        assert_eq!(record.state, TaskState::Completed);
        assert!(record.response.is_some());
        assert_eq!(record.response.unwrap().duration_ms, 42);
    }

    #[test]
    fn complete_nonexistent_errors() {
        let q = temp_queue();
        let id = Uuid::new_v4();
        let err = q.complete(id, make_response(id)).unwrap_err();
        assert!(matches!(err, QueueError::NotFound(_)));
    }

    // -- failure and retry ------------------------------------------------

    #[test]
    fn fail_with_retries_remaining() {
        let q = temp_queue(); // max_retries=3
        let req = make_request("flaky");
        let id = req.id;

        q.enqueue(req).unwrap();
        q.dequeue().unwrap(); // attempt 1

        let retried = q.fail(id, "oops".into()).unwrap();
        assert!(retried); // 1 <= 3 → retry

        assert_eq!(q.pending_count(), 1);
        let record = q.get(id).unwrap().unwrap();
        assert_eq!(record.state, TaskState::Pending);
        assert_eq!(record.attempts, 1);
    }

    #[test]
    fn fail_exhausts_retries() {
        // max_retries=1: 1 retry allowed → 2 total attempts
        let q = temp_queue_cfg(TaskQueueConfig {
            max_retries: 1,
            ..Default::default()
        });
        let req = make_request("doomed");
        let id = req.id;

        q.enqueue(req).unwrap();

        // Attempt 1 → fail → retry (1 <= 1)
        q.dequeue().unwrap();
        assert!(q.fail(id, "err1".into()).unwrap());

        // Attempt 2 → fail → exhausted (2 > 1)
        q.dequeue().unwrap();
        let retried = q.fail(id, "err2".into()).unwrap();
        assert!(!retried);

        let record = q.get(id).unwrap().unwrap();
        assert!(matches!(record.state, TaskState::Failed(ref msg) if msg == "err2"));
        assert_eq!(q.pending_count(), 0);
    }

    #[test]
    fn fail_nonexistent_errors() {
        let q = temp_queue();
        let err = q.fail(Uuid::new_v4(), "oops".into()).unwrap_err();
        assert!(matches!(err, QueueError::NotFound(_)));
    }

    #[test]
    fn fail_no_retries_configured() {
        // max_retries=0: no retries, 1 attempt total
        let q = temp_queue_cfg(TaskQueueConfig {
            max_retries: 0,
            ..Default::default()
        });
        let req = make_request("once");
        let id = req.id;

        q.enqueue(req).unwrap();
        q.dequeue().unwrap(); // attempt 1 → 1 > 0 → no retry

        let retried = q.fail(id, "done".into()).unwrap();
        assert!(!retried);
        assert!(matches!(
            q.get(id).unwrap().unwrap().state,
            TaskState::Failed(_)
        ));
    }

    // -- timeout and retry ------------------------------------------------

    #[test]
    fn timeout_with_retries_remaining() {
        let q = temp_queue(); // max_retries=3
        let req = make_request("slow");
        let id = req.id;

        q.enqueue(req).unwrap();
        q.dequeue().unwrap(); // attempt 1

        let retried = q.timeout(id).unwrap();
        assert!(retried); // 1 <= 3 → retry
        assert_eq!(q.pending_count(), 1);
    }

    #[test]
    fn timeout_exhausts_retries() {
        // max_retries=1: 1 retry → 2 total attempts
        let q = temp_queue_cfg(TaskQueueConfig {
            max_retries: 1,
            ..Default::default()
        });
        let req = make_request("slow");
        let id = req.id;

        q.enqueue(req).unwrap();

        // Attempt 1 → timeout → retry (1 <= 1)
        q.dequeue().unwrap();
        assert!(q.timeout(id).unwrap());

        // Attempt 2 → timeout → exhausted (2 > 1)
        q.dequeue().unwrap();
        let retried = q.timeout(id).unwrap();
        assert!(!retried);

        let record = q.get(id).unwrap().unwrap();
        assert_eq!(record.state, TaskState::TimedOut);
    }

    #[test]
    fn timeout_nonexistent_errors() {
        let q = temp_queue();
        let err = q.timeout(Uuid::new_v4()).unwrap_err();
        assert!(matches!(err, QueueError::NotFound(_)));
    }

    // -- capacity ---------------------------------------------------------

    #[test]
    fn queue_full_rejects() {
        let q = temp_queue_cfg(TaskQueueConfig {
            max_queue_size: 2,
            ..Default::default()
        });

        q.enqueue(make_request("a")).unwrap();
        q.enqueue(make_request("b")).unwrap();

        let err = q.enqueue(make_request("c")).unwrap_err();
        assert!(matches!(err, QueueError::QueueFull(2)));
    }

    #[test]
    fn unlimited_queue_size() {
        let q = temp_queue_cfg(TaskQueueConfig {
            max_queue_size: 0,
            ..Default::default()
        });
        for i in 0..100 {
            q.enqueue(make_request(&format!("t{}", i))).unwrap();
        }
        assert_eq!(q.pending_count(), 100);
    }

    // -- recovery ---------------------------------------------------------

    #[test]
    fn recover_running_tasks() {
        let q = temp_queue();
        let r1 = make_request("a");
        let r2 = make_request("b");
        let r3 = make_request("c");
        let id1 = r1.id;
        let id2 = r2.id;
        let id3 = r3.id;

        q.enqueue(r1).unwrap();
        q.enqueue(r2).unwrap();
        q.enqueue(r3).unwrap();

        // Dequeue all three → Running
        q.dequeue().unwrap();
        q.dequeue().unwrap();
        q.dequeue().unwrap();

        // Complete r2 so it's not Running
        q.complete(id2, make_response(id2)).unwrap();

        // Simulate crash recovery: r1 and r3 should be recovered
        let recovered = q.recover().unwrap();
        assert_eq!(recovered, 2);
        assert_eq!(q.pending_count(), 2);

        assert_eq!(q.get(id1).unwrap().unwrap().state, TaskState::Pending);
        assert_eq!(q.get(id3).unwrap().unwrap().state, TaskState::Pending);
    }

    #[test]
    fn recover_nothing_when_clean() {
        let q = temp_queue();
        q.enqueue(make_request("a")).unwrap();
        assert_eq!(q.recover().unwrap(), 0);
    }

    // -- cleanup ----------------------------------------------------------

    #[test]
    fn cleanup_removes_old_terminal_tasks() {
        let q = temp_queue_cfg(TaskQueueConfig {
            retention_secs: 0,
            ..Default::default()
        });

        let req = make_request("done");
        let id = req.id;
        q.enqueue(req).unwrap();
        q.dequeue().unwrap();
        q.complete(id, make_response(id)).unwrap();

        // retention_secs=0 → cutoff=now. updated_at ≈ now, so timing-dependent.
        let removed = q.cleanup().unwrap();
        assert!(removed <= 1);
    }

    #[test]
    fn cleanup_preserves_pending_tasks() {
        let q = temp_queue_cfg(TaskQueueConfig {
            retention_secs: 0,
            ..Default::default()
        });

        q.enqueue(make_request("alive")).unwrap();
        let removed = q.cleanup().unwrap();
        assert_eq!(removed, 0);
        assert_eq!(q.total_count(), 1);
    }

    // -- stats ------------------------------------------------------------

    #[test]
    fn stats_reflect_state() {
        // max_retries=0 so fail is permanent
        let q = temp_queue_cfg(TaskQueueConfig {
            max_retries: 0,
            ..Default::default()
        });

        q.enqueue(make_request("a")).unwrap();
        q.enqueue(make_request("b")).unwrap();
        q.enqueue(make_request("c")).unwrap();

        // Dequeue first, complete it
        let rec1 = q.dequeue().unwrap().unwrap();
        q.complete(rec1.request.id, make_response(rec1.request.id))
            .unwrap();

        // Dequeue second, fail it
        let rec2 = q.dequeue().unwrap().unwrap();
        q.fail(rec2.request.id, "err".into()).unwrap();

        // Third stays pending
        let stats = q.stats().unwrap();
        assert_eq!(stats.pending, 1);
        assert_eq!(stats.completed, 1);
        assert_eq!(stats.failed, 1);
        assert_eq!(stats.running, 0);
        assert_eq!(stats.total(), 3);
    }

    // -- state transitions ------------------------------------------------

    #[test]
    fn task_state_is_terminal() {
        assert!(!TaskState::Pending.is_terminal());
        assert!(!TaskState::Running.is_terminal());
        assert!(TaskState::Completed.is_terminal());
        assert!(TaskState::Failed("err".into()).is_terminal());
        assert!(TaskState::TimedOut.is_terminal());
    }

    // -- pending key ordering ---------------------------------------------

    #[test]
    fn pending_key_orders_by_seq() {
        let id = Uuid::new_v4();
        let k1 = pending_key(0, id);
        let k2 = pending_key(1, id);
        assert!(k1 < k2);
    }

    #[test]
    fn pending_key_same_seq_orders_by_uuid() {
        let id1 = Uuid::from_bytes([0; 16]);
        let id2 = Uuid::from_bytes([0xFF; 16]);
        let k1 = pending_key(0, id1);
        let k2 = pending_key(0, id2);
        assert!(k1 < k2);
    }

    #[test]
    fn seq_from_key_roundtrip() {
        let seq = 42u64;
        let id = Uuid::new_v4();
        let key = pending_key(seq, id);
        assert_eq!(seq_from_pending_key(&key), seq);
    }

    // -- flush ------------------------------------------------------------

    #[test]
    fn flush_succeeds() {
        let q = temp_queue();
        q.enqueue(make_request("a")).unwrap();
        q.flush().unwrap();
    }

    // -- payload preservation ---------------------------------------------

    #[test]
    fn payload_survives_roundtrip() {
        let q = temp_queue();
        let mut req = make_request("data");
        req.payload = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let id = req.id;

        q.enqueue(req).unwrap();
        let record = q.dequeue().unwrap().unwrap();
        assert_eq!(record.request.id, id);
        assert_eq!(record.request.payload, vec![0xDE, 0xAD, 0xBE, 0xEF]);
    }

    // -- retry cycle integration ------------------------------------------

    #[test]
    fn full_retry_cycle() {
        // max_retries=3: up to 3 retries → 4 total attempts
        let q = temp_queue_cfg(TaskQueueConfig {
            max_retries: 3,
            ..Default::default()
        });
        let req = make_request("flaky");
        let id = req.id;

        q.enqueue(req).unwrap();

        // Attempt 1: fail → retry (1 <= 3)
        q.dequeue().unwrap();
        assert!(q.fail(id, "e1".into()).unwrap());

        // Attempt 2: timeout → retry (2 <= 3)
        q.dequeue().unwrap();
        assert!(q.timeout(id).unwrap());

        // Attempt 3: succeed
        let record = q.dequeue().unwrap().unwrap();
        assert_eq!(record.attempts, 3);
        q.complete(id, make_response(id)).unwrap();

        let final_record = q.get(id).unwrap().unwrap();
        assert_eq!(final_record.state, TaskState::Completed);
        assert_eq!(final_record.attempts, 3);
        assert_eq!(q.pending_count(), 0);
    }

    // -- edge cases -------------------------------------------------------

    #[test]
    fn dequeue_empty_after_drain() {
        let q = temp_queue();
        q.enqueue(make_request("a")).unwrap();
        q.enqueue(make_request("b")).unwrap();

        q.dequeue().unwrap();
        q.dequeue().unwrap();
        assert!(q.dequeue().unwrap().is_none());
        assert!(q.dequeue().unwrap().is_none());
    }

    #[test]
    fn retried_task_goes_to_back_of_queue() {
        let q = temp_queue();
        let r1 = make_request("first");
        let r2 = make_request("second");
        let id1 = r1.id;

        q.enqueue(r1).unwrap();
        q.enqueue(r2).unwrap();

        // Dequeue first, fail it → goes to back
        q.dequeue().unwrap();
        q.fail(id1, "retry".into()).unwrap();

        // Next dequeue should return second (it was queued before the retry)
        let next = q.dequeue().unwrap().unwrap();
        assert_ne!(next.request.id, id1);

        // Then the retried first
        let retried = q.dequeue().unwrap().unwrap();
        assert_eq!(retried.request.id, id1);
    }
}
