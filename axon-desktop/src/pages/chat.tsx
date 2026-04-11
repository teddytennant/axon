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
      {/* messages */}
      <div ref={scrollRef} className="flex-1 overflow-auto px-4 py-4">
        {msgs.length === 0 ? (
          <div className="flex h-full items-center justify-center">
            <p className="text-[11px] text-[#2a2a2a]">start typing  ·  /help</p>
          </div>
        ) : (
          <div className="mx-auto flex max-w-xl flex-col gap-3">
            {msgs.map((m, i) => <Bubble key={i} msg={m} />)}
            {busy && (
              <div className="flex gap-1 pl-3">
                {[0, 150, 300].map(d => (
                  <span
                    key={d}
                    className="h-[3px] w-[3px] rounded-full bg-[#444] animate-[pulse-dot_1s_ease-in-out_infinite]"
                    style={{ animationDelay: `${d}ms` }}
                  />
                ))}
              </div>
            )}
          </div>
        )}
      </div>

      {/* input */}
      <div className="border-t border-[#1a1a1a] bg-[#000] p-3">
        <div className="mx-auto flex max-w-xl items-end gap-2">
          <button
            onClick={() => setMsgs([])}
            className="flex h-7 w-7 items-center justify-center text-[#2a2a2a] hover:text-[#666] transition-colors"
            title="Clear"
          >
            <Trash2 size={12} />
          </button>
          <textarea
            ref={textareaRef}
            value={input}
            onChange={e => setInput(e.target.value)}
            onKeyDown={(e: KeyboardEvent<HTMLTextAreaElement>) => {
              if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); void send(); }
            }}
            placeholder="message"
            rows={1}
            className="flex-1 resize-none rounded border border-[#1f1f1f] bg-[#111] px-3 py-[7px] text-[11px] text-[#eee] placeholder-[#2a2a2a] outline-none focus:border-[#333] transition-colors"
          />
          <button
            onClick={() => void send()}
            disabled={!input.trim() || busy}
            className={clsx(
              'flex h-7 w-7 items-center justify-center rounded transition-colors',
              input.trim() && !busy
                ? 'bg-white text-black hover:bg-[#ddd]'
                : 'text-[#2a2a2a]',
            )}
            title="Send  ↵"
          >
            <Send size={11} />
          </button>
        </div>
        {config?.llm.model && (
          <p className="mx-auto mt-1.5 max-w-xl text-[9px] text-[#222]">{config.llm.model}</p>
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
    <div className="my-1.5 overflow-hidden rounded border border-[#1f1f1f]">
      <div className="flex items-center justify-between bg-[#111] px-3 py-1 border-b border-[#1a1a1a]">
        <span className="text-[9px] text-[#333]">{lang || 'code'}</span>
        <button
          onClick={() => { navigator.clipboard.writeText(code); setCopied(true); setTimeout(() => setCopied(false), 2000); }}
          className="text-[#333] hover:text-[#888] transition-colors"
        >
          {copied ? <Check size={10} className="text-[#22c55e]" /> : <Copy size={10} />}
        </button>
      </div>
      <pre className="overflow-x-auto bg-[#0d0d0d] px-3 py-2.5">
        <code className="text-[10px] leading-relaxed text-[#ccc]">{code}</code>
      </pre>
    </div>
  );
}

function Bubble({ msg: m }: { msg: Msg }) {
  if (m.role === 'system') {
    return (
      <div className="flex justify-center">
        <span className="text-[9px] text-[#333]">{m.content}</span>
      </div>
    );
  }

  const isUser = m.role === 'user';
  const segs   = parseContent(m.content);

  return (
    <div className={clsx('flex items-start gap-2', isUser && 'flex-row-reverse')}>
      <span className={clsx(
        'mt-0.5 shrink-0 text-[9px] tabular-nums',
        isUser ? 'text-[#333]' : 'text-[#2a2a2a]',
      )}>
        {isUser ? 'you' : 'ai'}
      </span>
      <div className={clsx(
        'max-w-[85%] rounded px-3 py-2 text-[11px] leading-relaxed',
        isUser
          ? 'bg-[#161616] border border-[#222] text-[#eee]'
          : 'bg-transparent text-[#ccc]',
      )}>
        {!isUser && m.model && (
          <p className="mb-1 text-[9px] text-[#2a2a2a]">{m.model}</p>
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
