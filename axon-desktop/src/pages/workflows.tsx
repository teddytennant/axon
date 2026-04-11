import { useState, useEffect } from 'react';
import { GitBranch, ChevronDown, ChevronRight } from 'lucide-react';
import { useWebSocket } from '../hooks/use-websocket';
import type { WorkflowSnapshot, WorkflowsResponse } from '../lib/types';

const STATUS_CONFIG: Record<string, { label: string; color: string; bg: string }> = {
  Running:   { label: 'RUNNING',   color: '#50dc78', bg: 'bg-[#50dc78]/10' },
  Completed: { label: 'DONE',      color: '#00c8c8', bg: 'bg-[#00c8c8]/10' },
  Failed:    { label: 'FAILED',    color: '#f05050', bg: 'bg-[#f05050]/10' },
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
        <h1 className="text-lg font-semibold text-[#f5f5f5]">Workflows</h1>
        <span className="rounded-full bg-[#00c8c8]/10 px-2.5 py-0.5 font-mono text-xs text-[#00c8c8]">
          {data.active.length} active
        </span>
        <span className="rounded-full bg-[#333]/60 px-2.5 py-0.5 font-mono text-xs text-[#555]">
          {data.completed.length} completed
        </span>
      </div>

      {all.length === 0 ? (
        <div className="flex flex-col items-center justify-center py-24">
          <GitBranch size={32} className="mb-3 text-[#555]" />
          <p className="text-sm text-[#555]">No workflows running</p>
          <p className="mt-1 font-mono text-xs text-[#444]">
            Use pipeline(), fan_out(), delegate(), or swarm_dispatch()
          </p>
        </div>
      ) : (
        <div className="space-y-2">
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
  const st = STATUS_CONFIG[wf.status] ?? { label: wf.status, color: '#555', bg: 'bg-[#555]/10' };
  const progress = wf.steps_total > 0 ? wf.steps_completed / wf.steps_total : 0;

  return (
    <div className="rounded-lg border border-[#222] bg-[#111]">
      <button
        className="flex w-full items-center gap-3 px-4 py-3 text-left"
        onClick={onToggle}
      >
        <span className="text-[#555]">
          {expanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
        </span>

        <span className="font-mono text-[10px] text-[#444]">
          {wf.id.slice(0, 14)}…
        </span>

        <span className="rounded bg-[#181818] px-2 py-0.5 font-mono text-[10px] text-[#888]">
          {wf.pattern}
        </span>

        <div className="flex flex-1 items-center gap-2">
          <div className="h-1 flex-1 overflow-hidden rounded-full bg-[#181818]">
            <div
              className="h-full rounded-full transition-all"
              style={{ width: `${progress * 100}%`, backgroundColor: st.color }}
            />
          </div>
          <span className="font-mono text-[10px] text-[#555]">
            {wf.steps_completed}/{wf.steps_total}
          </span>
        </div>

        <span
          className={`rounded px-2 py-0.5 text-[10px] font-medium ${st.bg}`}
          style={{ color: st.color }}
        >
          {st.label}
        </span>

        {wf.duration_ms > 0 && (
          <span className="font-mono text-[10px] text-[#555]">{wf.duration_ms}ms</span>
        )}

        <span className="text-[10px] text-[#444]">{wf.started_at}</span>
      </button>

      {expanded && wf.steps.length > 0 && (
        <div className="border-t border-[#222] px-4 py-3">
          <div className="space-y-1.5">
            {wf.steps.map((step, i) => {
              const icon = STEP_ICON[step.status] ?? '○';
              const color = step.status === 'Completed' ? '#50dc78'
                : step.status === 'Failed' ? '#f05050'
                : '#f0c83c';
              return (
                <div key={i} className="flex items-center gap-3 font-mono text-xs">
                  <span style={{ color }}>{icon}</span>
                  <span className="flex-1 text-[#ccc]">{step.capability}</span>
                  <span className="text-[#555]">{step.latency_ms}ms</span>
                  <span className="text-[#444]">{step.payload_bytes}B</span>
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}
