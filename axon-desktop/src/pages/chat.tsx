import { useState, useRef, useEffect, useCallback, type KeyboardEvent } from 'react';
import { clsx } from 'clsx';
import { Send, Trash2, Copy, Check } from 'lucide-react';
import { sendChatStream } from '../lib/api';
import { useConfig } from '../hooks/use-api';

interface Msg {
  role: 'user' | 'assistant' | 'system';
  content: string;
  model?: string;
  ts: string;
}

export default function ChatPage() {
  const { data: config }  = useConfig();
  const [msgs, setMsgs]   = useState<Msg[]>([]);
  const [input, setInput] = useState('');
  const [busy, setBusy]   = useState(false);
  const scrollRef         = useRef<HTMLDivElement>(null);
  const textareaRef       = useRef<HTMLTextAreaElement>(null);

  const bottom = useCallback(() => {
    if (scrollRef.current) scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
  }, []);

  useEffect(() => { bottom(); }, [msgs, bottom]);

  useEffect(() => {
    const ta = textareaRef.current;
    if (ta) { ta.style.height = 'auto'; ta.style.height = Math.min(ta.scrollHeight, 180) + 'px'; }
  }, [input]);

  const sys = (c: string) =>
    setMsgs(p => [...p, { role: 'system', content: c, ts: new Date().toISOString() }]);

  const cmd = (s: string): boolean => {
    const [c] = s.trim().split(/\s+/);
    if (c === '/clear') { setMsgs([]); return true; }
    if (c === '/help')  { sys('commands: /clear  /help'); return true; }
    return false;
  };

  const send = async () => {
    const t = input.trim();
    if (!t || busy) return;
    if (t.startsWith('/') && cmd(t)) { setInput(''); return; }
    const history = msgs.filter(m => m.role !== 'system');
    setMsgs(p => [...p, { role: 'user', content: t, ts: new Date().toISOString() }]);
    setInput('');
    setBusy(true);
    const model = config?.llm.model;
    setMsgs(p => [...p, { role: 'assistant', content: '', model, ts: new Date().toISOString() }]);
    try {
      for await (const chunk of sendChatStream({
        messages: [...history.map(m => ({ role: m.role as 'user' | 'assistant', content: m.content })), { role: 'user', content: t }],
        model,
      })) {
        setMsgs(p => {
          const u = [...p];
          const l = u[u.length - 1];
          if (l?.role === 'assistant') u[u.length - 1] = { ...l, content: l.content + chunk };
          return u;
        });
      }
    } catch (err) {
      setMsgs(p => {
        const u = [...p];
        const l = u[u.length - 1];
        if (l?.role === 'assistant' && !l.content) u[u.length - 1] = { ...l, content: String(err) };
        return u;
      });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full flex-col bg-[#000]">
      {/* Messages */}
      <div ref={scrollRef} className="flex-1 overflow-auto">
        {msgs.length === 0 ? (
          <div className="flex h-full flex-col items-center justify-center gap-4">
            <div className="text-center">
              <p className="text-[11px] text-[#222] tracking-wider">axon / chat</p>
              <p className="mt-1.5 text-[10px] text-[#1c1c1c]">/help for commands · enter to send</p>
            </div>
          </div>
        ) : (
          <div className="mx-auto flex max-w-2xl flex-col gap-0.5 py-6 px-5">
            {msgs.map((m, i) => <Bubble key={i} msg={m} />)}
            {busy && (
              <div className="flex gap-[5px] px-4 py-3">
                {[0, 140, 280].map(d => (
                  <span
                    key={d}
                    className="h-[3px] w-[3px] rounded-full bg-[#3a3a3a] animate-[pulse-dot_1.2s_ease-in-out_infinite]"
                    style={{ animationDelay: `${d}ms` }}
                  />
                ))}
              </div>
            )}
          </div>
        )}
      </div>

      {/* Input */}
      <div className="border-t border-[#181818] bg-[#000] px-4 py-3">
        <div className="mx-auto flex max-w-2xl items-end gap-2">
          <button
            onClick={() => setMsgs([])}
            className="mb-[7px] flex h-7 w-7 shrink-0 items-center justify-center rounded text-[#252525] transition-colors hover:text-[#555]"
            title="Clear chat"
          >
            <Trash2 size={13} />
          </button>

          <textarea
            ref={textareaRef}
            value={input}
            onChange={e => setInput(e.target.value)}
            onKeyDown={(e: KeyboardEvent<HTMLTextAreaElement>) => {
              if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); void send(); }
            }}
            placeholder="message…"
            rows={1}
            className="flex-1 resize-none rounded-lg border border-[#1e1e1e] bg-[#0a0a0a] px-3 py-[7px] text-[12px] text-[#ddd] placeholder-[#282828] outline-none transition-colors focus:border-[#2e2e2e] focus:bg-[#0d0d0d]"
            style={{ userSelect: 'text' }}
          />

          <button
            onClick={() => void send()}
            disabled={!input.trim() || busy}
            className={clsx(
              'mb-[7px] flex h-7 w-7 shrink-0 items-center justify-center rounded-md transition-all',
              input.trim() && !busy
                ? 'bg-white text-black hover:bg-[#e8e8e8] active:scale-95'
                : 'text-[#222]',
            )}
            title="Send  ↵"
          >
            <Send size={12} />
          </button>
        </div>

        {config?.llm.model && (
          <p className="mx-auto mt-1.5 max-w-2xl text-[9px] text-[#1e1e1e] pl-9">{config.llm.model}</p>
        )}
      </div>
    </div>
  );
}

