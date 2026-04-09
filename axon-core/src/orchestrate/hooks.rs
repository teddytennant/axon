use crate::protocol::{TaskRequest, TaskResponse};

/// When a hook fires relative to agent task handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookPhase {
    /// Fires before `Agent::handle()`. Can modify payload or short-circuit.
    BeforeHandle,
    /// Fires after `Agent::handle()`. Can modify the response payload.
    AfterHandle,
}

/// Result returned by a hook execution.
#[derive(Debug)]
pub enum HookResult {
    /// Continue processing with this (possibly modified) payload.
    Continue(Vec<u8>),
    /// Skip remaining processing and return this response directly.
    /// Only valid in `BeforeHandle` phase; ignored in `AfterHandle`.
    ShortCircuit(TaskResponse),
}

/// A pre/post message processing hook.
///
/// Hooks are capability-gated: the agent definition declares
/// `permissions = ["mesh:send", "blackboard:write"]` and a hook
/// declares `required_permissions() = ["blackboard:write"]`.
/// The `HookRegistry` only runs hooks whose required permissions
/// are satisfied by the agent's granted permissions.
pub trait Hook: Send + Sync + 'static {
    fn name(&self) -> &str;
    fn phase(&self) -> HookPhase;
    /// Permissions this hook needs to run.
    fn required_permissions(&self) -> Vec<String>;
    /// Execute the hook. Receives the current payload; returns modified payload or short-circuit.
    fn execute(&self, request: &TaskRequest, payload: &[u8]) -> HookResult;
}

/// Manages registered hooks and runs them in order.
pub struct HookRegistry {
    hooks: Vec<Box<dyn Hook>>,
}

impl HookRegistry {
    pub fn new() -> Self {
        Self { hooks: Vec::new() }
    }

    /// Register a hook. Hooks run in registration order.
    pub fn register(&mut self, hook: Box<dyn Hook>) {
        self.hooks.push(hook);
    }

    /// Run all hooks for a given phase.
    ///
    /// Only hooks whose `required_permissions` are fully satisfied by
    /// `granted_permissions` are executed. Returns the final payload
    /// (possibly modified) or the first short-circuit response.
    pub fn run(
        &self,
        phase: HookPhase,
        request: &TaskRequest,
        payload: &[u8],
        granted_permissions: &[String],
    ) -> HookResult {
        let mut current_payload = payload.to_vec();

        for hook in &self.hooks {
            if hook.phase() != phase {
                continue;
            }
            // Permission check: all required permissions must be in granted set
            let permitted = hook
                .required_permissions()
                .iter()
                .all(|req| granted_permissions.contains(req));
            if !permitted {
                continue;
            }

            match hook.execute(request, &current_payload) {
                HookResult::Continue(p) => current_payload = p,
                HookResult::ShortCircuit(resp) => return HookResult::ShortCircuit(resp),
            }
        }

        HookResult::Continue(current_payload)
    }
}

