import type { WsEvent, WsEventDataFor, WsEventType } from './types';
import { getAxonBase } from './api';

type TypedCallback<T extends WsEventType> = (data: WsEventDataFor<T>) => void;
// Runtime-only holder for heterogeneous callbacks. The public API is still
// fully type-safe via `subscribe<T>`.
type RuntimeCallback = (data: WsEvent['data']) => void;

export interface WebSocketClient {
  subscribe: <T extends WsEventType>(type: T, cb: TypedCallback<T>) => () => void;
  close: () => void;
}

const MIN_DELAY = 500;
const MAX_DELAY = 30_000;
const JITTER     = 0.2; // ±20%

export function createWebSocket(): WebSocketClient {
  const subs  = new Map<WsEventType, Set<RuntimeCallback>>();
  let ws      : WebSocket | null = null;
  let closed  = false;
  let delay   = MIN_DELAY;
  let retryId : ReturnType<typeof setTimeout> | null = null;

  function connect(): void {
    if (closed) return;
    const base = getAxonBase();
    const url  = base.replace(/^http/, 'ws') + '/api/ws/live';
    ws = new WebSocket(url);

    ws.onopen = () => {
      delay = MIN_DELAY; // reset backoff on success
    };

    ws.onmessage = (e) => {
      try {
        const event = JSON.parse(e.data as string) as WsEvent;
        subs.get(event.type)?.forEach((cb) => cb(event.data));
      } catch {
        // ignore malformed frames
      }
    };

    ws.onclose = () => {
      if (closed) return;
      // Exponential backoff with jitter
      const jitter = 1 + (Math.random() * 2 - 1) * JITTER;
      retryId = setTimeout(connect, Math.min(delay * jitter, MAX_DELAY));
      delay   = Math.min(delay * 2, MAX_DELAY);
    };

    ws.onerror = () => ws?.close();
  }

  connect();

  return {
    subscribe<T extends WsEventType>(type: T, cb: TypedCallback<T>): () => void {
      if (!subs.has(type)) subs.set(type, new Set());
      const runtimeCb = cb as RuntimeCallback;
      subs.get(type)!.add(runtimeCb);
      return () => { subs.get(type)?.delete(runtimeCb); };
    },
    close(): void {
      closed = true;
      if (retryId !== null) clearTimeout(retryId);
      ws?.close();
    },
  };
}
