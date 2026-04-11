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

// Desktop app talks to a local axon node — configurable via env or settings
let AXON_BASE = 'http://localhost:3000';

export function setAxonBase(url: string) {
  AXON_BASE = url.replace(/\/$/, '');
}

export function getAxonBase() { return AXON_BASE; }

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${AXON_BASE}${path}`, {
    headers: { 'Content-Type': 'application/json', ...init?.headers },
    ...init,
  });
  if (!res.ok) {
    const body = await res.text().catch(() => 'error');
    throw new Error(`${res.status}: ${body}`);
  }
  return res.json() as Promise<T>;
}

export const getStatus = () => request<StatusResponse>('/api/status');
export const getPeers = () => request<PeerResponse[]>('/api/mesh/peers');
export const getAgents = () => request<AgentInfo[]>('/api/agents');
export const getTaskLog = () => request<TaskLogEntry[]>('/api/tasks/log');
export const getTaskStats = () => request<TaskStatsResponse>('/api/tasks/stats');
export const getTrust = () => request<TrustEntry[]>('/api/trust');
export const getConfig = () => request<ConfigResponse>('/api/config');

export async function* sendChatStream(req: ChatRequest): AsyncGenerator<string> {
  const res = await fetch(`${AXON_BASE}/api/chat/completions`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  });

  if (!res.ok || !res.body) throw new Error(`Chat failed: ${res.status}`);

  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buffer = '';

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split('\n');
    buffer = lines.pop() ?? '';
    for (const line of lines) {
      if (line.startsWith('data: ')) {
        const data = line.slice(6).trim();
        if (data === '[DONE]') return;
        try {
          const parsed = JSON.parse(data);
          if (parsed.content) yield parsed.content as string;
        } catch { /* skip */ }
      }
    }
  }
}
