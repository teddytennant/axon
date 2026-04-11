import { useEffect, useState } from 'react';
import { NavLink, useNavigate } from 'react-router';
import { clsx } from 'clsx';
import {
  Share2, MessageSquare, Network, Bot, ListTodo,
  GitBranch, Shield, Settings,
} from 'lucide-react';

const navItems = [
  { to: '/',          icon: Share2,        label: 'Graph',     key: '1' },
  { to: '/chat',      icon: MessageSquare, label: 'Chat',      key: '2' },
  { to: '/mesh',      icon: Network,       label: 'Mesh',      key: '3' },
  { to: '/agents',    icon: Bot,           label: 'Agents',    key: '4' },
  { to: '/tasks',     icon: ListTodo,      label: 'Tasks',     key: '5' },
  { to: '/workflows', icon: GitBranch,     label: 'Workflows', key: '6' },
  { to: '/trust',     icon: Shield,        label: 'Trust',     key: '7' },
  { to: '/settings',  icon: Settings,      label: 'Settings',  key: '8' },
] as const;

export function Sidebar() {
  const [collapsed, setCollapsed] = useState(false);
  const navigate = useNavigate();

  useEffect(() => {
    const down = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      const n = parseInt(e.key);
      if (n >= 1 && n <= 8) navigate(navItems[n - 1]!.to);
      if (e.key === '[') setCollapsed(c => !c);
    };
    window.addEventListener('keydown', down);
    return () => window.removeEventListener('keydown', down);
  }, [navigate]);

  return (
    <aside
      className={clsx(
        'flex flex-col h-screen bg-[#0a0a0a] border-r border-[#1a1a1a] shrink-0 transition-[width] duration-150',
        collapsed ? 'w-10' : 'w-40',
      )}
    >
      {/* wordmark / drag region */}
      <div
        className="flex items-center h-9 px-3 border-b border-[#1a1a1a] shrink-0"
        data-tauri-drag-region
      >
        <button
          onClick={() => setCollapsed(c => !c)}
          className="flex items-center gap-2 w-full text-left"
          title="Toggle sidebar  [ "
        >
          <span className="text-[10px] font-bold tracking-[0.25em] text-white">
            {collapsed ? 'A' : 'AXON'}
          </span>
        </button>
      </div>

      {/* nav */}
      <nav className="flex-1 py-1 overflow-y-auto">
        {navItems.map(({ to, icon: Icon, label, key }) => (
          <NavLink
            key={to}
            to={to}
            end={to === '/'}
            className={({ isActive }) => clsx(
              'relative flex items-center gap-2.5 px-3 py-[7px] group transition-colors',
              isActive ? 'text-white' : 'text-[#3a3a3a] hover:text-[#999]',
              collapsed && 'justify-center px-0',
            )}
          >
            {({ isActive }) => (
              <>
                {isActive && (
                  <span className="absolute left-0 inset-y-1 w-[2px] bg-white rounded-r" />
                )}
                <Icon
                  size={13}
                  strokeWidth={isActive ? 2 : 1.5}
                  className="shrink-0"
                />
                {!collapsed && (
                  <>
                    <span className="flex-1 text-[11px]">{label}</span>
                    <span className={clsx(
                      'text-[9px] tabular-nums',
                      isActive ? 'text-[#444]' : 'text-[#252525] group-hover:text-[#3a3a3a]',
                    )}>
                      {key}
                    </span>
                  </>
                )}
              </>
            )}
          </NavLink>
        ))}
      </nav>

      {/* footer */}
      {!collapsed && (
        <div className="px-3 py-2 border-t border-[#1a1a1a]">
          <p className="text-[9px] text-[#222]">v0.1  <span className="text-[#1a1a1a]">[ collapse</span></p>
        </div>
      )}
    </aside>
  );
}
