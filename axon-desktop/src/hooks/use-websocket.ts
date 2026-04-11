import { useEffect } from 'react';
import { createWebSocket } from '../lib/ws';
import type { WebSocketClient } from '../lib/ws';
import type { WsEventType } from '../lib/types';

let globalWs: WebSocketClient | null = null;

function getGlobalWs(): WebSocketClient {
  if (!globalWs) globalWs = createWebSocket();
  return globalWs;
}

export function useWebSocket() {
  useEffect(() => {
    getGlobalWs();
  }, []);

  function subscribe(type: WsEventType, cb: (data: unknown) => void) {
    return getGlobalWs().subscribe(type, cb);
  }

  return { subscribe };
}
