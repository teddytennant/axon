import { useEffect } from 'react';
import { NavLink, useNavigate } from 'react-router';
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
  const navigate = useNavigate();

  useEffect(() => {
    const down = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      const n = parseInt(e.key);
      if (n >= 1 && n <= 8) navigate(navItems[n - 1]!.to);
    };
    window.addEventListener('keydown', down);
    return () => window.removeEventListener('keydown', down);
  }, [navigate]);

  return (
    <aside className="flex w-10 shrink-0 flex-col border-r border-[#111] bg-[#000]">
      {/* Logo mark */}
      <div
        className="flex h-[30px] items-center justify-center border-b border-[#111] shrink-0"
        data-tauri-drag-region
      >
        <span className="select-none font-mono text-[8px] font-bold tracking-[0.3em] text-[#222]">A</span>
      </div>

      {/* Nav icons */}
      <nav className="flex flex-1 flex-col py-1">
        {navItems.map(({ to, icon: Icon, label, key }) => (
          <NavLink
            key={to}
            to={to}
            end={to === '/'}
            title={`${label}  ${key}`}
            className={({ isActive }) =>
              [
                'relative flex h-9 items-center justify-center transition-colors',
                isActive
                  ? 'text-white'
                  : 'text-[#2a2a2a] hover:text-[#666]',
              ].join(' ')
            }
          >
            {({ isActive }) => (
              <>
                {isActive && (
                  <span className="absolute left-0 inset-y-[6px] w-[2px] rounded-r bg-white" />
                )}
                <Icon
                  size={12}
                  strokeWidth={isActive ? 2 : 1.5}
                  className="shrink-0"
                />
              </>
            )}
          </NavLink>
        ))}
      </nav>
    </aside>
  );
}
