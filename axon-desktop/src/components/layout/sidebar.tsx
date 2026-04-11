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
    <aside className="flex w-11 shrink-0 flex-col border-r border-[#181818] bg-[#000]">
      {/* Logo */}
      <div
        className="flex h-8 shrink-0 items-center justify-center border-b border-[#181818]"
        data-tauri-drag-region
      >
        <span className="select-none text-[8px] font-bold tracking-[0.35em] text-[#252525]">A</span>
      </div>

      {/* Nav */}
      <nav className="flex flex-1 flex-col py-2">
        {navItems.map(({ to, icon: Icon, label, key }) => (
          <NavLink
            key={to}
            to={to}
            end={to === '/'}
            title={`${label}  ${key}`}
            className={({ isActive }) =>
              [
                'relative flex h-9 items-center justify-center transition-colors duration-100',
                isActive
                  ? 'text-white'
                  : 'text-[#2c2c2c] hover:text-[#5a5a5a]',
              ].join(' ')
            }
          >
            {({ isActive }) => (
              <>
                {isActive && (
                  <span className="absolute left-0 inset-y-[10px] w-[2px] rounded-r-full bg-white opacity-90" />
                )}
                <Icon
                  size={14}
                  strokeWidth={isActive ? 1.75 : 1.5}
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
