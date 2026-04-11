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
    <div className="h-full overflow-auto p-5">
      <div className="mb-5 flex items-center gap-3">
        <span className="text-[11px] text-[#666]">trust</span>
        <span className="text-[10px] text-[#333] tabular-nums">{entries.length}</span>
      </div>

      {sorted.length === 0 ? (
        <div className="flex h-48 items-center justify-center">
          <p className="text-[11px] text-[#2a2a2a]">no trust data</p>
        </div>
      ) : (
        <div className="rounded border border-[#1f1f1f] overflow-hidden">
          <table className="w-full text-left">
            <thead>
              <tr className="border-b border-[#1a1a1a] bg-[#111]">
                {['peer', 'overall', 'reliability', 'accuracy', 'availability', 'obs'].map(h => (
                  <th key={h} className="px-3 py-2.5 text-[9px] uppercase tracking-widest text-[#333]">{h}</th>
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
  const pct = Math.round(value * 100);
  // monochrome: bright white for high scores, dim for low
  const lightness = Math.round(20 + pct * 0.5); // 20–70% lightness
  const barColor  = `hsl(0 0% ${lightness}%)`;
  return (
    <div className="flex items-center gap-2">
      <div className="h-[3px] w-16 rounded-full bg-[#1a1a1a]">
        <div className="h-[3px] rounded-full" style={{ width: `${pct}%`, background: barColor }} />
      </div>
      <span className="text-[10px] tabular-nums text-[#555]">{pct}</span>
    </div>
  );
}

function TrustRow({ entry: e }: { entry: TrustEntry }) {
  return (
    <tr className="border-b border-[#1a1a1a] last:border-0 hover:bg-[#141414] transition-colors">
      <td className="px-3 py-2.5 text-[10px] text-[#ccc]" title={e.peer_id}>
        {e.peer_id.slice(0, 16)}…
      </td>
      <td className="px-3 py-2.5"><ScoreBar value={e.overall} /></td>
      <td className="px-3 py-2.5"><ScoreBar value={e.reliability} /></td>
      <td className="px-3 py-2.5"><ScoreBar value={e.accuracy} /></td>
      <td className="px-3 py-2.5"><ScoreBar value={e.availability} /></td>
      <td className="px-3 py-2.5 text-[10px] text-[#444] tabular-nums">{e.observation_count}</td>
    </tr>
  );
}

function Skeleton() {
  return (
    <div className="p-5">
      <div className="mb-5 h-4 w-16 rounded bg-[#1a1a1a] animate-pulse" />
      <div className="h-48 rounded border border-[#1a1a1a] bg-[#111] animate-pulse" />
    </div>
  );
}
