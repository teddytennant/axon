// Types matching the axon-web Rust API responses exactly

export interface StatusResponse {
  peer_id: string;
  listen_addr: string;
  uptime_secs: number;
  peer_count: number;
  agent_count: number;
  tasks_total: number;
  tasks_failed: number;
  messages_received: number;
  messages_sent: number;
  provider: string;
  model: string;
  mcp_tool_count: number;
  version: string;
}

export interface PeerResponse {
  peer_id: string;
  addr: string;
  capabilities: string[];
  last_seen: number;
  last_seen_ago: string;
}

export interface AgentInfo {
  name: string;
  capabilities: string[];
  provider_type: string;
  model_name: string;
  status: string;
  tasks_handled: number;
  tasks_succeeded: number;
  avg_latency_ms: number;
  lifecycle_state: string;
  last_heartbeat_secs_ago: number | null;
}

export interface TaskLogEntry {
  id: string;
  capability: string;
  status: string;
  duration_ms: number;
  peer: string;
}

export interface TaskStatsResponse {
  pending: number;
  running: number;
  completed: number;
  failed: number;
  timed_out: number;
  total: number;
}

export interface TrustEntry {
  peer_id: string;
  reliability: number;
  accuracy: number;
  availability: number;
  quality: number;
  overall: number;
  confidence: number;
  observation_count: number;
}

export interface ToolResponse {
  name: string;
  server: string;
  description: string;
  peer_id: string;
  score: number;
}

export interface ToolSearchParams {
  q?: string;
  server?: string;
  limit?: number;
  budget?: number;
}

export interface NodeConfigSection {
  listen: string;
  peers: string[];
  headless: boolean;
  health_port: number | null;
  web_port: number | null;
}

export interface LlmConfigSection {
  provider: string;
  endpoint: string;
  api_key: string;
  model: string;
}

export interface McpServerConfig {
  name: string;
  command: string;
  args: string[];
  timeout_secs: number;
}

export interface McpConfigSection {
  servers: McpServerConfig[];
}

export interface ConfigResponse {
  node: NodeConfigSection;
  llm: LlmConfigSection;
  mcp: McpConfigSection;
}

export interface ModelResponse {
  id: string;
  name: string;
  description: string;
  context_length: number | null;
}

export interface ChatMessage {
  role: 'user' | 'assistant' | 'system';
  content: string;
}

export interface ChatRequest {
  messages: ChatMessage[];
  model?: string;
  provider?: string;
  max_tokens?: number;
  temperature?: number;
}

export interface ValidateResponse {
  valid: boolean;
  error: string | null;
}

export interface StepSnapshot {
  capability: string;
  status: string;
  latency_ms: number;
  payload_bytes: number;
}

export interface WorkflowSnapshot {
  id: string;
  pattern: string;
  steps_completed: number;
  steps_total: number;
  status: string;
  duration_ms: number;
  started_at: string;
  steps: StepSnapshot[];
}

export interface WorkflowsResponse {
  active: WorkflowSnapshot[];
  completed: WorkflowSnapshot[];
}

export interface BlackboardEntry {
  key: string;
  value: string;
  timestamp_ms: number;
}

// WebSocket event types
export type WsEventType = 'metrics' | 'peers' | 'agents' | 'tasks' | 'trust' | 'log' | 'workflows' | 'blackboard';

export interface WsEvent {
  type: WsEventType;
  data: unknown;
}

export interface WsMetricsData {
  uptime_secs: number;
  tasks_total: number;
  tasks_failed: number;
  messages_received: number;
  messages_sent: number;
  throughput: number[];
}

export interface WsTasksData {
  stats: {
    pending: number;
    running: number;
    completed: number;
    failed: number;
    timed_out: number;
  };
  recent: TaskLogEntry[];
}
