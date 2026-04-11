import { useState, useEffect } from 'react';
import { Globe } from 'lucide-react';
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
        <h1 className="text-lg font-semibold text-[#f5f5f5]">Mesh</h1>
        <span className="rounded-full bg-[#00c8c8]/10 px-2.5 py-0.5 font-mono text-xs text-[#00c8c8]">{peers.length}</span>
      </div>

      {peers.length === 0 ? (
        <div className="flex flex-col items-center justify-center py-24">
          <Globe size={32} className="mb-3 text-[#555]" />
          <p className="text-sm text-[#555]">No peers connected</p>
        </div>
      ) : (
        <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
          {peers.map((peer) => <PeerCard key={peer.peer_id} peer={peer} />)}
        </div>
      )}
    </div>
  );
}

function PeerCard({ peer }: { peer: PeerResponse }) {
  return (
    <div className="rounded-lg border border-[#222] bg-[#111] p-4">
      <div className="mb-3 flex items-start justify-between gap-2">
        <p className="truncate font-mono text-xs text-[#f5f5f5]" title={peer.peer_id}>
          {peer.peer_id.slice(0, 16)}…
        </p>
        <span className="shrink-0 text-[10px] text-[#555]">{peer.last_seen_ago}</span>
      </div>

      <p className="mb-3 font-mono text-xs text-[#888]">{peer.addr}</p>

      {peer.capabilities.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {peer.capabilities.map((cap) => (
            <span key={cap} className="rounded bg-[#181818] px-2 py-0.5 font-mono text-[10px] text-[#888]">{cap}</span>
          ))}
        </div>
      )}
    </div>
  );
}

function LoadingSkeleton() {
  return (
    <div className="p-6">
      <div className="mb-6 h-6 w-24 animate-pulse rounded bg-[#181818]" />
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
        {Array.from({ length: 6 }).map((_, i) => (
          <div key={i} className="h-40 animate-pulse rounded-lg border border-[#222] bg-[#111]" />
        ))}
      </div>
    </div>
  );
}
