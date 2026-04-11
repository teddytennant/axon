import { useState, useEffect } from 'react';
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
    <div className="h-full overflow-auto p-6">
      <PageHeader label="agents" count={agents.length} />

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

const STATUS_COLOR: Record<string, string> = {
  idle:  '#22c55e',
  busy:  '#f59e0b',
  err:   '#ef4444',
};

const STATUS_BG: Record<string, string> = {
  idle:  'rgba(34,197,94,0.08)',
  busy:  'rgba(245,158,11,0.08)',
  err:   'rgba(239,68,68,0.08)',
};

function AgentCard({ agent: a }: { agent: AgentInfo }) {
  const key          = a.status.toLowerCase();
  const statusColor  = STATUS_COLOR[key] ?? '#444';
  const statusBg     = STATUS_BG[key]    ?? 'transparent';
  const success      = a.tasks_handled > 0
    ? Math.round((a.tasks_succeeded / a.tasks_handled) * 100)
    : null;

  return (
    <div className="flex flex-col rounded-lg border border-[#1e1e1e] bg-[#080808] overflow-hidden hover:border-[#282828] transition-colors">
      {/* Header */}
      <div className="flex items-center justify-between gap-2 border-b border-[#141414] px-4 py-3">
        <span className="truncate text-[12px] font-medium text-[#e2e2e2]">{a.name}</span>
        <span
          className="flex shrink-0 items-center gap-1.5 rounded-full px-2 py-0.5 text-[9px] font-medium tracking-wider"
          style={{ color: statusColor, background: statusBg }}
        >
          <span className="h-[4px] w-[4px] rounded-full" style={{ background: statusColor }} />
          {a.status.toLowerCase()}
        </span>
      </div>

      {/* Capabilities */}
      {a.capabilities.length > 0 && (
        <div className="flex flex-wrap gap-1 px-4 pt-3">
          {a.capabilities.map(c => (
            <span key={c} className="rounded-md border border-[#1a1a1a] bg-[#0e0e0e] px-1.5 py-px text-[9px] text-[#484848]">
              {c}
            </span>
          ))}
        </div>
      )}

      {/* Stats */}
      <div className="mt-auto grid grid-cols-3 gap-px border-t border-[#141414] pt-0 mt-3">
        <StatCell label="tasks"   value={String(a.tasks_handled)} />
        <StatCell label="success" value={success !== null ? `${success}%` : '—'} />
        <StatCell label="avg ms"  value={a.avg_latency_ms > 0 ? String(a.avg_latency_ms) : '—'} />
      </div>

      {/* Footer metadata */}
      {(a.lifecycle_state || a.provider_type) && (
        <div className="flex items-center gap-2 border-t border-[#0f0f0f] px-4 py-2 text-[9px] text-[#2e2e2e]">
          {a.lifecycle_state && <span className="tracking-wider">{a.lifecycle_state.toLowerCase()}</span>}
          {a.lifecycle_state && a.provider_type && <span className="text-[#1a1a1a]">·</span>}
          {a.provider_type && (
            <span className="tracking-wider">{a.provider_type}{a.model_name ? ` / ${a.model_name}` : ''}</span>
          )}
          {a.last_heartbeat_secs_ago != null && (
            <span className="ml-auto tabular-nums">hb {a.last_heartbeat_secs_ago}s</span>
          )}
        </div>
      )}
    </div>
  );
}

function StatCell({ label, value }: { label: string; value: string }) {
  return (
    <div className="px-4 py-3">
      <p className="text-[8px] uppercase tracking-[0.15em] text-[#2e2e2e]">{label}</p>
      <p className="mt-0.5 text-[14px] font-light tabular-nums text-[#999]">{value}</p>
    </div>
  );
}

function PageHeader({ label, count }: { label: string; count: number }) {
  return (
    <div className="mb-6 flex items-baseline gap-3">
      <span className="text-[11px] font-medium tracking-wider text-[#555]">{label}</span>
      <span className="text-[10px] tabular-nums text-[#2e2e2e]">{count}</span>
    </div>
  );
}

function Empty({ text }: { text: string }) {
  return (
    <div className="flex h-48 items-center justify-center">
      <p className="text-[11px] text-[#1e1e1e]">{text}</p>
    </div>
  );
}

function Skeleton() {
  return (
    <div className="p-6">
      <div className="mb-6 h-4 w-20 rounded animate-shimmer" />
      <div className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
        {Array.from({ length: 6 }).map((_, i) => (
          <div key={i} className="h-36 rounded-lg border border-[#141414] animate-shimmer" />
        ))}
      </div>
    </div>
  );
}
