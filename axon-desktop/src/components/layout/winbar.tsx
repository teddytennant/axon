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
      className="flex h-[30px] shrink-0 items-center border-b border-[#111] bg-[#000]"
      data-tauri-drag-region
    >
      {/* Traffic light window controls */}
      <div className="flex items-center gap-[5px] pl-[14px] pr-3" data-no-drag>
        <button
          onClick={() => win.close()}
          className="h-[11px] w-[11px] rounded-full bg-[#ff5f56] transition-opacity hover:opacity-70 active:opacity-50"
          title="Close  ⌘W"
        />
        <button
          onClick={() => win.minimize()}
          className="h-[11px] w-[11px] rounded-full bg-[#febc2e] transition-opacity hover:opacity-70 active:opacity-50"
          title="Minimize  ⌘M"
        />
        <button
          onClick={() => win.toggleMaximize()}
          className="h-[11px] w-[11px] rounded-full bg-[#28c840] transition-opacity hover:opacity-70 active:opacity-50"
          title="Maximize"
        />
      </div>

      {/* Page label — center */}
      <div className="flex flex-1 items-center justify-center" data-tauri-drag-region>
        <span className="select-none font-mono text-[9px] uppercase tracking-[0.2em] text-[#2a2a2a]">
          {page}
        </span>
      </div>

      {/* Status — right */}
      <div className="flex items-center gap-[7px] pr-3" data-no-drag>
        {peers > 0 && (
          <span className="font-mono text-[9px] tabular-nums text-[#2a2a2a]">
            {peers}p
          </span>
        )}
        <span
          className={clsx(
            'h-[4px] w-[4px] rounded-full',
            online ? 'bg-[#22c55e]' : 'bg-[#333]',
          )}
        />
      </div>
    </div>
  );
}
