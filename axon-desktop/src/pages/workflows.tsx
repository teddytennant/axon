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
    <div className="h-full overflow-auto p-5">
      <div className="mb-5 flex items-center gap-3">
        <span className="text-[11px] text-[#666]">workflows</span>
        <span className="text-[10px] text-[#fff] tabular-nums">{data.active.length} active</span>
        <span className="text-[10px] text-[#333] tabular-nums">{data.completed.length} done</span>
      </div>

      {all.length === 0 ? (
        <Empty text="no workflows running" />
      ) : (
        <div className="space-y-1">
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

function Empty({ text }: { text: string }) {
  return (
    <div className="flex h-40 items-center justify-center">
      <p className="text-[10px] text-[#2a2a2a]">{text}</p>
    </div>
  );
}

function WorkflowRow({ wf, expanded, onToggle }: {
  wf: WorkflowSnapshot;
  expanded: boolean;
  onToggle: () => void;
}) {
  const st = STATUS_CONFIG[wf.status] ?? { label: wf.status, color: '#555' };
  const progress = wf.steps_total > 0 ? wf.steps_completed / wf.steps_total : 0;

  return (
    <div className="rounded border border-[#1c1c1c] bg-[#0c0c0c]">
      <button
        className="flex w-full items-center gap-2.5 px-3 py-2 text-left hover:bg-[#141414] transition-colors"
        onClick={onToggle}
      >
        <span className="text-[#333] shrink-0">
          {expanded ? <ChevronDown size={11} /> : <ChevronRight size={11} />}
        </span>

        <span className="font-mono text-[9px] text-[#333] shrink-0">
          {wf.id.slice(0, 12)}…
        </span>

        <span className="rounded border border-[#1c1c1c] px-1.5 py-0.5 font-mono text-[9px] text-[#555] shrink-0">
          {wf.pattern}
        </span>

        <div className="flex flex-1 items-center gap-2 min-w-0">
          <div className="h-px flex-1 bg-[#1c1c1c]">
            <div
              className="h-px transition-all"
              style={{ width: `${progress * 100}%`, backgroundColor: st.color }}
            />
          </div>
          <span className="font-mono text-[9px] text-[#333] shrink-0 tabular-nums">
            {wf.steps_completed}/{wf.steps_total}
          </span>
        </div>

        <span className="text-[9px] font-medium shrink-0" style={{ color: st.color }}>
          {st.label}
        </span>

        {wf.duration_ms > 0 && (
          <span className="font-mono text-[9px] text-[#333] shrink-0 tabular-nums">{wf.duration_ms}ms</span>
        )}
      </button>

      {expanded && wf.steps.length > 0 && (
        <div className="border-t border-[#1c1c1c] px-3 py-2">
          <div className="space-y-1">
            {wf.steps.map((step, i) => {
              const icon = STEP_ICON[step.status] ?? '○';
              const color = step.status === 'Completed' ? '#22c55e'
                : step.status === 'Failed' ? '#ef4444'
                : '#f59e0b';
              return (
                <div key={i} className="flex items-center gap-2.5 font-mono text-[10px]">
                  <span style={{ color }}>{icon}</span>
                  <span className="flex-1 text-[#aaa]">{step.capability}</span>
                  <span className="text-[#333] tabular-nums">{step.latency_ms}ms</span>
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}
