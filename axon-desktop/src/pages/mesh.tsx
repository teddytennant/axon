import { useState, useEffect } from 'react';
import { usePeers } from '../hooks/use-api';
import { useWebSocket } from '../hooks/use-websocket';
import { EmptyState, GridSkeleton, PageHeader } from '../components/page';
import type { PeerResponse } from '../lib/types';

export default function MeshPage() {
  const { data: init, isLoading } = usePeers();
  const { subscribe }             = useWebSocket();
  const [peers, setPeers]         = useState<PeerResponse[]>([]);

  useEffect(() => { if (init) setPeers(init); }, [init]);
  useEffect(() => subscribe('peers', d => setPeers(d)), [subscribe]);

  if (isLoading) return <GridSkeleton cards={6} />;

  return (
    <div className="h-full overflow-auto p-6">
      <PageHeader label="mesh" count={peers.length} />

      {peers.length === 0 ? (
        <EmptyState text="no peers connected" />
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
    <div className="rounded-lg border border-[#1e1e1e] bg-[#080808] p-4 hover:border-[#282828] transition-colors">
      {/* Header */}
      <div className="mb-3 flex items-start justify-between gap-3">
        <div className="flex items-center gap-2 min-w-0">
          <span className="h-[5px] w-[5px] shrink-0 rounded-full bg-[#2a2a2a]" />
          <span
            className="truncate font-mono text-[11px] text-[#c8c8c8]"
            title={p.peer_id}
          >
            {p.peer_id.slice(0, 16)}&hellip;
          </span>
        </div>
        <span className="shrink-0 text-[9px] tabular-nums text-[#2e2e2e]">{p.last_seen_ago}</span>
      </div>

      {/* Address */}
      <p className="mb-3 text-[10px] text-[#3a3a3a] font-mono">{p.addr}</p>

      {/* Capabilities */}
      {p.capabilities.length > 0 && (
        <div className="flex flex-wrap gap-1">
          {p.capabilities.map(c => (
            <span key={c} className="rounded-md border border-[#1a1a1a] bg-[#0e0e0e] px-1.5 py-px text-[9px] text-[#444]">
              {c}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

