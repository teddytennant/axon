import type {
  StatusResponse,
  PeerResponse,
  AgentInfo,
  TaskLogEntry,
  TaskStatsResponse,
  TrustEntry,
  ConfigResponse,
  ChatRequest,
} from './types';

let AXON_BASE = 'http://localhost:3000';

export function setAxonBase(url: string) {
  AXON_BASE = url.replace(/\/$/, '');
  // Persist for next session
  try { localStorage.setItem('axon_base', AXON_BASE); } catch { /* ignore */ }
}

export function getAxonBase() { return AXON_BASE; }

// Restore persisted base URL on module load
try {
  const saved = localStorage.getItem('axon_base');
  if (saved) AXON_BASE = saved;
} catch { /* ignore */ }

async function request<T>(path: string, signal?: AbortSignal, init?: RequestInit): Promise<T> {
  const res = await fetch(`${AXON_BASE}${path}`, {
    signal,
    headers: { 'Content-Type': 'application/json', ...init?.headers },
    ...init,
  });
  if (!res.ok) {
    const body = await res.text().catch(() => '');
    throw new Error(`${res.status}${body ? ': ' + body : ''}`);
  }
  return res.json() as Promise<T>;
}

export const getStatus    = (s?: AbortSignal) => request<StatusResponse>('/api/status', s);
export const getPeers     = (s?: AbortSignal) => request<PeerResponse[]>('/api/mesh/peers', s);
export const getAgents    = (s?: AbortSignal) => request<AgentInfo[]>('/api/agents', s);
export const getTaskLog   = (s?: AbortSignal) => request<TaskLogEntry[]>('/api/tasks/log', s);
export const getTaskStats = (s?: AbortSignal) => request<TaskStatsResponse>('/api/tasks/stats', s);
export const getTrust     = (s?: AbortSignal) => request<TrustEntry[]>('/api/trust', s);
export const getConfig    = (s?: AbortSignal) => request<ConfigResponse>('/api/config', s);

export async function* sendChatStream(req: ChatRequest): AsyncGenerator<string> {
  const res = await fetch(`${AXON_BASE}/api/chat/completions`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  });

  if (!res.ok || !res.body) throw new Error(`Chat failed: ${res.status}`);

  const reader  = res.body.getReader();
  const decoder = new TextDecoder();
  let buffer    = '';

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split('\n');
    buffer = lines.pop() ?? '';
    for (const line of lines) {
      if (!line.startsWith('data: ')) continue;
      const data = line.slice(6).trim();
      if (data === '[DONE]') return;
      try {
        const parsed = JSON.parse(data);
        if (parsed.content) yield parsed.content as string;
      } catch { /* skip */ }
    }
  }
}
