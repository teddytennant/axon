//! Adapter between axon's agent orchestration and nous-pouw consensus.
//!
//! Each PoUW [`JobEnvelope`] specifies an opaque workflow payload + a model
//! pin. An [`AxonRunner`] implementation knows how to actually execute that
//! workflow against the axon mesh (or a deterministic stub, for v0 tests).
//! The adapter wraps the runner so it satisfies the [`WorkExecutor`] trait
//! that `nous-pouw` consumes.
//!
//! ```ignore
//! use axon_pouw::{Adapter, FakeRunner};
//! use nous_pouw::{Engine, EngineConfig};
//!
//! let runner = FakeRunner::default();
//! let mut adapter = Adapter::new(runner, signing_keys);
//! let mut engine = Engine::new(state, EngineConfig::default());
//! engine.step(&mut adapter, &[job], &leader_sk, now)?;
//! ```

pub mod adapter;
pub mod rubric;
pub mod runner;
pub mod trace;
pub mod trust_bridge;

pub use adapter::Adapter;
pub use rubric::{ExactMatchRubric, Rubric, RubricEval};
pub use runner::{AxonRunner, FakeRunner, RunnerError, WorkflowOutput};
pub use trace::{build_trace_root, TraceEvent};
pub use trust_bridge::{observation_for, TrustBridge};
