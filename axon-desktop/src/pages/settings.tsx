import { useState } from 'react';
import { Server } from 'lucide-react';
import { setAxonBase, getAxonBase } from '../lib/api';
import { useQueryClient } from '@tanstack/react-query';

export default function SettingsPage() {
  const qc = useQueryClient();
  const [url, setUrl] = useState(getAxonBase());
  const [saved, setSaved] = useState(false);

  const save = () => {
    setAxonBase(url);
    void qc.invalidateQueries();
    setSaved(true);
    setTimeout(() => setSaved(false), 2000);
  };

  return (
    <div className="p-5 space-y-5 bg-[#07070d] h-full overflow-auto">
      <h1 className="text-sm font-semibold text-[#e0e0f4]">Settings</h1>

      <div className="rounded-xl border border-[#141424] bg-[#0e0e18] p-4 space-y-3">
        <div className="flex items-center gap-2 mb-1">
          <Server size={13} className="text-[#6868a0]" />
          <span className="text-xs font-medium text-[#a0a0c8]">Axon Node</span>
        </div>
        <p className="text-[10px] text-[#3a3a58]">Connect to a running axon node. Start one with <code className="font-mono bg-[#141424] px-1 rounded">axon start --web-port 3000</code></p>
        <div className="flex gap-2">
          <input
            value={url}
            onChange={e => setUrl(e.target.value)}
            className="flex-1 rounded-lg border border-[#1a1a2a] bg-[#0c0c16] px-3 py-1.5 font-mono text-xs text-[#e0e0f4] outline-none focus:border-[#00c8c8]/30"
          />
          <button
            onClick={save}
            className="rounded-lg bg-[#00c8c8]/10 px-3 py-1.5 text-xs font-medium text-[#00c8c8] transition-colors hover:bg-[#00c8c8]/20"
          >
            {saved ? 'Saved!' : 'Connect'}
          </button>
        </div>
      </div>
    </div>
  );
}
