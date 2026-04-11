import { useState, useEffect } from 'react';
import { ChevronDown, ChevronRight } from 'lucide-react';
import { useWebSocket } from '../hooks/use-websocket';
import type { WorkflowSnapshot, WorkflowsResponse } from '../lib/types';

const STATUS_CONFIG: Record<string, { label: string; color: string }> = {
  Running:   { label: 'RUNNING',   color: '#ffffff' },
  Completed: { label: 'DONE',      color: '#22c55e' },
  Failed:    { label: 'FAILED',    color: '#ef4444' },
};

const STEP_ICON: Record<string, string> = {
  Completed: '✔',
  Failed: '✘',
  Running: '▶',
};

export default function WorkflowsPage() {
  const { subscribe } = useWebSocket();
  const [data, setData] = useState<WorkflowsResponse>({ active: [], completed: [] });
  const [expanded, setExpanded] = useState<Set<string>>(new Set());

  useEffect(() => {
    return subscribe('workflows', (d) => setData(d as WorkflowsResponse));
  }, [subscribe]);

  const all = [...data.active, ...data.completed];

  function toggle(id: string) {
    setExpanded(prev => {
      const next = new Set(prev);
      next.has(id) ? next.delete(id) : next.add(id);
      return next;
    });
  }

  return (
    <div className="p-6">
      <div className="mb-6 flex items-center gap-3">
        <h1 className="text-sm font-medium text-white">Workflows</h1>
        <span className="font-mono text-xs text-white tabular-nums">{data.active.length} active</span>
        <span className="font-mono text-xs text-[#3a3a3a] tabular-nums">{data.completed.length} done</span>
      </div>

      {all.length === 0 ? (
        <div className="flex flex-col items-center justify-center py-24">
          <p className="text-sm text-[#3a3a3a]">No workflows running</p>
          <p className="mt-1 font-mono text-xs text-[#2a2a2a]">
            pipeline(), fan_out(), delegate(), swarm_dispatch()
          </p>
        </div>
      ) : (
        <div className="space-y-1.5">
          {all.map(wf => (
            <WorkflowRow
              key={wf.id}
              wf={wf}
              expanded={expanded.has(wf.id)}
              onToggle={() => toggle(wf.id)}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function WorkflowRow({ wf, expanded, onToggle }: {
  wf: WorkflowSnapshot;
  expanded: boolean;
  onToggle: () => void;
}) {
  const st = STATUS_CONFIG[wf.status] ?? { label: wf.status, color: '#6b6b6b' };
  const progress = wf.steps_total > 0 ? wf.steps_completed / wf.steps_total : 0;

  return (
    <div className="rounded border border-[#1c1c1c] bg-[#0c0c0c]">
      <button
        className="flex w-full items-center gap-3 px-4 py-3 text-left hover:bg-[#141414] transition-colors"
        onClick={onToggle}
      >
        <span className="text-[#3a3a3a] shrink-0">
          {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        </span>

        <span className="font-mono text-[10px] text-[#3a3a3a] shrink-0">
          {wf.id.slice(0, 14)}…
        </span>

        <span className="rounded border border-[#1c1c1c] px-2 py-0.5 font-mono text-[10px] text-[#6b6b6b] shrink-0">
          {wf.pattern}
        </span>

        <div className="flex flex-1 items-center gap-2 min-w-0">
          <div className="h-px flex-1 overflow-hidden bg-[#1c1c1c]">
            <div
              className="h-full transition-all"
              style={{ width: `${progress * 100}%`, backgroundColor: st.color }}
            />
          </div>
          <span className="font-mono text-[10px] text-[#3a3a3a] shrink-0 tabular-nums">
            {wf.steps_completed}/{wf.steps_total}
          </span>
        </div>

        <span
          className="text-[10px] font-medium shrink-0"
          style={{ color: st.color }}
        >
          {st.label}
        </span>

        {wf.duration_ms > 0 && (
          <span className="font-mono text-[10px] text-[#3a3a3a] shrink-0 tabular-nums">{wf.duration_ms}ms</span>
        )}

        <span className="text-[10px] text-[#2a2a2a] shrink-0">{wf.started_at}</span>
      </button>

      {expanded && wf.steps.length > 0 && (
        <div className="border-t border-[#1c1c1c] px-4 py-3">
          <div className="space-y-1.5">
            {wf.steps.map((step, i) => {
              const icon = STEP_ICON[step.status] ?? '○';
              const color = step.status === 'Completed' ? '#22c55e'
                : step.status === 'Failed' ? '#ef4444'
                : '#f59e0b';
              return (
                <div key={i} className="flex items-center gap-3 font-mono text-xs">
                  <span style={{ color }}>{icon}</span>
                  <span className="flex-1 text-[#aaaaaa]">{step.capability}</span>
                  <span className="text-[#3a3a3a] tabular-nums">{step.latency_ms}ms</span>
                  <span className="text-[#2a2a2a] tabular-nums">{step.payload_bytes}B</span>
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}
