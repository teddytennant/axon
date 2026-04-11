import { useState, useEffect } from 'react';
import { useTrust } from '../hooks/use-api';
import { useWebSocket } from '../hooks/use-websocket';
import type { TrustEntry } from '../lib/types';

export default function TrustPage() {
  const { data: init, isLoading } = useTrust();
  const { subscribe }             = useWebSocket();
  const [entries, setEntries]     = useState<TrustEntry[]>([]);

  useEffect(() => { if (init) setEntries(init); }, [init]);
  useEffect(() => subscribe('trust', d => setEntries(d as TrustEntry[])), [subscribe]);

  const sorted = [...entries].sort((a, b) => b.overall - a.overall);

  if (isLoading) return <Skeleton />;

  return (
    <div className="h-full overflow-auto p-6">
      <div className="mb-6 flex items-baseline gap-3">
        <span className="text-[11px] font-medium tracking-wider text-[#555]">trust</span>
        <span className="text-[10px] tabular-nums text-[#2e2e2e]">{entries.length}</span>
      </div>

      {sorted.length === 0 ? (
        <div className="flex h-48 items-center justify-center">
          <p className="text-[11px] text-[#1e1e1e]">no trust data</p>
        </div>
      ) : (
        <div className="overflow-hidden rounded-lg border border-[#1e1e1e]">
          <table className="w-full text-left">
            <thead>
              <tr className="border-b border-[#181818] bg-[#080808]">
                {['peer', 'overall', 'reliability', 'accuracy', 'availability', 'obs'].map(h => (
                  <th key={h} className="px-4 py-2.5 text-[8px] font-medium uppercase tracking-[0.15em] text-[#2e2e2e]">{h}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {sorted.map(e => <TrustRow key={e.peer_id} entry={e} />)}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

function ScoreBar({ value }: { value: number }) {
  const pct       = Math.round(value * 100);
  const lightness = Math.round(18 + pct * 0.52);
  const barColor  = `hsl(0 0% ${lightness}%)`;
  return (
    <div className="flex items-center gap-2.5">
      <div className="relative h-[2px] w-20 rounded-full bg-[#181818]">
        <div
          className="absolute inset-y-0 left-0 rounded-full transition-all duration-500"
          style={{ width: `${pct}%`, background: barColor }}
        />
      </div>
      <span className="w-6 text-right text-[10px] tabular-nums text-[#484848]">{pct}</span>
    </div>
  );
}

function TrustRow({ entry: e }: { entry: TrustEntry }) {
  return (
    <tr className="border-b border-[#111] last:border-0 hover:bg-[#0a0a0a] transition-colors">
      <td className="px-4 py-3 font-mono text-[10px] text-[#888]" title={e.peer_id}>
        {e.peer_id.slice(0, 16)}&hellip;
      </td>
      <td className="px-4 py-3"><ScoreBar value={e.overall} /></td>
      <td className="px-4 py-3"><ScoreBar value={e.reliability} /></td>
      <td className="px-4 py-3"><ScoreBar value={e.accuracy} /></td>
      <td className="px-4 py-3"><ScoreBar value={e.availability} /></td>
      <td className="px-4 py-3 text-[10px] tabular-nums text-[#383838]">{e.observation_count}</td>
    </tr>
  );
}

function Skeleton() {
  return (
    <div className="p-6">
      <div className="mb-6 h-4 w-16 rounded animate-shimmer" />
      <div className="h-48 rounded-lg border border-[#141414] animate-shimmer" />
    </div>
  );
}
