import { useState, useEffect } from 'react';
import { Database } from 'lucide-react';
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
        <h1 className="text-lg font-semibold text-[#f5f5f5]">Blackboard</h1>
        <span className="rounded-full bg-[#00c8c8]/10 px-2.5 py-0.5 font-mono text-xs text-[#00c8c8]">
          {entries.length}
        </span>
      </div>

      {entries.length === 0 ? (
        <div className="flex flex-col items-center justify-center py-24">
          <Database size={32} className="mb-3 text-[#555]" />
          <p className="text-sm text-[#555]">No blackboard entries yet</p>
          <p className="mt-1 font-mono text-xs text-[#444]">
            Written by agents via Blackboard::write()
          </p>
        </div>
      ) : (
        <div className="rounded-lg border border-[#222] bg-[#111]">
          <table className="w-full font-mono text-xs">
            <thead>
              <tr className="border-b border-[#222]">
                <th className="px-4 py-2.5 text-left text-[10px] uppercase tracking-widest text-[#444]">Key</th>
                <th className="px-4 py-2.5 text-left text-[10px] uppercase tracking-widest text-[#444]">Value</th>
                <th className="px-4 py-2.5 text-right text-[10px] uppercase tracking-widest text-[#444]">Timestamp</th>
              </tr>
            </thead>
            <tbody>
              {entries.map((e) => (
                <tr key={e.key} className="border-b border-[#1a1a1a] last:border-0 hover:bg-[#181818]">
                  <td className="px-4 py-2.5 text-[#f5f5f5]">{e.key}</td>
                  <td className="px-4 py-2.5 max-w-md truncate text-[#f0c83c]">{e.value}</td>
                  <td className="px-4 py-2.5 text-right text-[#444]">
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
