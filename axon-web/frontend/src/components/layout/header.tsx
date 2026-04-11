import { useLocation } from 'react-router';
import { useStatus } from '../../hooks/use-api';
import { Activity, Zap } from 'lucide-react';

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
    <header className="flex h-12 items-center justify-between border-b border-[#1a1a1a] bg-[#0a0a0a] px-5">
      <div className="flex items-center gap-3">
        <span className="text-sm font-semibold text-[#f5f5f5]">{pageTitle}</span>
        {currentModel && (
          <>
            <span className="text-[#333]">·</span>
            <span className="font-mono text-xs text-[#444]">{currentModel}</span>
          </>
        )}
      </div>

      <div className="flex items-center gap-4">
        <div className="flex items-center gap-1.5 text-xs text-[#444]">
          <Activity size={13} />
          <span className="font-mono">{peerCount} peer{peerCount !== 1 ? 's' : ''}</span>
        </div>

        <div className={`flex items-center gap-1.5 text-[10px] font-medium px-2 py-0.5 rounded-full ${
          isOnline
            ? 'bg-[#50dc78]/8 text-[#50dc78]'
            : 'bg-[#f05050]/8 text-[#f05050]'
        }`}>
          {isOnline ? (
            <Zap size={10} className="fill-current" />
          ) : (
            <span className="h-1.5 w-1.5 rounded-full bg-current" />
          )}
          {isOnline ? 'live' : 'offline'}
        </div>
      </div>
    </header>
  );
}
