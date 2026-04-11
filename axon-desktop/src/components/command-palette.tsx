import { useState, useEffect, useRef, useCallback } from 'react';
import { useNavigate } from 'react-router';

interface Cmd {
  id: string;
  label: string;
  key: string;
  to: string;
}

const COMMANDS: Cmd[] = [
  { id: 'graph',     label: 'Graph',     key: '1', to: '/'          },
  { id: 'chat',      label: 'Chat',      key: '2', to: '/chat'      },
  { id: 'mesh',      label: 'Mesh',      key: '3', to: '/mesh'      },
  { id: 'agents',    label: 'Agents',    key: '4', to: '/agents'    },
  { id: 'tasks',     label: 'Tasks',     key: '5', to: '/tasks'     },
  { id: 'workflows', label: 'Workflows', key: '6', to: '/workflows' },
  { id: 'trust',     label: 'Trust',     key: '7', to: '/trust'     },
  { id: 'settings',  label: 'Settings',  key: '8', to: '/settings'  },
];

export function CommandPalette() {
  const [open, setOpen]   = useState(false);
  const [query, setQuery] = useState('');
  const [idx, setIdx]     = useState(0);
  const inputRef          = useRef<HTMLInputElement>(null);
  const navigate          = useNavigate();

  const filtered = COMMANDS.filter(c =>
    c.label.toLowerCase().includes(query.toLowerCase()),
  );

  const exec = useCallback((cmd: Cmd) => {
    setOpen(false);
    setQuery('');
    navigate(cmd.to);
  }, [navigate]);

  // Open on Cmd+K / Ctrl+K
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

  // Focus input when opened
  useEffect(() => {
    if (open) setTimeout(() => inputRef.current?.focus(), 10);
  }, [open]);

  // Keyboard navigation
  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown') { e.preventDefault(); setIdx(i => Math.min(i + 1, filtered.length - 1)); }
    if (e.key === 'ArrowUp')   { e.preventDefault(); setIdx(i => Math.max(i - 1, 0)); }
    if (e.key === 'Enter' && filtered[idx]) exec(filtered[idx]);
  };

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center pt-[20vh]"
      onMouseDown={() => setOpen(false)}
    >
      <div
        className="w-[320px] overflow-hidden rounded border border-[#1e1e1e] bg-[#000] shadow-2xl"
        onMouseDown={e => e.stopPropagation()}
      >
        {/* Input */}
        <div className="flex items-center gap-2 border-b border-[#141414] px-3 py-2">
          <span className="font-mono text-[9px] text-[#2a2a2a]">⌘K</span>
          <input
            ref={inputRef}
            value={query}
            onChange={e => { setQuery(e.target.value); setIdx(0); }}
            onKeyDown={onKey}
            placeholder="navigate…"
            className="flex-1 bg-transparent font-mono text-[11px] text-[#888] placeholder:text-[#2a2a2a] focus:outline-none"
            style={{ userSelect: 'text' }}
          />
        </div>

        {/* Results */}
        <div className="py-1">
          {filtered.length === 0 ? (
            <p className="px-3 py-2 font-mono text-[10px] text-[#2a2a2a]">no results</p>
          ) : (
            filtered.map((cmd, i) => (
              <button
                key={cmd.id}
                onClick={() => exec(cmd)}
                onMouseEnter={() => setIdx(i)}
                className={[
                  'flex w-full items-center justify-between px-3 py-[6px] text-left transition-colors',
                  i === idx ? 'bg-[#0e0e0e]' : '',
                ].join(' ')}
              >
                <span className={`font-mono text-[11px] ${i === idx ? 'text-white' : 'text-[#444]'}`}>
                  {cmd.label}
                </span>
                <span className="font-mono text-[9px] text-[#2a2a2a]">{cmd.key}</span>
              </button>
            ))
          )}
        </div>
      </div>
    </div>
  );
}
