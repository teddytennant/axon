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
  const { data }  = useStatus();
  const location  = useLocation();
  const online    = data !== undefined;
  const peers     = data?.peer_count ?? 0;
  const page      = PAGE_LABELS[location.pathname] ?? 'axon';
  const win       = getCurrentWindow();

  return (
    <div
      className="group flex h-8 shrink-0 items-center border-b border-[#181818] bg-[#000]"
      data-tauri-drag-region
    >
      {/* Window controls — invisible until hover */}
      <div className="flex items-center gap-[5px] pl-[14px] pr-3" data-no-drag>
        <WinBtn
          onClick={() => win.close()}
          title="Close  ⌘W"
          hoverClass="group-hover:bg-[#ff5f57]"
        />
        <WinBtn
          onClick={() => win.minimize()}
          title="Minimize  ⌘M"
          hoverClass="group-hover:bg-[#febc2e]"
        />
        <WinBtn
          onClick={() => win.toggleMaximize()}
          title="Maximize"
          hoverClass="group-hover:bg-[#28c840]"
        />
      </div>

      {/* Page label — center */}
      <div className="flex flex-1 items-center justify-center" data-tauri-drag-region>
        <span className="select-none text-[9px] font-medium uppercase tracking-[0.28em] text-[#2a2a2a]">
          {page}
        </span>
      </div>

      {/* Status — right */}
      <div className="flex items-center gap-[6px] pr-[14px]" data-no-drag>
        {peers > 0 && (
          <span className="text-[9px] tabular-nums text-[#2e2e2e]">{peers}p</span>
        )}
        <span className={clsx(
          'h-[5px] w-[5px] rounded-full transition-colors duration-500',
          online ? 'bg-[#22c55e]' : 'bg-[#202020]',
        )} />
      </div>
    </div>
  );
}

function WinBtn({
  onClick,
  title,
  hoverClass,
}: {
  onClick: () => void;
  title: string;
  hoverClass: string;
}) {
  return (
    <button
      onClick={onClick}
      title={title}
      className={clsx(
        'h-[11px] w-[11px] rounded-full transition-all duration-150 active:scale-90',
        'bg-[#1e1e1e]',
        hoverClass,
      )}
    />
  );
}
