import { useState, useMemo } from 'react';
import { Search, Wrench } from 'lucide-react';
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
        <h1 className="text-lg font-semibold text-[#f5f5f5]">Tools</h1>
        <span className="rounded-full bg-[#00c8c8]/10 px-2.5 py-0.5 font-mono text-xs text-[#00c8c8]">{filtered.length}</span>
      </div>

      <div className="mb-6 flex flex-col gap-3 sm:flex-row">
        <div className="relative flex-1">
          <Search size={14} className="absolute left-3 top-1/2 -translate-y-1/2 text-[#555]" />
          <input
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search tools..."
            className="w-full rounded-lg border border-[#222] bg-[#111] py-2 pl-9 pr-4 text-sm text-[#f5f5f5] outline-none transition-colors placeholder:text-[#555] focus:border-[#333]"
          />
        </div>
        {serverNames.length > 0 && (
          <select
            value={serverFilter}
            onChange={(e) => setServerFilter(e.target.value)}
            className="rounded-lg border border-[#222] bg-[#111] px-3 py-2 text-sm text-[#f5f5f5] outline-none"
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
          <Wrench size={32} className="mb-3 text-[#555]" />
          <p className="text-sm text-[#555]">
            {query || serverFilter ? 'No matching tools' : 'No tools available'}
          </p>
        </div>
      ) : (
        <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
          {filtered.map((tool) => <ToolCard key={`${tool.server}:${tool.name}`} tool={tool} />)}
        </div>
      )}
    </div>
  );
}

function ToolCard({ tool }: { tool: ToolResponse }) {
  return (
    <div className="rounded-lg border border-[#222] bg-[#111] p-4">
      <div className="mb-2 flex items-start justify-between gap-2">
        <p className="font-mono text-sm font-medium text-[#f5f5f5]">{tool.name}</p>
        <span className="shrink-0 rounded bg-[#00c8c8]/10 px-2 py-0.5 font-mono text-[10px] text-[#00c8c8]">
          {tool.server}
        </span>
      </div>
      {tool.description && (
        <p className="text-xs leading-relaxed text-[#888]">{tool.description}</p>
      )}
      {tool.peer_id && (
        <p className="mt-2 font-mono text-[10px] text-[#555]" title={tool.peer_id}>
          {tool.peer_id.slice(0, 16)}…
        </p>
      )}
    </div>
  );
}

function LoadingSkeleton() {
  return (
    <div className="p-6">
      <div className="mb-6 h-6 w-24 animate-pulse rounded bg-[#181818]" />
      <div className="mb-6 h-10 animate-pulse rounded-lg bg-[#111]" />
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
        {Array.from({ length: 6 }).map((_, i) => (
          <div key={i} className="h-28 animate-pulse rounded-lg border border-[#222] bg-[#111]" />
        ))}
      </div>
    </div>
  );
}
