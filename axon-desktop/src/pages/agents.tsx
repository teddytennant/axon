import { useState, useEffect } from 'react';
import { clsx } from 'clsx';
import { Bot } from 'lucide-react';
import { useAgents } from '../hooks/use-api';
import { useWebSocket } from '../hooks/use-websocket';
import type { AgentInfo } from '../lib/types';

export default function AgentsPage() {
  const { data: initialAgents, isLoading } = useAgents();
  const { subscribe } = useWebSocket();
  const [agents, setAgents] = useState<AgentInfo[]>([]);

  useEffect(() => { if (initialAgents) setAgents(initialAgents); }, [initialAgents]);

  useEffect(() => {
    return subscribe('agents', (data) => {
      setAgents(data as AgentInfo[]);
    });
  }, [subscribe]);

  if (isLoading) return <LoadingSkeleton />;

  return (
    <div className="p-6">
      <div className="mb-6 flex items-center gap-3">
        <h1 className="text-lg font-semibold text-[#f5f5f5]">Agents</h1>
        <span className="rounded-full bg-[#00c8c8]/10 px-2.5 py-0.5 font-mono text-xs text-[#00c8c8]">
          {agents.length}
        </span>
      </div>

      {agents.length === 0 ? (
        <div className="flex flex-col items-center justify-center py-24">
          <Bot size={32} className="mb-3 text-[#555]" />
          <p className="text-sm text-[#555]">No agents registered</p>
        </div>
      ) : (
        <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
          {agents.map((agent) => <AgentCard key={agent.name} agent={agent} />)}
        </div>
      )}
    </div>
  );
}

function AgentCard({ agent }: { agent: AgentInfo }) {
  const statusKey = agent.status.toLowerCase();
  const statusConfig: Record<string, { label: string; color: string; bg: string }> = {
    idle: { label: 'IDLE', color: '#50dc78', bg: 'bg-[#50dc78]/10' },
    busy: { label: 'BUSY', color: '#f0c83c', bg: 'bg-[#f0c83c]/10' },
    err: { label: 'ERR', color: '#f05050', bg: 'bg-[#f05050]/10' },
  };
  const st = statusConfig[statusKey] ?? statusConfig.idle;
  const successRate = agent.tasks_handled > 0
    ? Math.round((agent.tasks_succeeded / agent.tasks_handled) * 100)
    : null;

  return (
    <div className="rounded-lg border border-[#222] bg-[#111] p-4">
      <div className="mb-3 flex items-center justify-between">
        <h3 className="truncate text-sm font-medium text-[#f5f5f5]">{agent.name}</h3>
        <span className={clsx('flex items-center gap-1.5 rounded px-2 py-0.5 text-[10px] font-medium', st.bg)} style={{ color: st.color }}>
          <span className="h-1.5 w-1.5 rounded-full" style={{ backgroundColor: st.color }} />
          {st.label}
        </span>
      </div>

      {agent.capabilities.length > 0 && (
        <div className="mb-3 flex flex-wrap gap-1.5">
          {agent.capabilities.map((cap) => (
            <span key={cap} className="rounded bg-[#181818] px-2 py-0.5 font-mono text-[10px] text-[#888]">{cap}</span>
          ))}
        </div>
      )}

      <div className="grid grid-cols-3 gap-3 border-t border-[#222] pt-3">
        <Stat label="Tasks" value={String(agent.tasks_handled)} />
        <Stat label="Success" value={successRate !== null ? `${successRate}%` : '—'} />
        <Stat label="Avg ms" value={agent.avg_latency_ms > 0 ? String(agent.avg_latency_ms) : '—'} />
      </div>

      {agent.lifecycle_state && (
        <div className="mt-2 flex items-center gap-2">
          <LifecycleBadge state={agent.lifecycle_state} />
          {agent.last_heartbeat_secs_ago != null && (
            <span className="text-[10px] text-[#444]">hb: {agent.last_heartbeat_secs_ago}s ago</span>
          )}
        </div>
      )}
      {agent.provider_type && (
        <p className="mt-1 text-[10px] text-[#555]">{agent.provider_type} · {agent.model_name}</p>
      )}
    </div>
  );
}

function LifecycleBadge({ state }: { state: string }) {
  const config: Record<string, { color: string; bg: string }> = {
    Running:  { color: '#50dc78', bg: 'bg-[#50dc78]/10' },
    Paused:   { color: '#f0c83c', bg: 'bg-[#f0c83c]/10' },
    Stopped:  { color: '#f05050', bg: 'bg-[#f05050]/10' },
    Created:  { color: '#555',    bg: 'bg-[#555]/10' },
  };
  const c = config[state] ?? config.Created;
  return (
    <span className={`rounded px-1.5 py-0.5 text-[10px] font-medium ${c.bg}`} style={{ color: c.color }}>
      {state}
    </span>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <p className="text-[10px] uppercase tracking-widest text-[#555]">{label}</p>
      <p className="mt-0.5 text-xs text-[#f5f5f5]">{value}</p>
    </div>
  );
}

function LoadingSkeleton() {
  return (
    <div className="p-6">
      <div className="mb-6 h-6 w-24 animate-pulse rounded bg-[#181818]" />
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
        {Array.from({ length: 6 }).map((_, i) => (
          <div key={i} className="h-44 animate-pulse rounded-lg border border-[#222] bg-[#111]" />
        ))}
      </div>
    </div>
  );
}
