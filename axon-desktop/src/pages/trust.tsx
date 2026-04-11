import { useState, useEffect } from 'react';
import { Shield } from 'lucide-react';
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
        <h1 className="text-lg font-semibold text-[#f5f5f5]">Trust</h1>
        <span className="rounded-full bg-[#00c8c8]/10 px-2.5 py-0.5 font-mono text-xs text-[#00c8c8]">{entries.length}</span>
      </div>

      {sorted.length === 0 ? (
        <div className="flex flex-col items-center justify-center py-24">
          <Shield size={32} className="mb-3 text-[#555]" />
          <p className="text-sm text-[#555]">No trust data available</p>
        </div>
      ) : (
        <div className="overflow-x-auto rounded-lg border border-[#222]">
          <table className="w-full text-left text-sm">
            <thead>
              <tr className="border-b border-[#222] bg-[#111]">
                {['Peer', 'Overall', 'Reliability', 'Accuracy', 'Availability', 'Observations'].map((h) => (
                  <th key={h} className="px-4 py-3 text-[10px] font-medium uppercase tracking-widest text-[#555]">{h}</th>
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
  const color = pct >= 80 ? '#50dc78' : pct >= 60 ? '#00c8c8' : pct >= 40 ? '#f0c83c' : '#f05050';
  return (
    <div className="flex items-center gap-2">
      <div className="h-1.5 w-20 rounded-full bg-[#181818]">
        <div className="h-1.5 rounded-full transition-all" style={{ width: `${pct}%`, backgroundColor: color }} />
      </div>
      <span className="font-mono text-xs" style={{ color }}>{pct}%</span>
    </div>
  );
}

function TrustRow({ entry }: { entry: TrustEntry }) {
  return (
    <tr className="border-b border-[#222] last:border-0 hover:bg-[#181818]">
      <td className="px-4 py-3 font-mono text-xs text-[#f5f5f5]" title={entry.peer_id}>
        {entry.peer_id.slice(0, 16)}…
      </td>
      <td className="px-4 py-3"><ScoreBar value={entry.overall} /></td>
      <td className="px-4 py-3"><ScoreBar value={entry.reliability} /></td>
      <td className="px-4 py-3"><ScoreBar value={entry.accuracy} /></td>
      <td className="px-4 py-3"><ScoreBar value={entry.availability} /></td>
      <td className="px-4 py-3 font-mono text-xs text-[#888]">{entry.observation_count}</td>
    </tr>
  );
}

function LoadingSkeleton() {
  return (
    <div className="p-6">
      <div className="mb-6 h-6 w-24 animate-pulse rounded bg-[#181818]" />
      <div className="h-64 animate-pulse rounded-lg border border-[#222] bg-[#111]" />
    </div>
  );
}
