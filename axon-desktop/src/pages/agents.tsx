import { useState, useEffect } from 'react';
import { clsx } from 'clsx';
import { useAgents } from '../hooks/use-api';
import { useWebSocket } from '../hooks/use-websocket';
import type { AgentInfo } from '../lib/types';

export default function AgentsPage() {
  const { data: init, isLoading } = useAgents();
  const { subscribe }             = useWebSocket();
  const [agents, setAgents]       = useState<AgentInfo[]>([]);

  useEffect(() => { if (init) setAgents(init); }, [init]);
  useEffect(() => subscribe('agents', d => setAgents(d as AgentInfo[])), [subscribe]);

  if (isLoading) return <Skeleton />;

  return (
    <div className="h-full overflow-auto p-5">
      <div className="mb-5 flex items-center gap-3">
        <span className="text-[11px] text-[#666]">agents</span>
        <span className="text-[10px] text-[#333] tabular-nums">{agents.length}</span>
      </div>

      {agents.length === 0 ? (
        <Empty text="no agents registered" />
      ) : (
        <div className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
          {agents.map(a => <AgentCard key={a.name} agent={a} />)}
        </div>
      )}
    </div>
  );
}

const STATUS: Record<string, string> = {
  idle: '#22c55e', busy: '#f59e0b', err: '#ef4444',
};

function AgentCard({ agent: a }: { agent: AgentInfo }) {
  const statusColor = STATUS[a.status.toLowerCase()] ?? '#555';
  const success     = a.tasks_handled > 0
    ? Math.round((a.tasks_succeeded / a.tasks_handled) * 100)
    : null;

  return (
    <div className="rounded border border-[#1f1f1f] bg-[#111] p-4">
      <div className="mb-3 flex items-center justify-between gap-2">
        <span className="truncate text-[11px] text-[#eee]">{a.name}</span>
        <span className="flex items-center gap-1.5 text-[10px]" style={{ color: statusColor }}>
          <span className="h-[5px] w-[5px] rounded-full" style={{ background: statusColor }} />
          {a.status.toLowerCase()}
        </span>
      </div>

      {a.capabilities.length > 0 && (
        <div className="mb-3 flex flex-wrap gap-1">
          {a.capabilities.map(c => (
            <span key={c} className="rounded bg-[#1a1a1a] px-1.5 py-px text-[9px] text-[#555]">{c}</span>
          ))}
        </div>
      )}

      <div className="grid grid-cols-3 gap-2 border-t border-[#1a1a1a] pt-3">
        <Stat label="tasks"   value={String(a.tasks_handled)} />
        <Stat label="success" value={success !== null ? `${success}%` : '—'} />
        <Stat label="avg ms"  value={a.avg_latency_ms > 0 ? String(a.avg_latency_ms) : '—'} />
      </div>

      {(a.lifecycle_state || a.provider_type) && (
        <div className="mt-2.5 flex items-center gap-2 text-[9px] text-[#333]">
          {a.lifecycle_state && <span>{a.lifecycle_state.toLowerCase()}</span>}
          {a.lifecycle_state && a.provider_type && <span>·</span>}
          {a.provider_type && <span>{a.provider_type}  {a.model_name}</span>}
          {a.last_heartbeat_secs_ago != null && (
            <span className="ml-auto">hb {a.last_heartbeat_secs_ago}s</span>
          )}
        </div>
      )}
    </div>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <p className="text-[9px] text-[#333] uppercase tracking-widest">{label}</p>
      <p className="mt-0.5 text-[11px] text-[#ccc] tabular-nums">{value}</p>
    </div>
  );
}

function Empty({ text }: { text: string }) {
  return (
    <div className="flex h-48 items-center justify-center">
      <p className="text-[11px] text-[#2a2a2a]">{text}</p>
    </div>
  );
}

function Skeleton() {
  return (
    <div className="p-5">
      <div className="mb-5 h-4 w-20 rounded bg-[#1a1a1a] animate-pulse" />
      <div className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
        {Array.from({ length: 6 }).map((_, i) => (
          <div key={i} className="h-36 rounded border border-[#1a1a1a] bg-[#111] animate-pulse" />
        ))}
      </div>
    </div>
  );
}
