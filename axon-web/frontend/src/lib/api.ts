import type {
  StatusResponse,
  PeerResponse,
  AgentInfo,
  TaskLogEntry,
  TaskStatsResponse,
  TrustEntry,
  ToolResponse,
  ToolSearchParams,
  ConfigResponse,
  LlmConfigSection,
  ModelResponse,
  ChatRequest,
  ValidateResponse,
} from './types';

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, {
    headers: { 'Content-Type': 'application/json', ...init?.headers },
    ...init,
  });
  if (!res.ok) {
    const body = await res.text().catch(() => 'Unknown error');
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
export const getPeerTrust = (peerId: string) => request<TrustEntry>(`/api/trust/${peerId}`);
export const getTools = () => request<ToolResponse[]>('/api/tools');
export const searchTools = (params: ToolSearchParams) => {
  const qs = new URLSearchParams();
  if (params.q) qs.set('q', params.q);
  if (params.server) qs.set('server', params.server);
  if (params.limit) qs.set('limit', String(params.limit));
  return request<ToolResponse[]>(`/api/tools/search?${qs}`);
};
export const getModels = (provider: string) =>
  request<ModelResponse[]>(`/api/models/${provider}`);
export const getConfig = () => request<ConfigResponse>('/api/config');

export const updateConfig = (data: Partial<ConfigResponse>) =>
  request<{ ok: boolean }>('/api/config', {
    method: 'PUT',
    body: JSON.stringify(data),
  });

export const updateLlmConfig = (data: LlmConfigSection) =>
  request<{ ok: boolean }>('/api/config/llm', {
    method: 'PUT',
    body: JSON.stringify(data),
  });

export const validateKey = (provider: string, api_key: string) =>
  request<ValidateResponse>('/api/auth/validate', {
    method: 'POST',
    body: JSON.stringify({ provider, api_key }),
  });

export const setKey = (provider: string, api_key: string) =>
  request<{ ok: boolean }>(`/api/auth/key/${provider}`, {
    method: 'PUT',
    body: JSON.stringify({ api_key }),
  });

/** Returns an EventSource for SSE streaming. Caller is responsible for closing it. */
export function sendChat(_req: ChatRequest): EventSource {
  // POST via EventSource isn't supported; use fetch SSE pattern
  // We POST the body, then read the response as a stream
  // For simplicity, use a URL-encoded approach — but SSE requires GET
  // So we send the request body as a query param (base64) — or better: use fetch API
  // Actually, we'll use fetch with ReadableStream
  throw new Error('Use sendChatFetch instead');
}

/** Stream chat response via fetch + ReadableStream. Returns async generator of chunks. */
export async function* sendChatStream(req: ChatRequest): AsyncGenerator<string> {
  const res = await fetch('/api/chat/completions', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  });

  if (!res.ok || !res.body) {
    throw new Error(`Chat failed: ${res.status}`);
  }

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
        } catch {
          // skip malformed
        }
      }
    }
  }
}
