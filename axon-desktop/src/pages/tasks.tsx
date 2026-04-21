import { useState, useEffect } from 'react';
import { clsx } from 'clsx';
import { useTaskLog, useTaskStats } from '../hooks/use-api';
import { useWebSocket } from '../hooks/use-websocket';
import { EmptyState, GridSkeleton, PageHeader } from '../components/page';
import type { TaskLogEntry, TaskStatsResponse } from '../lib/types';

export default function TasksPage() {
  const { data: initTasks, isLoading: tl } = useTaskLog();
  const { data: stats, isLoading: sl }     = useTaskStats();
  const { subscribe }                       = useWebSocket();
  const [tasks, setTasks]                   = useState<TaskLogEntry[]>([]);

  useEffect(() => { if (initTasks) setTasks(initTasks); }, [initTasks]);
  useEffect(() => subscribe('tasks', d => setTasks(d.recent)), [subscribe]);

  if (tl || sl) return <GridSkeleton statsCount={5} cards={null} />;

  return (
    <div className="h-full overflow-auto p-6">
      <PageHeader label="tasks" />

      {stats && <StatsRow stats={stats} />}

      {tasks.length === 0 ? (
        <EmptyState text="no tasks recorded" />
      ) : (
        <div className="overflow-hidden rounded-lg border border-[#1e1e1e]">
          <table className="w-full text-left">
            <thead>
              <tr className="border-b border-[#181818] bg-[#080808]">
                {['id', 'capability', 'peer', 'status', 'duration'].map(h => (
                  <th key={h} className="px-4 py-2.5 text-[8px] font-medium uppercase tracking-[0.15em] text-[#2e2e2e]">{h}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {tasks.map(t => <TaskRow key={t.id} task={t} />)}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

function StatsRow({ stats: s }: { stats: TaskStatsResponse }) {
  const items = [
    { label: 'pending',   value: s.pending,   color: '#484848' },
    { label: 'running',   value: s.running,   color: '#c8c8c8' },
    { label: 'completed', value: s.completed, color: '#22c55e' },
    { label: 'failed',    value: s.failed,    color: '#ef4444' },
    { label: 'total',     value: s.total,     color: '#888'    },
  ];
  return (
    <div className="mb-6 grid grid-cols-5 gap-2">
      {items.map(({ label, value, color }) => (
        <div key={label} className="rounded-lg border border-[#1e1e1e] bg-[#080808] p-3.5">
          <p className="text-[8px] uppercase tracking-[0.15em] text-[#2e2e2e]">{label}</p>
          <p className="mt-1.5 text-[22px] font-light tabular-nums leading-none" style={{ color }}>{value}</p>
        </div>
      ))}
    </div>
  );
}

const STATUS_CONFIG: Record<string, { color: string; bg: string }> = {
  pending:   { color: '#484848', bg: 'transparent' },
  running:   { color: '#c8c8c8', bg: 'transparent' },
  completed: { color: '#22c55e', bg: 'rgba(34,197,94,0.07)' },
  failed:    { color: '#ef4444', bg: 'rgba(239,68,68,0.07)' },
  cancelled: { color: '#f59e0b', bg: 'rgba(245,158,11,0.07)' },
};

function TaskRow({ task: t }: { task: TaskLogEntry }) {
  const cfg = STATUS_CONFIG[t.status] ?? { color: '#484848', bg: 'transparent' };
  const dur  = t.duration_ms > 0
    ? t.duration_ms < 1000 ? `${t.duration_ms}ms` : `${(t.duration_ms / 1000).toFixed(1)}s`
    : '—';

  return (
    <tr className="border-b border-[#111] last:border-0 hover:bg-[#0a0a0a] transition-colors">
      <td className="px-4 py-2.5 text-[10px] text-[#383838] tabular-nums font-mono" title={t.id}>
        {t.id.slice(0, 8)}
      </td>
      <td className="px-4 py-2.5 text-[11px] text-[#b5b5b5]">{t.capability}</td>
      <td className="max-w-[160px] truncate px-4 py-2.5 text-[10px] text-[#383838] font-mono" title={t.peer}>
        {t.peer ? t.peer.slice(0, 14) + '…' : '—'}
      </td>
      <td className="px-4 py-2.5">
        <span
          className={clsx('inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 text-[9px] font-medium tracking-wide')}
          style={{ color: cfg.color, background: cfg.bg }}
        >
          <span className="h-[3.5px] w-[3.5px] rounded-full" style={{ background: cfg.color }} />
          {t.status}
        </span>
      </td>
      <td className="px-4 py-2.5 text-[10px] text-[#383838] tabular-nums font-mono">{dur}</td>
    </tr>
  );
}

