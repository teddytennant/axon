import { useState, useEffect } from 'react';
import { useTrust } from '../hooks/use-api';
import { useWebSocket } from '../hooks/use-websocket';
import type { TrustEntry } from '../lib/types';

export default function TrustPage() {
  const { data: initialTrust, isLoading } = useTrust();
  const { subscribe } = useWebSocket();
  const [entries, setEntries] = useState<TrustEntry[]>([]);

  useEffect(() => { if (initialTrust) setEntries(initialTrust); }, [initialTrust]);

  useEffect(() => {
    return subscribe('trust', (data) => {
      setEntries(data as TrustEntry[]);
    });
  }, [subscribe]);

  const sorted = [...entries].sort((a, b) => b.overall - a.overall);

  if (isLoading) return <LoadingSkeleton />;

  return (
    <div className="p-6">
      <div className="mb-6 flex items-center gap-3">
        <h1 className="text-sm font-medium text-white">Trust</h1>
        <span className="font-mono text-xs text-[#3a3a3a] tabular-nums">{entries.length}</span>
      </div>

      {sorted.length === 0 ? (
        <div className="flex flex-col items-center justify-center py-24">
          <p className="text-sm text-[#3a3a3a]">No trust data available</p>
        </div>
      ) : (
        <div className="overflow-x-auto rounded border border-[#1c1c1c]">
          <table className="w-full text-left text-sm">
            <thead>
              <tr className="border-b border-[#1c1c1c] bg-[#0c0c0c]">
                {['Peer', 'Overall', 'Reliability', 'Accuracy', 'Availability', 'Obs'].map((h) => (
                  <th key={h} className="px-4 py-3 text-[10px] font-medium uppercase tracking-widest text-[#3a3a3a]">{h}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {sorted.map((entry) => <TrustRow key={entry.peer_id} entry={entry} />)}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

function ScoreBar({ value }: { value: number }) {
  const pct = Math.round(value * 100);
  return (
    <div className="flex items-center gap-2">
      <div className="h-px w-20 bg-[#1c1c1c]">
        <div className="h-px bg-white transition-all" style={{ width: `${pct}%` }} />
      </div>
      <span className="font-mono text-xs text-[#6b6b6b] tabular-nums">{pct}%</span>
    </div>
  );
}

function TrustRow({ entry }: { entry: TrustEntry }) {
  return (
    <tr className="border-b border-[#1c1c1c] last:border-0 hover:bg-[#0c0c0c]">
      <td className="px-4 py-3 font-mono text-xs text-white" title={entry.peer_id}>
        {entry.peer_id.slice(0, 16)}…
      </td>
      <td className="px-4 py-3"><ScoreBar value={entry.overall} /></td>
      <td className="px-4 py-3"><ScoreBar value={entry.reliability} /></td>
      <td className="px-4 py-3"><ScoreBar value={entry.accuracy} /></td>
      <td className="px-4 py-3"><ScoreBar value={entry.availability} /></td>
      <td className="px-4 py-3 font-mono text-xs text-[#6b6b6b] tabular-nums">{entry.observation_count}</td>
    </tr>
  );
}

function LoadingSkeleton() {
  return (
    <div className="p-6">
      <div className="mb-6 h-5 w-14 animate-pulse rounded bg-[#141414]" />
      <div className="h-64 animate-pulse rounded border border-[#1c1c1c] bg-[#0c0c0c]" />
    </div>
  );
}