impl Default for HookRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::TaskStatus;

    fn make_request(payload: &[u8]) -> TaskRequest {
        TaskRequest {
            id: uuid::Uuid::new_v4(),
            capability: crate::protocol::Capability::new("test", "op", 1),
            payload: payload.to_vec(),
            timeout_ms: 5000,
        }
    }

    fn all_perms() -> Vec<String> {
        vec!["perm:a".to_string(), "perm:b".to_string()]
    }

    fn no_perms() -> Vec<String> {
        vec![]
    }

    /// Hook that uppercases the payload (ASCII).
    struct UppercaseHook;
    impl Hook for UppercaseHook {
        fn name(&self) -> &str {
            "uppercase"
        }
        fn phase(&self) -> HookPhase {
            HookPhase::BeforeHandle
        }
        fn required_permissions(&self) -> Vec<String> {
            vec![]
        }
        fn execute(&self, _: &TaskRequest, payload: &[u8]) -> HookResult {
            HookResult::Continue(payload.to_ascii_uppercase())
        }
    }

    /// Hook that reverses the payload bytes.
    struct ReverseHook;
    impl Hook for ReverseHook {
        fn name(&self) -> &str {
            "reverse"
        }
        fn phase(&self) -> HookPhase {
            HookPhase::BeforeHandle
        }
        fn required_permissions(&self) -> Vec<String> {
            vec![]
        }
        fn execute(&self, _: &TaskRequest, payload: &[u8]) -> HookResult {
            let mut v = payload.to_vec();
            v.reverse();
            HookResult::Continue(v)
        }
    }

    /// Hook that appends " modified" to the response payload.
    struct AppendAfterHook;
    impl Hook for AppendAfterHook {
        fn name(&self) -> &str {
            "append-after"
        }
        fn phase(&self) -> HookPhase {
            HookPhase::AfterHandle
        }
        fn required_permissions(&self) -> Vec<String> {
            vec![]
        }
        fn execute(&self, _: &TaskRequest, payload: &[u8]) -> HookResult {
            let mut v = payload.to_vec();
            v.extend_from_slice(b" modified");
            HookResult::Continue(v)
        }
    }

    /// Hook that short-circuits with a fixed response.
    struct ShortCircuitHook;
    impl Hook for ShortCircuitHook {
        fn name(&self) -> &str {
            "short-circuit"
        }
        fn phase(&self) -> HookPhase {
            HookPhase::BeforeHandle
        }
        fn required_permissions(&self) -> Vec<String> {
            vec![]
        }
        fn execute(&self, request: &TaskRequest, _: &[u8]) -> HookResult {
            HookResult::ShortCircuit(TaskResponse {
                request_id: request.id,
                status: TaskStatus::Success,
                payload: b"short-circuited".to_vec(),
                duration_ms: 0,
            })
        }
    }

    /// Hook that requires "perm:a" to run.
    struct PermissionedHook;
    impl Hook for PermissionedHook {
        fn name(&self) -> &str {
            "permissioned"
        }
        fn phase(&self) -> HookPhase {
            HookPhase::BeforeHandle
        }
        fn required_permissions(&self) -> Vec<String> {
            vec!["perm:a".to_string()]
        }
        fn execute(&self, _: &TaskRequest, _: &[u8]) -> HookResult {
            HookResult::Continue(b"ran-permissioned".to_vec())
        }
    }

    #[test]
    fn hook_registry_empty_continues_unchanged() {
        let reg = HookRegistry::new();
        let req = make_request(b"data");
        match reg.run(HookPhase::BeforeHandle, &req, b"data", &no_perms()) {
            HookResult::Continue(p) => assert_eq!(p, b"data"),
            HookResult::ShortCircuit(_) => panic!("should not short-circuit"),
        }
    }

    #[test]
    fn before_hook_modifies_payload() {
        let mut reg = HookRegistry::new();
        reg.register(Box::new(UppercaseHook));
        let req = make_request(b"hello");
        match reg.run(HookPhase::BeforeHandle, &req, b"hello", &no_perms()) {
            HookResult::Continue(p) => assert_eq!(p, b"HELLO"),
            _ => panic!("expected continue"),
        }
    }

    #[test]
    fn after_hook_modifies_response() {
        let mut reg = HookRegistry::new();
        reg.register(Box::new(AppendAfterHook));
        let req = make_request(b"");
        match reg.run(HookPhase::AfterHandle, &req, b"result", &no_perms()) {
            HookResult::Continue(p) => assert_eq!(p, b"result modified"),
            _ => panic!("expected continue"),
        }
    }

    #[test]
    fn before_hook_does_not_run_in_after_phase() {
        let mut reg = HookRegistry::new();
        reg.register(Box::new(UppercaseHook)); // phase = BeforeHandle
        let req = make_request(b"");
        // Running in AfterHandle phase — UppercaseHook should not fire
        match reg.run(HookPhase::AfterHandle, &req, b"data", &no_perms()) {
            HookResult::Continue(p) => assert_eq!(p, b"data"),
            _ => panic!("expected continue unchanged"),
        }
    }

    #[test]
    fn hook_short_circuits() {
        let mut reg = HookRegistry::new();
        reg.register(Box::new(ShortCircuitHook));
        let req = make_request(b"anything");
        match reg.run(HookPhase::BeforeHandle, &req, b"anything", &no_perms()) {
            HookResult::ShortCircuit(resp) => {
                assert_eq!(resp.status, TaskStatus::Success);
                assert_eq!(resp.payload, b"short-circuited");
            }
            HookResult::Continue(_) => panic!("expected short-circuit"),
        }
    }

    #[test]
    fn short_circuit_stops_subsequent_hooks() {
        let mut reg = HookRegistry::new();
        reg.register(Box::new(ShortCircuitHook));
        reg.register(Box::new(UppercaseHook)); // should never run
        let req = make_request(b"data");
        match reg.run(HookPhase::BeforeHandle, &req, b"data", &no_perms()) {
            HookResult::ShortCircuit(resp) => assert_eq!(resp.payload, b"short-circuited"),
            _ => panic!("expected short-circuit"),
        }
    }

    #[test]
    fn hook_without_permission_skipped() {
        let mut reg = HookRegistry::new();
        reg.register(Box::new(PermissionedHook));
        let req = make_request(b"original");
        // No permissions granted — PermissionedHook should not run
        match reg.run(HookPhase::BeforeHandle, &req, b"original", &no_perms()) {
            HookResult::Continue(p) => assert_eq!(p, b"original"),
            _ => panic!("expected continue unchanged"),
        }
    }

    #[test]
    fn hook_with_permission_runs() {
        let mut reg = HookRegistry::new();
        reg.register(Box::new(PermissionedHook));
        let req = make_request(b"original");
        match reg.run(HookPhase::BeforeHandle, &req, b"original", &all_perms()) {
            HookResult::Continue(p) => assert_eq!(p, b"ran-permissioned"),
            _ => panic!("expected continue"),
        }
    }

    #[test]
    fn multiple_hooks_chain_correctly() {
        let mut reg = HookRegistry::new();
        reg.register(Box::new(UppercaseHook)); // "hello" -> "HELLO"
        reg.register(Box::new(ReverseHook)); // "HELLO" -> "OLLEH"
        let req = make_request(b"hello");
        match reg.run(HookPhase::BeforeHandle, &req, b"hello", &no_perms()) {
            HookResult::Continue(p) => assert_eq!(p, b"OLLEH"),
            _ => panic!("expected continue"),
        }
    }

    #[test]
    fn hooks_run_in_registration_order() {
        // Reverse first, then uppercase: "hello" -> "olleh" -> "OLLEH"
        let mut reg = HookRegistry::new();
        reg.register(Box::new(ReverseHook));
        reg.register(Box::new(UppercaseHook));
        let req = make_request(b"hello");
        match reg.run(HookPhase::BeforeHandle, &req, b"hello", &no_perms()) {
            HookResult::Continue(p) => assert_eq!(p, b"OLLEH"),
            _ => panic!("expected continue"),
        }
    }
}
