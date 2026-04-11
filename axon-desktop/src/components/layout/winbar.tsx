import { getCurrentWindow } from '@tauri-apps/api/window';
import { useStatus } from '../../hooks/use-api';
import { useLocation } from 'react-router';
import { clsx } from 'clsx';

const PAGE_LABELS: Record<string, string> = {
  '/':          'graph',
  '/chat':      'chat',
  '/mesh':      'mesh',
  '/agents':    'agents',
  '/tasks':     'tasks',
  '/workflows': 'workflows',
  '/trust':     'trust',
  '/settings':  'settings',
};

export function Winbar() {
  const { data }   = useStatus();
  const location   = useLocation();
  const online     = data !== undefined;
  const peers      = data?.peer_count ?? 0;
  const page       = PAGE_LABELS[location.pathname] ?? 'axon';
  const win        = getCurrentWindow();

  return (
    <div
      className="flex h-8 shrink-0 items-center border-b border-[#181818] bg-[#000]"
      data-tauri-drag-region
    >
      {/* Window controls — monochrome */}
      <div className="flex items-center gap-[5px] pl-[14px] pr-3" data-no-drag>
        <WinBtn onClick={() => win.close()}          symbol="×" title="Close  ⌘W"   />
        <WinBtn onClick={() => win.minimize()}       symbol="−" title="Minimize ⌘M" />
        <WinBtn onClick={() => win.toggleMaximize()} symbol="□" title="Maximize"    />
      </div>

      {/* Page label — center */}
      <div className="flex flex-1 items-center justify-center" data-tauri-drag-region>
        <span className="select-none text-[9px] font-medium uppercase tracking-[0.28em] text-[#2e2e2e]">
          {page}
        </span>
      </div>

      {/* Status — right */}
      <div className="flex items-center gap-[6px] pr-[14px]" data-no-drag>
        {peers > 0 && (
          <span className="text-[9px] tabular-nums text-[#333]">
            {peers}p
          </span>
        )}
        <span className={clsx(
          'h-[5px] w-[5px] rounded-full transition-colors duration-500',
          online ? 'bg-[#22c55e]' : 'bg-[#242424]',
        )} />
      </div>
    </div>
  );
}

function WinBtn({
  onClick,
  symbol,
  title,
}: {
  onClick: () => void;
  symbol: string;
  title: string;
}) {
  return (
    <button
      onClick={onClick}
      title={title}
      className="group relative flex h-[11px] w-[11px] items-center justify-center rounded-full bg-[#1e1e1e] transition-all duration-100 hover:bg-[#2e2e2e] active:scale-90"
    >
      <span className="absolute text-[6.5px] font-bold leading-none text-transparent transition-colors group-hover:text-[#888]">
        {symbol}
      </span>
    </button>
  );
}
