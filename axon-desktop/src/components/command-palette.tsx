import { useState, useEffect, useRef, useCallback } from 'react';
import { useNavigate } from 'react-router';

interface Cmd {
  id: string;
  label: string;
  key: string;
  to: string;
  desc?: string;
}

const COMMANDS: Cmd[] = [
  { id: 'graph',     label: 'Graph',     key: '1', to: '/',          desc: 'Force-directed mesh view' },
  { id: 'chat',      label: 'Chat',      key: '2', to: '/chat',      desc: 'LLM proxy interface'      },
  { id: 'mesh',      label: 'Mesh',      key: '3', to: '/mesh',      desc: 'Connected peers'          },
  { id: 'agents',    label: 'Agents',    key: '4', to: '/agents',    desc: 'Registered agents'        },
  { id: 'tasks',     label: 'Tasks',     key: '5', to: '/tasks',     desc: 'Task log & stats'         },
  { id: 'workflows', label: 'Workflows', key: '6', to: '/workflows', desc: 'Orchestration runs'       },
  { id: 'trust',     label: 'Trust',     key: '7', to: '/trust',     desc: 'Peer reputation scores'   },
  { id: 'settings',  label: 'Settings',  key: '8', to: '/settings',  desc: 'Node configuration'      },
];

export function CommandPalette() {
  const [open, setOpen]   = useState(false);
  const [query, setQuery] = useState('');
  const [idx, setIdx]     = useState(0);
  const inputRef          = useRef<HTMLInputElement>(null);
  const navigate          = useNavigate();

  const filtered = COMMANDS.filter(c =>
    c.label.toLowerCase().includes(query.toLowerCase()) ||
    (c.desc ?? '').toLowerCase().includes(query.toLowerCase()),
  );

  const exec = useCallback((cmd: Cmd) => {
    setOpen(false);
    setQuery('');
    navigate(cmd.to);
  }, [navigate]);

  useEffect(() => {
    const down = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
        e.preventDefault();
        setOpen(o => !o);
        setQuery('');
        setIdx(0);
      }
      if (e.key === 'Escape') setOpen(false);
    };
    window.addEventListener('keydown', down);
    return () => window.removeEventListener('keydown', down);
  }, []);

  useEffect(() => {
    if (open) setTimeout(() => inputRef.current?.focus(), 10);
  }, [open]);

  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown') { e.preventDefault(); setIdx(i => Math.min(i + 1, filtered.length - 1)); }
    if (e.key === 'ArrowUp')   { e.preventDefault(); setIdx(i => Math.max(i - 1, 0)); }
    if (e.key === 'Enter' && filtered[idx]) exec(filtered[idx]);
  };

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/40 pt-[18vh] backdrop-blur-[2px]"
      onMouseDown={() => setOpen(false)}
    >
      <div
        className="w-[340px] overflow-hidden rounded-lg border border-[#222] bg-[#050505] shadow-2xl animate-slide-down"
        onMouseDown={e => e.stopPropagation()}
      >
        {/* Input */}
        <div className="flex items-center gap-2.5 border-b border-[#161616] px-3.5 py-2.5">
          <span className="shrink-0 text-[9px] text-[#2a2a2a] tracking-widest">⌘K</span>
          <input
            ref={inputRef}
            value={query}
            onChange={e => { setQuery(e.target.value); setIdx(0); }}
            onKeyDown={onKey}
            placeholder="go to…"
            className="flex-1 bg-transparent text-[12px] text-[#999] placeholder:text-[#2a2a2a] focus:outline-none"
            style={{ userSelect: 'text' }}
          />
        </div>

        {/* Results */}
        <div className="py-1">
          {filtered.length === 0 ? (
            <p className="px-4 py-3 text-[11px] text-[#2a2a2a]">no results</p>
          ) : (
            filtered.map((cmd, i) => (
              <button
                key={cmd.id}
                onClick={() => exec(cmd)}
                onMouseEnter={() => setIdx(i)}
                className={[
                  'flex w-full items-center justify-between px-3.5 py-2 text-left transition-colors',
                  i === idx ? 'bg-[#0e0e0e]' : '',
                ].join(' ')}
              >
                <div className="flex items-baseline gap-3 min-w-0">
                  <span className={`text-[12px] shrink-0 ${i === idx ? 'text-white' : 'text-[#444]'}`}>
                    {cmd.label}
                  </span>
                  {cmd.desc && (
                    <span className={`truncate text-[10px] ${i === idx ? 'text-[#555]' : 'text-[#262626]'}`}>
                      {cmd.desc}
                    </span>
                  )}
                </div>
                <span className={`shrink-0 text-[9px] ml-2 ${i === idx ? 'text-[#444]' : 'text-[#222]'}`}>
                  {cmd.key}
                </span>
              </button>
            ))
          )}
        </div>

        {/* Footer */}
        <div className="border-t border-[#111] px-3.5 py-2 flex items-center gap-3">
          <span className="text-[9px] text-[#222]">↑↓ navigate</span>
          <span className="text-[9px] text-[#222]">↵ select</span>
          <span className="text-[9px] text-[#222]">esc close</span>
        </div>
      </div>
    </div>
  );
}
