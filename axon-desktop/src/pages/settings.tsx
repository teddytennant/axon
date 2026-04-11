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
    <div className="p-5 space-y-5 bg-[#000000] h-full overflow-auto">
      <h1 className="text-[11px] font-medium text-white">settings</h1>

      <div className="rounded border border-[#1c1c1c] bg-[#0c0c0c] p-4 space-y-3">
        <div className="flex items-center gap-2 mb-1">
          <Server size={12} className="text-[#555]" />
          <span className="text-[10px] font-medium text-[#888]">Axon Node</span>
        </div>
        <p className="text-[10px] text-[#3a3a3a]">
          Connect to a running axon node. Start with{' '}
          <code className="font-mono bg-[#141414] px-1 rounded text-[#666]">axon start --web-port 3000</code>
        </p>
        <div className="flex gap-2">
          <input
            value={url}
            onChange={e => setUrl(e.target.value)}
            className="flex-1 rounded border border-[#1c1c1c] bg-[#000] px-3 py-1.5 font-mono text-[10px] text-white outline-none focus:border-[#2a2a2a] transition-colors"
          />
          <button
            onClick={save}
            className="rounded border border-[#1c1c1c] px-3 py-1.5 text-[10px] font-medium text-white transition-colors hover:bg-[#141414] hover:border-[#2a2a2a]"
          >
            {saved ? 'saved' : 'connect'}
          </button>
        </div>
      </div>
    </div>
  );
}
