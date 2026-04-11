import { useState } from 'react';
import { NavLink } from 'react-router';
import { clsx } from 'clsx';
import {
  Share2, MessageSquare, Network, Bot, ListTodo,
  GitBranch, Shield, Settings, PanelLeft, PanelLeftClose,
} from 'lucide-react';

const navItems = [
  { to: '/', icon: Share2, label: 'Graph', primary: true },
  { to: '/chat', icon: MessageSquare, label: 'Chat' },
  { to: '/mesh', icon: Network, label: 'Mesh' },
  { to: '/agents', icon: Bot, label: 'Agents' },
  { to: '/tasks', icon: ListTodo, label: 'Tasks' },
  { to: '/workflows', icon: GitBranch, label: 'Workflows' },
  { to: '/trust', icon: Shield, label: 'Trust' },
  { to: '/settings', icon: Settings, label: 'Settings' },
] as const;

export function Sidebar() {
  const [collapsed, setCollapsed] = useState(false);

  return (
    <aside
      className={clsx(
        'flex flex-col h-screen border-r border-[#141424] bg-[#0a0a12] transition-all duration-200 shrink-0',
        collapsed ? 'w-12' : 'w-52',
      )}
    >
      {/* Logo */}
      <div
        className="flex items-center justify-between h-11 px-3 border-b border-[#141424]"
        data-tauri-drag-region
      >
        {!collapsed && (
          <div className="flex items-center gap-2">
            <div className="flex h-5 w-5 items-center justify-center rounded bg-[#00c8c8]/10">
              <Share2 size={10} className="text-[#00c8c8]" strokeWidth={2.5} />
            </div>
            <span className="font-mono text-xs font-bold tracking-widest text-[#00c8c8]">AXON</span>
          </div>
        )}
        <button
          onClick={() => setCollapsed(c => !c)}
          className="p-1 rounded text-[#2e2e4a] hover:text-[#6868a0] hover:bg-[#141424] transition-colors"
        >
          {collapsed ? <PanelLeft size={14} /> : <PanelLeftClose size={14} />}
        </button>
      </div>

      {/* Nav */}
      <nav className="flex-1 py-2 px-1.5 space-y-px overflow-y-auto">
        {navItems.map(({ to, icon: Icon, label, primary }) => (
          <NavLink
            key={to}
            to={to}
            end={to === '/'}
            className={({ isActive }) => clsx(
              'relative flex items-center gap-2.5 rounded-md px-2.5 py-1.5 text-xs transition-colors',
              isActive
                ? primary
                  ? 'bg-[#00c8c8]/10 text-[#00c8c8]'
                  : 'bg-[#00c8c8]/8 text-[#00c8c8]/80'
                : 'text-[#3a3a58] hover:text-[#8888b0] hover:bg-[#141424]',
              collapsed && 'justify-center px-0',
            )}
          >
            {({ isActive }) => (
              <>
                {isActive && !collapsed && (
                  <span className="absolute left-0 top-1 bottom-1 w-0.5 rounded-full bg-[#00c8c8]/70" />
                )}
                <Icon size={14} className="shrink-0" strokeWidth={isActive ? 2.2 : 1.8} />
                {!collapsed && <span className={isActive ? 'font-semibold' : ''}>{label}</span>}
              </>
            )}
          </NavLink>
        ))}
      </nav>

      {/* Version */}
      {!collapsed && (
        <div className="border-t border-[#141424] p-3">
          <p className="font-mono text-[8px] text-[#1e1e30]">axon desktop v0.1</p>
        </div>
      )}
    </aside>
  );
}
