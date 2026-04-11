import { useState, useEffect } from 'react';
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
        <h1 className="text-sm font-medium text-white">Agents</h1>
        <span className="font-mono text-xs text-[#3a3a3a] tabular-nums">{agents.length}</span>
      </div>

      {agents.length === 0 ? (
        <div className="flex flex-col items-center justify-center py-24">
          <p className="text-sm text-[#3a3a3a]">No agents registered</p>
        </div>
      ) : (
        <div className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
          {agents.map((agent) => <AgentCard key={agent.name} agent={agent} />)}
        </div>
      )}
    </div>
  );
}

const STATUS_COLORS: Record<string, string> = {
  idle: '#22c55e',
  busy: '#f59e0b',
  err:  '#ef4444',
};

function AgentCard({ agent }: { agent: AgentInfo }) {
  const statusColor = STATUS_COLORS[agent.status.toLowerCase()] ?? '#6b6b6b';
  const successRate = agent.tasks_handled > 0
    ? Math.round((agent.tasks_succeeded / agent.tasks_handled) * 100)
    : null;

  return (
    <div className="rounded border border-[#1c1c1c] bg-[#0c0c0c] p-4">
      <div className="mb-3 flex items-center justify-between gap-2">
        <h3 className="truncate text-sm font-medium text-white">{agent.name}</h3>
        <span className="flex items-center gap-1.5 text-[10px] font-medium" style={{ color: statusColor }}>
          <span className="h-[5px] w-[5px] rounded-full shrink-0" style={{ backgroundColor: statusColor }} />
          {agent.status.toUpperCase()}
        </span>
      </div>

      {agent.capabilities.length > 0 && (
        <div className="mb-3 flex flex-wrap gap-1.5">
          {agent.capabilities.map((cap) => (
            <span key={cap} className="rounded border border-[#1c1c1c] px-2 py-0.5 font-mono text-[10px] text-[#6b6b6b]">{cap}</span>
          ))}
        </div>
      )}

      <div className="grid grid-cols-3 gap-3 border-t border-[#1c1c1c] pt-3">
        <Stat label="Tasks" value={String(agent.tasks_handled)} />
        <Stat label="Success" value={successRate !== null ? `${successRate}%` : '—'} />
        <Stat label="Avg ms" value={agent.avg_latency_ms > 0 ? String(agent.avg_latency_ms) : '—'} />
      </div>

      {agent.lifecycle_state && (
        <div className="mt-2 flex items-center gap-2">
          <LifecycleBadge state={agent.lifecycle_state} />
          {agent.last_heartbeat_secs_ago != null && (
            <span className="text-[10px] text-[#3a3a3a]">hb {agent.last_heartbeat_secs_ago}s</span>
          )}
        </div>
      )}
      {agent.provider_type && (
        <p className="mt-1 text-[10px] text-[#3a3a3a]">{agent.provider_type} · {agent.model_name}</p>
      )}
    </div>
  );
}

const LIFECYCLE_COLORS: Record<string, string> = {
  Running: '#22c55e',
  Paused:  '#f59e0b',
  Stopped: '#ef4444',
  Created: '#3a3a3a',
};

function LifecycleBadge({ state }: { state: string }) {
  const color = LIFECYCLE_COLORS[state] ?? '#3a3a3a';
  return (
    <span className="text-[10px] font-medium" style={{ color }}>{state}</span>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <p className="text-[10px] uppercase tracking-widest text-[#3a3a3a]">{label}</p>
      <p className="mt-0.5 font-mono text-xs text-white">{value}</p>
    </div>
  );
}

function LoadingSkeleton() {
  return (
    <div className="p-6">
      <div className="mb-6 h-5 w-20 animate-pulse rounded bg-[#141414]" />
      <div className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
        {Array.from({ length: 6 }).map((_, i) => (
          <div key={i} className="h-40 animate-pulse rounded border border-[#1c1c1c] bg-[#0c0c0c]" />
        ))}
      </div>
    </div>
  );
}
