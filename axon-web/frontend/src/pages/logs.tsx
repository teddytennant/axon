import { useState, useEffect, useRef, useCallback } from 'react';
import { clsx } from 'clsx';
import { ScrollText } from 'lucide-react';
import { useWebSocket } from '../hooks/use-websocket';

type LogLevel = 'all' | 'warn' | 'error';

interface LogLine {
  id: number;
  text: string;
  level: 'error' | 'warn' | 'info' | 'debug';
}

let logId = 0;

function detectLevel(text: string): LogLine['level'] {
  const t = text.toUpperCase();
  if (t.includes('ERROR') || t.includes(' ERR ') || t.includes('[ERR]')) return 'error';
  if (t.includes('WARN') || t.includes('[WARN]')) return 'warn';
  if (t.includes('DEBUG') || t.includes('[DEBUG]') || t.includes('[TRACE]')) return 'debug';
  return 'info';
}

export default function LogsPage() {
  const { subscribe } = useWebSocket();
  const [logs, setLogs] = useState<LogLine[]>([]);
  const [filter, setFilter] = useState<LogLevel>('all');
  const scrollRef = useRef<HTMLDivElement>(null);
  const autoScrollRef = useRef(true);

  const handleScroll = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
    autoScrollRef.current = atBottom;
  }, []);

  useEffect(() => {
    return subscribe('log', (data) => {
      const text = String(data);
      const level = detectLevel(text);
      setLogs((prev) => {
        const next = [...prev, { id: ++logId, text, level }];
        return next.length > 500 ? next.slice(-500) : next;
      });
    });
  }, [subscribe]);

  useEffect(() => {
    if (autoScrollRef.current && scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [logs]);

  const filtered = logs.filter((log) => {
    if (filter === 'all') return true;
    if (filter === 'warn') return log.level === 'warn' || log.level === 'error';
    return log.level === 'error';
  });

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-3 border-b border-[#222] px-6 py-3">
        <h1 className="text-lg font-semibold text-[#f5f5f5]">Logs</h1>
        <span className="font-mono text-xs text-[#555]">{filtered.length} lines</span>
        <div className="ml-auto flex gap-1.5">
          {(['all', 'warn', 'error'] as const).map((level) => (
            <button
              key={level}
              onClick={() => setFilter(level)}
              className={clsx(
                'rounded-lg px-3 py-1 text-xs font-medium uppercase transition-colors',
                filter === level ? 'bg-[#181818] text-[#f5f5f5]' : 'text-[#555] hover:text-[#888]',
              )}
            >
              {level === 'all' ? 'ALL' : level === 'warn' ? 'WARN+' : 'ERR'}
            </button>
          ))}
        </div>
      </div>

      <div
        ref={scrollRef}
        onScroll={handleScroll}
        className="flex-1 overflow-auto p-4 font-mono text-xs"
      >
        {filtered.length === 0 ? (
          <div className="flex h-full flex-col items-center justify-center">
            <ScrollText size={32} className="mb-3 text-[#555]" />
            <p className="font-sans text-sm text-[#555]">
              {logs.length === 0 ? 'Waiting for log events...' : 'No matching logs'}
            </p>
          </div>
        ) : (
          <div className="flex flex-col">
            {filtered.map((log) => (
              <LogEntry key={log.id} log={log} />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

const levelColor: Record<LogLine['level'], string> = {
  error: 'text-[#f05050]',
  warn: 'text-[#f0c83c]',
  info: 'text-[#888]',
  debug: 'text-[#444]',
};

function LogEntry({ log }: { log: LogLine }) {
  return (
    <div className={clsx('py-0.5 leading-5 hover:bg-[#111]', levelColor[log.level])}>
      {log.text}
    </div>
  );
}
