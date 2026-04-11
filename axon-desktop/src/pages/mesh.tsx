import { useState, useEffect } from 'react';
import { usePeers } from '../hooks/use-api';
import { useWebSocket } from '../hooks/use-websocket';
import type { PeerResponse } from '../lib/types';

export default function MeshPage() {
  const { data: init, isLoading } = usePeers();
  const { subscribe }             = useWebSocket();
  const [peers, setPeers]         = useState<PeerResponse[]>([]);

  useEffect(() => { if (init) setPeers(init); }, [init]);
  useEffect(() => subscribe('peers', d => setPeers(d as PeerResponse[])), [subscribe]);

  if (isLoading) return <Skeleton />;

  return (
    <div className="h-full overflow-auto p-5">
      <div className="mb-5 flex items-center gap-3">
        <span className="text-[11px] text-[#666]">mesh</span>
        <span className="text-[10px] text-[#333] tabular-nums">{peers.length}</span>
      </div>

      {peers.length === 0 ? (
        <div className="flex h-48 items-center justify-center">
          <p className="text-[11px] text-[#2a2a2a]">no peers connected</p>
        </div>
      ) : (
        <div className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
          {peers.map(p => <PeerCard key={p.peer_id} peer={p} />)}
        </div>
      )}
    </div>
  );
}

function PeerCard({ peer: p }: { peer: PeerResponse }) {
  return (
    <div className="rounded border border-[#1f1f1f] bg-[#111] p-4">
      <div className="mb-2 flex items-start justify-between gap-2">
        <span className="truncate text-[11px] text-[#eee]" title={p.peer_id}>
          {p.peer_id.slice(0, 16)}…
        </span>
        <span className="shrink-0 text-[10px] text-[#333]">{p.last_seen_ago}</span>
      </div>

      <p className="mb-3 text-[10px] text-[#444]">{p.addr}</p>

      {p.capabilities.length > 0 && (
        <div className="flex flex-wrap gap-1">
          {p.capabilities.map(c => (
            <span key={c} className="rounded bg-[#1a1a1a] px-1.5 py-px text-[9px] text-[#555]">{c}</span>
          ))}
        </div>
      )}
    </div>
  );
}

function Skeleton() {
  return (
    <div className="p-5">
      <div className="mb-5 h-4 w-16 rounded bg-[#1a1a1a] animate-pulse" />
      <div className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
        {Array.from({ length: 6 }).map((_, i) => (
          <div key={i} className="h-28 rounded border border-[#1a1a1a] bg-[#111] animate-pulse" />
        ))}
      </div>
    </div>
  );
}
