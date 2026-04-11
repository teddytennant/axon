import type { WsEvent, WsEventType } from './types';

type Callback = (data: unknown) => void;

export interface WebSocketClient {
  subscribe: (type: WsEventType, cb: Callback) => () => void;
  close: () => void;
}

export function createWebSocket(): WebSocketClient {
  const subs = new Map<WsEventType, Set<Callback>>();
  let ws: WebSocket | null = null;
  let closed = false;

  function connect() {
    if (closed) return;
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const url = `${protocol}//${window.location.host}/api/ws/live`;
    ws = new WebSocket(url);

    ws.onmessage = (e) => {
      try {
        const event: WsEvent = JSON.parse(e.data as string);
        const listeners = subs.get(event.type);
        if (listeners) listeners.forEach((cb) => cb(event.data));
      } catch {
        // ignore parse errors
      }
    };

    ws.onclose = () => {
      if (!closed) setTimeout(connect, 2000);
    };

    ws.onerror = () => {
      ws?.close();
    };
  }

  connect();

  return {
    subscribe(type, cb) {
      if (!subs.has(type)) subs.set(type, new Set());
      subs.get(type)!.add(cb);
      return () => subs.get(type)?.delete(cb);
    },
    close() {
      closed = true;
      ws?.close();
    },
  };
}
