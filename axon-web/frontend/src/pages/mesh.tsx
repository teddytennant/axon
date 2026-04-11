import { useState, useEffect } from 'react';
import { usePeers } from '../hooks/use-api';
import { useWebSocket } from '../hooks/use-websocket';
import type { PeerResponse } from '../lib/types';

export default function MeshPage() {
  const { data: initialPeers, isLoading } = usePeers();
  const { subscribe } = useWebSocket();
  const [peers, setPeers] = useState<PeerResponse[]>([]);

  useEffect(() => { if (initialPeers) setPeers(initialPeers); }, [initialPeers]);

  useEffect(() => {
    return subscribe('peers', (data) => {
      setPeers(data as PeerResponse[]);
    });
  }, [subscribe]);

  if (isLoading) return <LoadingSkeleton />;

  return (
    <div className="p-6">
      <div className="mb-6 flex items-center gap-3">
        <h1 className="text-sm font-medium text-white">Mesh</h1>
        <span className="font-mono text-xs text-[#3a3a3a] tabular-nums">{peers.length}</span>
      </div>

      {peers.length === 0 ? (
        <div className="flex flex-col items-center justify-center py-24">
          <p className="text-sm text-[#3a3a3a]">No peers connected</p>
        </div>
      ) : (
        <div className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
          {peers.map((peer) => <PeerCard key={peer.peer_id} peer={peer} />)}
        </div>
      )}
    </div>
  );
}

function PeerCard({ peer }: { peer: PeerResponse }) {
  return (
    <div className="rounded border border-[#1c1c1c] bg-[#0c0c0c] p-4">
      <div className="mb-3 flex items-start justify-between gap-2">
        <p className="truncate font-mono text-xs text-white" title={peer.peer_id}>
          {peer.peer_id.slice(0, 16)}…
        </p>
        <span className="shrink-0 text-[10px] text-[#3a3a3a]">{peer.last_seen_ago}</span>
      </div>

      <p className="mb-3 font-mono text-xs text-[#6b6b6b]">{peer.addr}</p>

      {peer.capabilities.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {peer.capabilities.map((cap) => (
            <span key={cap} className="rounded border border-[#1c1c1c] px-2 py-0.5 font-mono text-[10px] text-[#6b6b6b]">{cap}</span>
          ))}
        </div>
      )}
    </div>
  );
}

function LoadingSkeleton() {
  return (
    <div className="p-6">
      <div className="mb-6 h-5 w-16 animate-pulse rounded bg-[#141414]" />
      <div className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
        {Array.from({ length: 6 }).map((_, i) => (
          <div key={i} className="h-36 animate-pulse rounded border border-[#1c1c1c] bg-[#0c0c0c]" />
        ))}
      </div>
    </div>
  );
}
