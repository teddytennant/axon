import { useState, useEffect } from 'react';
import { clsx } from 'clsx';
import { useTaskLog, useTaskStats } from '../hooks/use-api';
import { useWebSocket } from '../hooks/use-websocket';
import type { TaskLogEntry, TaskStatsResponse, WsTasksData } from '../lib/types';

export default function TasksPage() {
  const { data: initTasks, isLoading: tl } = useTaskLog();
  const { data: stats, isLoading: sl }     = useTaskStats();
  const { subscribe }                       = useWebSocket();
  const [tasks, setTasks]                   = useState<TaskLogEntry[]>([]);

  useEffect(() => { if (initTasks) setTasks(initTasks); }, [initTasks]);
  useEffect(() => subscribe('tasks', d => setTasks((d as WsTasksData).recent)), [subscribe]);

  if (tl || sl) return <Skeleton />;

  return (
    <div className="h-full overflow-auto p-5">
      <div className="mb-5">
        <span className="text-[11px] text-[#666]">tasks</span>
      </div>

      {stats && <StatsRow stats={stats} />}

      {tasks.length === 0 ? (
        <div className="flex h-48 items-center justify-center">
          <p className="text-[11px] text-[#2a2a2a]">no tasks recorded</p>
        </div>
      ) : (
        <div className="rounded border border-[#1f1f1f] overflow-hidden">
          <table className="w-full text-left">
            <thead>
              <tr className="border-b border-[#1a1a1a] bg-[#111]">
                {['id', 'capability', 'peer', 'status', 'duration'].map(h => (
                  <th key={h} className="px-3 py-2.5 text-[9px] uppercase tracking-widest text-[#333]">{h}</th>
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
    { label: 'pending',   value: s.pending,   color: '#555' },
    { label: 'running',   value: s.running,   color: '#eee' },
    { label: 'completed', value: s.completed, color: '#22c55e' },
    { label: 'failed',    value: s.failed,    color: '#ef4444' },
    { label: 'total',     value: s.total,     color: '#eee' },
  ];
  return (
    <div className="mb-5 grid grid-cols-5 gap-2">
      {items.map(({ label, value, color }) => (
        <div key={label} className="rounded border border-[#1f1f1f] bg-[#111] p-3">
          <p className="text-[9px] uppercase tracking-widest text-[#333]">{label}</p>
          <p className="mt-1 text-lg tabular-nums" style={{ color }}>{value}</p>
        </div>
      ))}
    </div>
  );
}

const STATUS_COLOR: Record<string, string> = {
  pending: '#555', running: '#eee', completed: '#22c55e', failed: '#ef4444', cancelled: '#f59e0b',
};

function TaskRow({ task: t }: { task: TaskLogEntry }) {
  const color = STATUS_COLOR[t.status] ?? '#555';
  const dur   = t.duration_ms > 0
    ? t.duration_ms < 1000 ? `${t.duration_ms}ms` : `${(t.duration_ms / 1000).toFixed(1)}s`
    : '—';

  return (
    <tr className={clsx('border-b border-[#1a1a1a] last:border-0 hover:bg-[#141414] transition-colors')}>
      <td className="px-3 py-2.5 text-[10px] text-[#444] tabular-nums" title={t.id}>{t.id.slice(0, 8)}</td>
      <td className="px-3 py-2.5 text-[11px] text-[#ccc]">{t.capability}</td>
      <td className="max-w-[160px] truncate px-3 py-2.5 text-[10px] text-[#444]" title={t.peer}>
        {t.peer ? t.peer.slice(0, 14) + '…' : '—'}
      </td>
      <td className="px-3 py-2.5">
        <span className="flex items-center gap-1.5 text-[10px]" style={{ color }}>
          <span className="h-[4px] w-[4px] rounded-full" style={{ background: color }} />
          {t.status}
        </span>
      </td>
      <td className="px-3 py-2.5 text-[10px] text-[#444] tabular-nums">{dur}</td>
    </tr>
  );
}

function Skeleton() {
  return (
    <div className="p-5">
      <div className="mb-5 h-4 w-16 rounded bg-[#1a1a1a] animate-pulse" />
      <div className="mb-5 grid grid-cols-5 gap-2">
        {Array.from({ length: 5 }).map((_, i) => (
          <div key={i} className="h-16 rounded border border-[#1a1a1a] bg-[#111] animate-pulse" />
        ))}
      </div>
      <div className="h-48 rounded border border-[#1a1a1a] bg-[#111] animate-pulse" />
    </div>
  );
}
