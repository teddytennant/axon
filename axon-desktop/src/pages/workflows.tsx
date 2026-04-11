import { useState, useEffect } from 'react';
import { ChevronDown, ChevronRight } from 'lucide-react';
import { useWebSocket } from '../hooks/use-websocket';
import type { WorkflowSnapshot, WorkflowsResponse } from '../lib/types';

const STATUS_CONFIG: Record<string, { label: string; color: string; bg: string }> = {
  Running:   { label: 'RUNNING',   color: '#c8c8c8', bg: 'rgba(200,200,200,0.06)' },
  Completed: { label: 'DONE',      color: '#22c55e', bg: 'rgba(34,197,94,0.07)'  },
  Failed:    { label: 'FAILED',    color: '#ef4444', bg: 'rgba(239,68,68,0.07)'  },
};

const STEP_ICON: Record<string, string> = {
  Completed: '✓',
  Failed:    '✗',
  Running:   '▸',
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
    <div className="h-full overflow-auto p-6">
      <div className="mb-6 flex items-baseline gap-4">
        <span className="text-[11px] font-medium tracking-wider text-[#555]">workflows</span>
        {data.active.length > 0 && (
          <span className="text-[10px] text-[#c8c8c8] tabular-nums">{data.active.length} active</span>
        )}
        {data.completed.length > 0 && (
          <span className="text-[10px] tabular-nums text-[#2e2e2e]">{data.completed.length} done</span>
        )}
      </div>

      {all.length === 0 ? (
        <div className="flex h-40 items-center justify-center">
          <p className="text-[11px] text-[#1e1e1e]">no workflows running</p>
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
  const st       = STATUS_CONFIG[wf.status] ?? { label: wf.status, color: '#484848', bg: 'transparent' };
  const progress = wf.steps_total > 0 ? wf.steps_completed / wf.steps_total : 0;

  return (
    <div className="overflow-hidden rounded-lg border border-[#1c1c1c] bg-[#080808] hover:border-[#252525] transition-colors">
      <button
        className="flex w-full items-center gap-3 px-4 py-2.5 text-left transition-colors hover:bg-[#0c0c0c]"
        onClick={onToggle}
      >
        {/* Expand icon */}
        <span className="shrink-0 text-[#282828]">
          {expanded
            ? <ChevronDown size={11} strokeWidth={2} />
            : <ChevronRight size={11} strokeWidth={2} />
          }
        </span>

        {/* ID */}
        <span className="shrink-0 font-mono text-[9px] text-[#2e2e2e] tabular-nums">
          {wf.id.slice(0, 8)}
        </span>

        {/* Pattern badge */}
        <span className="shrink-0 rounded border border-[#1c1c1c] bg-[#0e0e0e] px-1.5 py-px font-mono text-[9px] text-[#444]">
          {wf.pattern}
        </span>

        {/* Progress bar */}
        <div className="flex min-w-0 flex-1 items-center gap-2.5">
          <div className="relative h-[1px] flex-1 bg-[#181818]">
            <div
              className="absolute inset-y-0 left-0 transition-all duration-500"
              style={{ width: `${progress * 100}%`, backgroundColor: st.color, opacity: 0.7 }}
            />
          </div>
          <span className="shrink-0 font-mono text-[9px] tabular-nums text-[#2e2e2e]">
            {wf.steps_completed}/{wf.steps_total}
          </span>
        </div>

        {/* Status badge */}
        <span
          className="shrink-0 rounded-full px-2 py-px text-[8px] font-semibold tracking-wider"
          style={{ color: st.color, background: st.bg }}
        >
          {st.label}
        </span>

        {/* Duration */}
        {wf.duration_ms > 0 && (
          <span className="shrink-0 font-mono text-[9px] tabular-nums text-[#2e2e2e]">
            {wf.duration_ms < 1000 ? `${wf.duration_ms}ms` : `${(wf.duration_ms / 1000).toFixed(1)}s`}
          </span>
        )}
      </button>

      {expanded && wf.steps.length > 0 && (
        <div className="border-t border-[#141414] bg-[#050505] px-4 py-2.5">
          <div className="space-y-1.5">
            {wf.steps.map((step, i) => {
              const icon  = STEP_ICON[step.status] ?? '○';
              const color = step.status === 'Completed' ? '#22c55e'
                : step.status === 'Failed'    ? '#ef4444'
                : '#888';
              return (
                <div key={i} className="flex items-center gap-3 font-mono text-[10px]">
                  <span className="shrink-0 w-3 text-center" style={{ color }}>{icon}</span>
                  <span className="flex-1 text-[#7a7a7a]">{step.capability}</span>
                  <span className="text-[#2e2e2e] tabular-nums">{step.latency_ms}ms</span>
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}
