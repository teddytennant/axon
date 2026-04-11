import { useState, useEffect } from 'react';
import { setAxonBase, getAxonBase } from '../lib/api';
import { useQueryClient } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';

export default function SettingsPage() {
  const [url, setUrl]         = useState(getAxonBase());
  const [probing, setProbing] = useState(false);
  const [saved, setSaved]     = useState(false);
  const qc = useQueryClient();

  // Try to auto-detect node on mount
  useEffect(() => {
    void autoProbe();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function autoProbe() {
    setProbing(true);
    try {
      const found = await invoke<string>('probe_axon_ports');
      if (found && found !== getAxonBase()) {
        setUrl(found);
        toast.success(`Axon found at ${found}`);
      }
    } catch { /* invoke may fail in dev */ }
    setProbing(false);
  }

  function save() {
    const trimmed = url.trim();
    if (!trimmed) return;
    setAxonBase(trimmed);
    void qc.invalidateQueries();
    setSaved(true);
    setTimeout(() => setSaved(false), 1500);
  }

  return (
    <div className="flex h-full flex-col overflow-auto p-5">
      <h1 className="mb-6 text-[9px] uppercase tracking-[0.2em] text-[#2a2a2a]">Settings</h1>

      <section className="mb-5 max-w-xs space-y-1.5">
        <p className="text-[9px] uppercase tracking-widest text-[#222]">Node URL</p>
        <div className="flex items-center gap-2">
          <input
            value={url}
            onChange={e => setUrl(e.target.value)}
            onKeyDown={e => e.key === 'Enter' && save()}
            className="flex-1 rounded border border-[#181818] bg-[#080808] px-2.5 py-1.5 font-mono text-[11px] text-[#888] placeholder:text-[#2a2a2a] focus:border-[#242424] focus:outline-none transition-colors"
            placeholder="http://localhost:3000"
            spellCheck={false}
            style={{ userSelect: 'text' }}
          />
          <button
            onClick={save}
            className="rounded border border-[#181818] bg-transparent px-2.5 py-1.5 font-mono text-[10px] text-[#444] transition-colors hover:border-[#2a2a2a] hover:text-[#888]"
          >
            {saved ? 'saved' : 'connect'}
          </button>
        </div>
        <p className="font-mono text-[9px] text-[#1a1a1a]">run: axon start --web-port 3000</p>
      </section>

      <section className="max-w-xs space-y-1.5">
        <p className="text-[9px] uppercase tracking-widest text-[#222]">Discovery</p>
        <button
          onClick={autoProbe}
          disabled={probing}
          className="rounded border border-[#181818] bg-transparent px-2.5 py-1.5 font-mono text-[10px] text-[#333] transition-colors hover:border-[#242424] hover:text-[#666] disabled:opacity-30"
        >
          {probing ? 'scanning…' : 'auto-detect node'}
        </button>
      </section>

      <div className="mt-auto border-t border-[#0e0e0e] pt-4">
        <p className="font-mono text-[9px] text-[#1a1a1a]">⌘K  navigate</p>
      </div>
    </div>
  );
}
