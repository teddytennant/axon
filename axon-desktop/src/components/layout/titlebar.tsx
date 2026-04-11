import { useStatus } from '../../hooks/use-api';
import { useLocation } from 'react-router';
import { clsx } from 'clsx';

const PAGE_TITLES: Record<string, string> = {
  '/': 'Graph',
  '/chat': 'Chat',
  '/mesh': 'Mesh',
  '/agents': 'Agents',
  '/tasks': 'Tasks',
  '/workflows': 'Workflows',
  '/trust': 'Trust',
  '/settings': 'Settings',
};

export function Titlebar() {
  const location = useLocation();
  const { data: status } = useStatus();
  const isOnline = status !== undefined;
  const pageTitle = PAGE_TITLES[location.pathname] ?? 'axon';
  const peers = status?.peer_count ?? 0;

  return (
    <div
      className="flex h-11 items-center justify-between border-b border-[#141424] bg-[#0a0a12] px-4 shrink-0"
      data-tauri-drag-region
    >
      <span className="text-xs font-semibold text-[#c8c8e8]">{pageTitle}</span>

      <div className="flex items-center gap-3">
        {isOnline && (
          <span className="font-mono text-[9px] text-[#2e2e4a]">
            {peers} peer{peers !== 1 ? 's' : ''}
          </span>
        )}
        <div className={clsx(
          'flex items-center gap-1 rounded-full px-2 py-0.5 text-[8px] font-mono',
          isOnline ? 'bg-[#50dc78]/8 text-[#50dc78]/70' : 'bg-[#f05050]/8 text-[#f05050]/60',
        )}>
          <span className={clsx(
            'h-1 w-1 rounded-full',
            isOnline ? 'bg-[#50dc78] animate-pulse' : 'bg-[#f05050]',
          )} />
          {isOnline ? 'live' : 'offline'}
        </div>
      </div>
    </div>
  );
}
