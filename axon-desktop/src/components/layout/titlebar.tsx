import { useStatus } from '../../hooks/use-api';
import { useLocation } from 'react-router';
import { clsx } from 'clsx';

const TITLES: Record<string, string> = {
  '/':          'graph',
  '/chat':      'chat',
  '/mesh':      'mesh',
  '/agents':    'agents',
  '/tasks':     'tasks',
  '/workflows': 'workflows',
  '/trust':     'trust',
  '/settings':  'settings',
};

export function Titlebar() {
  const location  = useLocation();
  const { data }  = useStatus();
  const online    = data !== undefined;
  const peers     = data?.peer_count ?? 0;
  const title     = TITLES[location.pathname] ?? 'axon';

  return (
    <div
      className="flex h-9 items-center justify-between border-b border-[#1a1a1a] bg-[#0a0a0a] px-4 shrink-0"
      data-tauri-drag-region
    >
      <span className="text-[11px] text-[#444]">{title}</span>

      <div className="flex items-center gap-3">
        {online && peers > 0 && (
          <span className="text-[10px] text-[#2a2a2a] tabular-nums">{peers}p</span>
        )}
        <span className={clsx(
          'h-[5px] w-[5px] rounded-full',
          online ? 'bg-[#22c55e]' : 'bg-[#ef4444]',
        )} />
      </div>
    </div>
  );
}
