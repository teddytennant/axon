import { useState, useMemo } from 'react';
import { Search } from 'lucide-react';
import { useTools } from '../hooks/use-api';
import type { ToolResponse } from '../lib/types';

export default function ToolsPage() {
  const { data: tools, isLoading } = useTools();
  const [query, setQuery] = useState('');
  const [serverFilter, setServerFilter] = useState('');

  const serverNames = useMemo(() => {
    if (!tools) return [];
    return Array.from(new Set((tools as ToolResponse[]).map((t) => t.server))).sort();
  }, [tools]);

  const filtered = useMemo(() => {
    if (!tools) return [];
    return (tools as ToolResponse[]).filter((tool) => {
      const q = query.toLowerCase();
      const matchesQuery = !q || tool.name.toLowerCase().includes(q) || tool.description.toLowerCase().includes(q);
      const matchesServer = !serverFilter || tool.server === serverFilter;
      return matchesQuery && matchesServer;
    });
  }, [tools, query, serverFilter]);

  if (isLoading) return <LoadingSkeleton />;

  return (
    <div className="p-6">
      <div className="mb-6 flex items-center gap-3">
        <h1 className="text-sm font-medium text-white">Tools</h1>
        <span className="font-mono text-xs text-[#3a3a3a] tabular-nums">{filtered.length}</span>
      </div>

      <div className="mb-6 flex flex-col gap-3 sm:flex-row">
        <div className="relative flex-1">
          <Search size={12} className="absolute left-3 top-1/2 -translate-y-1/2 text-[#3a3a3a]" />
          <input
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search tools..."
            className="w-full rounded border border-[#1c1c1c] bg-[#0c0c0c] py-2 pl-8 pr-4 text-sm text-white outline-none transition-colors placeholder:text-[#3a3a3a] hover:border-[#2a2a2a] focus:border-[#2a2a2a]"
          />
        </div>
        {serverNames.length > 0 && (
          <select
            value={serverFilter}
            onChange={(e) => setServerFilter(e.target.value)}
            className="rounded border border-[#1c1c1c] bg-[#0c0c0c] px-3 py-2 text-sm text-white outline-none hover:border-[#2a2a2a]"
          >
            <option value="">All servers</option>
            {serverNames.map((name) => (
              <option key={name} value={name}>{name}</option>
            ))}
          </select>
        )}
      </div>

      {filtered.length === 0 ? (
        <div className="flex flex-col items-center justify-center py-24">
          <p className="text-sm text-[#3a3a3a]">
            {query || serverFilter ? 'No matching tools' : 'No tools available'}
          </p>
        </div>
      ) : (
        <div className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
          {filtered.map((tool) => <ToolCard key={`${tool.server}:${tool.name}`} tool={tool} />)}
        </div>
      )}
    </div>
  );
}

function ToolCard({ tool }: { tool: ToolResponse }) {
  return (
    <div className="rounded border border-[#1c1c1c] bg-[#0c0c0c] p-4">
      <div className="mb-2 flex items-start justify-between gap-2">
        <p className="font-mono text-sm font-medium text-white">{tool.name}</p>
        <span className="shrink-0 rounded border border-[#1c1c1c] px-2 py-0.5 font-mono text-[10px] text-[#6b6b6b]">
          {tool.server}
        </span>
      </div>
      {tool.description && (
        <p className="text-xs leading-relaxed text-[#6b6b6b]">{tool.description}</p>
      )}
      {tool.peer_id && (
        <p className="mt-2 font-mono text-[10px] text-[#3a3a3a]" title={tool.peer_id}>
          {tool.peer_id.slice(0, 16)}…
        </p>
      )}
    </div>
  );
}

function LoadingSkeleton() {
  return (
    <div className="p-6">
      <div className="mb-6 h-5 w-14 animate-pulse rounded bg-[#141414]" />
      <div className="mb-6 h-9 animate-pulse rounded bg-[#0c0c0c] border border-[#1c1c1c]" />
      <div className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
        {Array.from({ length: 6 }).map((_, i) => (
          <div key={i} className="h-28 animate-pulse rounded border border-[#1c1c1c] bg-[#0c0c0c]" />
        ))}
      </div>
    </div>
  );
}
