import { useEffect } from 'react';
import { createWebSocket } from '../lib/ws';
import type { WebSocketClient } from '../lib/ws';
import type { WsEventDataFor, WsEventType } from '../lib/types';

let globalWs: WebSocketClient | null = null;

function getGlobalWs(): WebSocketClient {
  if (!globalWs) globalWs = createWebSocket();
  return globalWs;
}

export function useWebSocket(): {
  subscribe: <T extends WsEventType>(type: T, cb: (data: WsEventDataFor<T>) => void) => () => void;
} {
  useEffect(() => {
    getGlobalWs();
  }, []);

  function subscribe<T extends WsEventType>(
    type: T,
    cb: (data: WsEventDataFor<T>) => void,
  ): () => void {
    return getGlobalWs().subscribe(type, cb);
  }

  return { subscribe };
}
