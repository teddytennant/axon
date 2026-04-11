import { useState, useEffect } from 'react';
import { useWebSocket } from '../hooks/use-websocket';
import type { BlackboardEntry } from '../lib/types';

export default function BlackboardPage() {
  const { subscribe } = useWebSocket();
  const [entries, setEntries] = useState<BlackboardEntry[]>([]);

  useEffect(() => {
    return subscribe('blackboard', (data) => setEntries(data as BlackboardEntry[]));
  }, [subscribe]);

  return (
    <div className="p-6">
      <div className="mb-6 flex items-center gap-3">
        <h1 className="text-sm font-medium text-white">Blackboard</h1>
        <span className="font-mono text-xs text-[#3a3a3a] tabular-nums">{entries.length}</span>
      </div>

      {entries.length === 0 ? (
        <div className="flex flex-col items-center justify-center py-24">
          <p className="text-sm text-[#3a3a3a]">No blackboard entries yet</p>
          <p className="mt-1 font-mono text-xs text-[#2a2a2a]">
            Written by agents via Blackboard::write()
          </p>
        </div>
      ) : (
        <div className="rounded border border-[#1c1c1c]">
          <table className="w-full font-mono text-xs">
            <thead>
              <tr className="border-b border-[#1c1c1c] bg-[#0c0c0c]">
                <th className="px-4 py-2.5 text-left text-[10px] uppercase tracking-widest text-[#3a3a3a]">Key</th>
                <th className="px-4 py-2.5 text-left text-[10px] uppercase tracking-widest text-[#3a3a3a]">Value</th>
                <th className="px-4 py-2.5 text-right text-[10px] uppercase tracking-widest text-[#3a3a3a]">Time</th>
              </tr>
            </thead>
            <tbody>
              {entries.map((e) => (
                <tr key={e.key} className="border-b border-[#1c1c1c] last:border-0 hover:bg-[#0c0c0c]">
                  <td className="px-4 py-2.5 text-white">{e.key}</td>
                  <td className="px-4 py-2.5 max-w-md truncate text-[#aaaaaa]">{e.value}</td>
                  <td className="px-4 py-2.5 text-right text-[#3a3a3a]">
                    {new Date(e.timestamp_ms).toLocaleTimeString()}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
