import { useStatus } from '../../hooks/use-api';
import { Activity } from 'lucide-react';
import { Badge } from '../ui/badge';

export function Header() {
  const { data: status } = useStatus();

  const currentModel = status?.model ?? '—';
  const peerCount = status?.peer_count ?? 0;
  const isOnline = status !== undefined;

  return (
    <header className="flex h-14 items-center justify-between border-b border-[#222] bg-[#0a0a0a] px-6">
      <span className="font-mono text-sm text-[#888]">{currentModel}</span>

      <div className="flex items-center gap-4">
        <div className="flex items-center gap-1.5 text-xs text-[#555]">
          <Activity size={14} />
          <span className="font-mono">{peerCount} peer{peerCount !== 1 ? 's' : ''}</span>
        </div>

        <Badge variant={isOnline ? 'success' : 'error'}>
          <span
            className={`mr-1.5 inline-block h-1.5 w-1.5 rounded-full ${isOnline ? 'bg-[#50dc78]' : 'bg-[#f05050]'}`}
          />
          {isOnline ? 'online' : 'offline'}
        </Badge>
      </div>
    </header>
  );
}
