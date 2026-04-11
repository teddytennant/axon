import { useState, useEffect } from 'react';
import { clsx } from 'clsx';
import { ListChecks } from 'lucide-react';
import { useTaskLog, useTaskStats } from '../hooks/use-api';
import { useWebSocket } from '../hooks/use-websocket';
import type { TaskLogEntry, TaskStatsResponse, WsTasksData } from '../lib/types';

export default function TasksPage() {
  const { data: initialTasks, isLoading: tasksLoading } = useTaskLog();
  const { data: stats, isLoading: statsLoading } = useTaskStats();
  const { subscribe } = useWebSocket();
  const [tasks, setTasks] = useState<TaskLogEntry[]>([]);

  useEffect(() => { if (initialTasks) setTasks(initialTasks); }, [initialTasks]);

  useEffect(() => {
    return subscribe('tasks', (data) => {
      setTasks((data as WsTasksData).recent);
    });
  }, [subscribe]);

  if (tasksLoading || statsLoading) return <LoadingSkeleton />;

  return (
    <div className="p-6">
      <h1 className="mb-6 text-lg font-semibold text-[#f5f5f5]">Tasks</h1>

      {stats && <StatsRow stats={stats} />}

      {tasks.length === 0 ? (
        <div className="flex flex-col items-center justify-center py-24">
          <ListChecks size={32} className="mb-3 text-[#555]" />
          <p className="text-sm text-[#555]">No tasks recorded</p>
        </div>
      ) : (
        <div className="overflow-x-auto rounded-lg border border-[#222]">
          <table className="w-full text-left text-sm">
            <thead>
              <tr className="border-b border-[#222] bg-[#111]">
                {['ID', 'Capability', 'Peer', 'Status', 'Duration'].map((h) => (
                  <th key={h} className="px-4 py-3 text-[10px] font-medium uppercase tracking-widest text-[#555]">{h}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {tasks.map((task) => <TaskRow key={task.id} task={task} />)}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

function StatsRow({ stats }: { stats: TaskStatsResponse }) {
  const cards = [
    { label: 'Pending', value: stats.pending, color: '#888' },
    { label: 'Running', value: stats.running, color: '#00c8c8' },
    { label: 'Completed', value: stats.completed, color: '#50dc78' },
    { label: 'Failed', value: stats.failed, color: '#f05050' },
    { label: 'Total', value: stats.total, color: '#f5f5f5' },
  ];

  return (
    <div className="mb-6 grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-5">
      {cards.map(({ label, value, color }) => (
        <div key={label} className="rounded-lg border border-[#222] bg-[#111] p-4">
          <p className="text-[10px] uppercase tracking-widest text-[#555]">{label}</p>
          <p className="mt-1 font-mono text-xl font-semibold" style={{ color }}>{value}</p>
        </div>
      ))}
    </div>
  );
}

function TaskRow({ task }: { task: TaskLogEntry }) {
  const statusStyles: Record<string, { color: string; bg: string }> = {
    pending: { color: '#888', bg: 'bg-[#888]/10' },
    running: { color: '#00c8c8', bg: 'bg-[#00c8c8]/10' },
    completed: { color: '#50dc78', bg: 'bg-[#50dc78]/10' },
    failed: { color: '#f05050', bg: 'bg-[#f05050]/10' },
    cancelled: { color: '#f0c83c', bg: 'bg-[#f0c83c]/10' },
  };
  const st = statusStyles[task.status] ?? statusStyles.pending;

  const durationStr = task.duration_ms > 0
    ? task.duration_ms < 1000 ? `${task.duration_ms}ms` : `${(task.duration_ms / 1000).toFixed(1)}s`
    : '—';

  return (
    <tr className="border-b border-[#222] last:border-0 hover:bg-[#181818]">
      <td className="px-4 py-3 font-mono text-xs text-[#888]" title={task.id}>{task.id.slice(0, 8)}</td>
      <td className="px-4 py-3 text-xs text-[#f5f5f5]">{task.capability}</td>
      <td className="max-w-[200px] truncate px-4 py-3 font-mono text-xs text-[#888]" title={task.peer}>
        {task.peer ? task.peer.slice(0, 16) + '…' : '—'}
      </td>
      <td className="px-4 py-3">
        <span
          className={clsx('inline-flex items-center gap-1.5 rounded px-2 py-0.5 text-[10px] font-medium uppercase', st.bg)}
          style={{ color: st.color }}
        >
          <span className="h-1.5 w-1.5 rounded-full" style={{ backgroundColor: st.color }} />
          {task.status}
        </span>
      </td>
      <td className="px-4 py-3 font-mono text-xs text-[#888]">{durationStr}</td>
    </tr>
  );
}

function LoadingSkeleton() {
  return (
    <div className="p-6">
      <div className="mb-6 h-6 w-24 animate-pulse rounded bg-[#181818]" />
      <div className="mb-6 grid grid-cols-5 gap-3">
        {Array.from({ length: 5 }).map((_, i) => (
          <div key={i} className="h-20 animate-pulse rounded-lg border border-[#222] bg-[#111]" />
        ))}
      </div>
      <div className="h-64 animate-pulse rounded-lg border border-[#222] bg-[#111]" />
    </div>
  );
}
