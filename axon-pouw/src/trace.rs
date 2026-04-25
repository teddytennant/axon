//! Trace events + Merkle root.
//!
//! Each workflow run produces an ordered log of [`TraceEvent`]s. The
//! [`build_trace_root`] function hashes them into a 32-byte root that goes
//! into the [`WorkReceipt`]. Two workers with byte-identical traces produce
//! the same root, so trace_root is a useful equality witness for v1 fraud
//! proofs without revealing the full trace on chain.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TraceEvent {
    WorkflowStart,
    StepStart {
        step: u32,
    },
    StepComplete {
        step: u32,
        output_hash: [u8; 32],
    },
    /// Inline a tool call's input/output hashes — large bytes stay off-chain.
    ToolCall {
        step: u32,
        tool_name: String,
        input_hash: [u8; 32],
        output_hash: [u8; 32],
    },
    WorkflowEnd {
        ok: bool,
    },
}

/// Compute the trace Merkle root: simple sequential hash chain.
///
/// `root_0 = blake3(canonical_event_0)`
/// `root_n = blake3(root_{n-1} || canonical_event_n)`
///
/// Not a balanced Merkle tree (we don't need range proofs for v0); just a
/// deterministic digest that changes if any event or order changes.
pub fn build_trace_root(events: &[TraceEvent]) -> [u8; 32] {
    let mut acc = [0u8; 32];
    for e in events {
        let bytes = serde_json::to_vec(e).expect("TraceEvent is JSON-serializable");
        let mut h = blake3::Hasher::new();
        h.update(&acc);
        h.update(&bytes);
        acc = *h.finalize().as_bytes();
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evts() -> Vec<TraceEvent> {
        vec![
            TraceEvent::WorkflowStart,
            TraceEvent::StepStart { step: 0 },
            TraceEvent::StepComplete {
                step: 0,
                output_hash: [1; 32],
            },
            TraceEvent::WorkflowEnd { ok: true },
        ]
    }

    #[test]
    fn root_is_deterministic() {
        assert_eq!(build_trace_root(&evts()), build_trace_root(&evts()));
    }

    #[test]
    fn root_changes_with_event_order() {
        let mut reordered = evts();
        reordered.swap(1, 2);
        assert_ne!(build_trace_root(&evts()), build_trace_root(&reordered));
    }

    #[test]
    fn root_changes_with_event_content() {
        let mut tweaked = evts();
        if let TraceEvent::StepComplete { output_hash, .. } = &mut tweaked[2] {
            output_hash[0] ^= 1;
        }
        assert_ne!(build_trace_root(&evts()), build_trace_root(&tweaked));
    }

    #[test]
    fn empty_trace_has_zero_root() {
        assert_eq!(build_trace_root(&[]), [0u8; 32]);
    }

    #[test]
    fn tool_call_events_distinguishable() {
        let with_tool: Vec<TraceEvent> = vec![TraceEvent::ToolCall {
            step: 0,
            tool_name: "read_file".into(),
            input_hash: [1; 32],
            output_hash: [2; 32],
        }];
        let without_tool: Vec<TraceEvent> = vec![TraceEvent::StepComplete {
            step: 0,
            output_hash: [2; 32],
        }];
        assert_ne!(
            build_trace_root(&with_tool),
            build_trace_root(&without_tool)
        );
    }
}
