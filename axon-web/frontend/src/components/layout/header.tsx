import { useLocation } from 'react-router';
import { useStatus } from '../../hooks/use-api';

const PAGE_TITLES: Record<string, string> = {
  '/': 'Chat',
  '/mesh': 'Mesh',
  '/agents': 'Agents',
  '/graph': 'Graph',
  '/tasks': 'Tasks',
  '/workflows': 'Workflows',
  '/blackboard': 'Blackboard',
  '/trust': 'Trust',
  '/tools': 'Tools',
  '/settings': 'Settings',
  '/logs': 'Logs',
};

export function Header() {
  const location = useLocation();
  const { data: status } = useStatus();

  const currentModel = status?.model ?? null;
  const peerCount = status?.peer_count ?? 0;
  const isOnline = status !== undefined;
  const pageTitle = PAGE_TITLES[location.pathname] ?? '';

  return (
    <header className="flex h-11 items-center justify-between border-b border-[#1c1c1c] bg-[#000000] px-5">
      <div className="flex items-center gap-3">
        <span className="text-sm font-medium text-white">{pageTitle}</span>
        {currentModel && (
          <>
            <span className="text-[#2a2a2a]">·</span>
            <span className="font-mono text-xs text-[#3a3a3a]">{currentModel}</span>
          </>
        )}
      </div>

      <div className="flex items-center gap-4">
        {peerCount > 0 && (
          <span className="font-mono text-xs text-[#3a3a3a] tabular-nums">
            {peerCount}p
          </span>
        )}

        <div className="flex items-center gap-1.5">
          <span className={`h-[5px] w-[5px] rounded-full ${isOnline ? 'bg-[#22c55e]' : 'bg-[#ef4444]'}`} />
          <span className="text-[11px] text-[#3a3a3a]">{isOnline ? 'live' : 'offline'}</span>
        </div>
      </div>
    </header>
  );
}
