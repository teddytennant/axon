import { useState, useEffect } from 'react';
import { setAxonBase, getAxonBase } from '../lib/api';
import { useQueryClient } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Check, Loader2 } from 'lucide-react';

export default function SettingsPage() {
  const [url, setUrl]         = useState(getAxonBase());
  const [probing, setProbing] = useState(false);
  const [saved, setSaved]     = useState(false);
  const qc = useQueryClient();

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
    <div className="flex h-full flex-col overflow-auto p-6">
      {/* Page title */}
      <div className="mb-8">
        <h1 className="text-[11px] font-medium uppercase tracking-[0.25em] text-[#444]">Settings</h1>
      </div>

      {/* Node URL */}
      <section className="mb-8 max-w-sm">
        <Label>Node URL</Label>
        <p className="mb-3 text-[10px] text-[#2a2a2a]">Address of the running axon node</p>
        <div className="flex items-center gap-2">
          <input
            value={url}
            onChange={e => setUrl(e.target.value)}
            onKeyDown={e => e.key === 'Enter' && save()}
            className="flex-1 rounded-lg border border-[#1e1e1e] bg-[#080808] px-3 py-2 font-mono text-[11px] text-[#aaa] placeholder:text-[#252525] outline-none transition-colors focus:border-[#2e2e2e] focus:bg-[#0c0c0c]"
            placeholder="http://localhost:3000"
            spellCheck={false}
            style={{ userSelect: 'text' }}
          />
          <button
            onClick={save}
            className="flex h-[34px] items-center gap-1.5 rounded-lg border border-[#1e1e1e] bg-[#080808] px-3 text-[10px] text-[#555] transition-all hover:border-[#2e2e2e] hover:text-[#aaa] active:scale-95"
          >
            {saved ? (
              <>
                <Check size={11} className="text-[#22c55e]" />
                <span>saved</span>
              </>
            ) : (
              <span>connect</span>
            )}
          </button>
        </div>
        <p className="mt-2 font-mono text-[9px] text-[#1c1c1c]">axon start --web-port 3000</p>
      </section>

      {/* Discovery */}
      <section className="mb-8 max-w-sm">
        <Label>Discovery</Label>
        <p className="mb-3 text-[10px] text-[#2a2a2a]">Scan common ports for a running node</p>
        <button
          onClick={autoProbe}
          disabled={probing}
          className="flex h-[34px] items-center gap-2 rounded-lg border border-[#1e1e1e] bg-[#080808] px-3 text-[10px] text-[#555] transition-all hover:border-[#2e2e2e] hover:text-[#aaa] active:scale-95 disabled:opacity-40 disabled:cursor-not-allowed"
        >
          {probing ? (
            <>
              <Loader2 size={11} className="animate-spin" />
              <span>scanning…</span>
            </>
          ) : (
            <span>auto-detect node</span>
          )}
        </button>
      </section>

      {/* Keyboard shortcuts */}
      <div className="max-w-sm">
        <Label>Keyboard</Label>
        <div className="mt-3 space-y-2">
          {[
            { key: '⌘K',    desc: 'Command palette' },
            { key: '1 – 8', desc: 'Navigate pages'  },
          ].map(({ key, desc }) => (
            <div key={key} className="flex items-center justify-between">
              <span className="text-[10px] text-[#2e2e2e]">{desc}</span>
              <kbd className="rounded border border-[#1a1a1a] bg-[#0a0a0a] px-2 py-px font-mono text-[9px] text-[#444]">
                {key}
              </kbd>
            </div>
          ))}
        </div>
      </div>

      {/* Footer */}
      <div className="mt-auto border-t border-[#0f0f0f] pt-4">
        <p className="font-mono text-[9px] text-[#181818]">axon desktop</p>
      </div>
    </div>
  );
}

function Label({ children }: { children: React.ReactNode }) {
  return (
    <p className="mb-1 text-[9px] font-medium uppercase tracking-[0.2em] text-[#333]">
      {children}
    </p>
  );
}