function parseContent(content: string) {
  const segs: Array<{ type: 'text' | 'code'; content: string; lang?: string }> = [];
  for (const part of content.split(/(```[\s\S]*?```)/g)) {
    if (part.startsWith('```')) {
      const nl = part.indexOf('\n');
      segs.push({
        type: 'code',
        content: nl > -1 ? part.slice(nl + 1, -3) : part.slice(3, -3),
        lang: nl > 3 ? part.slice(3, nl).trim() : '',
      });
    } else if (part) {
      segs.push({ type: 'text', content: part });
    }
  }
  return segs;
}

function CodeBlock({ code, lang }: { code: string; lang?: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <div className="my-2 overflow-hidden rounded-lg border border-[#1e1e1e]">
      <div className="flex items-center justify-between border-b border-[#181818] bg-[#0a0a0a] px-3 py-1.5">
        <span className="text-[9px] text-[#3a3a3a] tracking-wider">{lang || 'code'}</span>
        <button
          onClick={() => { navigator.clipboard.writeText(code); setCopied(true); setTimeout(() => setCopied(false), 2000); }}
          className="text-[#2e2e2e] transition-colors hover:text-[#777]"
        >
          {copied ? <Check size={11} className="text-[#22c55e]" /> : <Copy size={11} />}
        </button>
      </div>
      <pre className="overflow-x-auto bg-[#060606] px-4 py-3">
        <code className="text-[11px] leading-relaxed text-[#bbb]">{code}</code>
      </pre>
    </div>
  );
}

function Bubble({ msg: m }: { msg: Msg }) {
  if (m.role === 'system') {
    return (
      <div className="flex justify-center py-2">
        <span className="text-[9px] text-[#2a2a2a] tracking-wider">{m.content}</span>
      </div>
    );
  }

  const isUser = m.role === 'user';
  const segs   = parseContent(m.content);

  return (
    <div className={clsx('group flex gap-3 px-1 py-2', isUser && 'flex-row-reverse')}>
      {/* Role label */}
      <span className={clsx(
        'mt-1 shrink-0 text-[9px] leading-none tracking-wider',
        isUser ? 'text-[#2e2e2e]' : 'text-[#222]',
      )}>
        {isUser ? 'you' : 'ai'}
      </span>

      {/* Content */}
      <div className={clsx(
        'max-w-[88%] text-[12px] leading-relaxed',
        isUser
          ? 'rounded-xl rounded-tr-sm border border-[#1e1e1e] bg-[#0d0d0d] px-3.5 py-2.5 text-[#ddd]'
          : 'px-0.5 text-[#b5b5b5]',
      )}>
        {!isUser && m.model && (
          <p className="mb-1.5 text-[9px] text-[#252525] tracking-wider">{m.model}</p>
        )}
        {segs.map((s, i) =>
          s.type === 'code'
            ? <CodeBlock key={i} code={s.content} lang={s.lang} />
            : <span key={i} className="whitespace-pre-wrap break-words">{s.content}</span>
        )}
      </div>
    </div>
  );
}
