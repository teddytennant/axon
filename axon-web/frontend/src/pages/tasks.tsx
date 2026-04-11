import { useState, useEffect } from 'react';
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
      <h1 className="mb-6 text-sm font-medium text-white">Tasks</h1>

      {stats && <StatsRow stats={stats} />}

      {tasks.length === 0 ? (
        <div className="flex flex-col items-center justify-center py-24">
          <p className="text-sm text-[#3a3a3a]">No tasks recorded</p>
        </div>
      ) : (
        <div className="overflow-x-auto rounded border border-[#1c1c1c]">
          <table className="w-full text-left text-sm">
            <thead>
              <tr className="border-b border-[#1c1c1c] bg-[#0c0c0c]">
                {['ID', 'Capability', 'Peer', 'Status', 'Duration'].map((h) => (
                  <th key={h} className="px-4 py-3 text-[10px] font-medium uppercase tracking-widest text-[#3a3a3a]">{h}</th>
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
    { label: 'Pending',   value: stats.pending,   color: '#6b6b6b' },
    { label: 'Running',   value: stats.running,   color: '#ffffff' },
    { label: 'Completed', value: stats.completed, color: '#22c55e' },
    { label: 'Failed',    value: stats.failed,    color: '#ef4444' },
    { label: 'Total',     value: stats.total,     color: '#ffffff' },
  ];

  return (
    <div className="mb-6 grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-5">
      {cards.map(({ label, value, color }) => (
        <div key={label} className="rounded border border-[#1c1c1c] bg-[#0c0c0c] p-4">
          <p className="text-[10px] uppercase tracking-widest text-[#3a3a3a]">{label}</p>
          <p className="mt-1 font-mono text-xl font-medium" style={{ color }}>{value}</p>
        </div>
      ))}
    </div>
  );
}

const STATUS_STYLES: Record<string, { color: string }> = {
  pending:   { color: '#6b6b6b' },
  running:   { color: '#ffffff' },
  completed: { color: '#22c55e' },
  failed:    { color: '#ef4444' },
  cancelled: { color: '#f59e0b' },
};

function TaskRow({ task }: { task: TaskLogEntry }) {
  const st = STATUS_STYLES[task.status] ?? STATUS_STYLES.pending;

  const durationStr = task.duration_ms > 0
    ? task.duration_ms < 1000 ? `${task.duration_ms}ms` : `${(task.duration_ms / 1000).toFixed(1)}s`
    : '—';

  return (
    <tr className="border-b border-[#1c1c1c] last:border-0 hover:bg-[#0c0c0c]">
      <td className="px-4 py-3 font-mono text-xs text-[#6b6b6b]" title={task.id}>{task.id.slice(0, 8)}</td>
      <td className="px-4 py-3 text-xs text-white">{task.capability}</td>
      <td className="max-w-[200px] truncate px-4 py-3 font-mono text-xs text-[#6b6b6b]" title={task.peer}>
        {task.peer ? task.peer.slice(0, 16) + '…' : '—'}
      </td>
      <td className="px-4 py-3">
        <span
          className="inline-flex items-center gap-1.5 text-[10px] font-medium uppercase"
          style={{ color: st.color }}
        >
          <span className="h-[5px] w-[5px] rounded-full shrink-0" style={{ backgroundColor: st.color }} />
          {task.status}
        </span>
      </td>
      <td className="px-4 py-3 font-mono text-xs text-[#6b6b6b]">{durationStr}</td>
    </tr>
  );
}

function LoadingSkeleton() {
  return (
    <div className="p-6">
      <div className="mb-6 h-5 w-16 animate-pulse rounded bg-[#141414]" />
      <div className="mb-6 grid grid-cols-5 gap-3">
        {Array.from({ length: 5 }).map((_, i) => (
          <div key={i} className="h-20 animate-pulse rounded border border-[#1c1c1c] bg-[#0c0c0c]" />
        ))}
      </div>
      <div className="h-64 animate-pulse rounded border border-[#1c1c1c] bg-[#0c0c0c]" />
    </div>
  );
}
