import { useState, useRef, useEffect, useCallback, type KeyboardEvent } from 'react';
import { Send, Copy, Check } from 'lucide-react';
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
    if (ta) { ta.style.height = 'auto'; ta.style.height = Math.min(ta.scrollHeight, 160) + 'px'; }
  }, [input]);

  const sys = (c: string) =>
    setMsgs(p => [...p, { role: 'system', content: c, ts: new Date().toISOString() }]);

  const cmd = (s: string): boolean => {
    const [c] = s.trim().split(/\s+/);
    if (c === '/clear') { setMsgs([]); return true; }
    if (c === '/help')  { sys('/clear  /help'); return true; }
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
        messages: [
          ...history.map(m => ({ role: m.role as 'user' | 'assistant', content: m.content })),
          { role: 'user', content: t },
        ],
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

  const isEmpty = msgs.length === 0;

  return (
    <div className="flex h-full flex-col bg-[#000]">
      {/* Messages */}
      <div ref={scrollRef} className="flex-1 overflow-auto">
        {isEmpty ? (
          <div className="flex h-full items-center justify-center">
            <span className="text-[10px] tracking-[0.2em] text-[#1e1e1e] uppercase">axon chat</span>
          </div>
        ) : (
          <div className="mx-auto max-w-2xl space-y-1 py-8 px-6">
            {msgs.map((m, i) => <Bubble key={i} msg={m} />)}
            {busy && (
              <div className="flex gap-[5px] py-4 pl-0.5">
                {[0, 130, 260].map(d => (
                  <span
                    key={d}
                    className="h-[3px] w-[3px] rounded-full bg-[#303030] animate-[pulse-dot_1.2s_ease-in-out_infinite]"
                    style={{ animationDelay: `${d}ms` }}
                  />
                ))}
              </div>
            )}
          </div>
        )}
      </div>

      {/* Input */}
      <div className="px-6 pb-5 pt-3">
        <div className="mx-auto max-w-2xl">
          <div className="flex items-end gap-2 rounded-2xl border border-[#181818] bg-[#080808] pl-4 pr-2 py-2 focus-within:border-[#232323] transition-colors">
            <textarea
              ref={textareaRef}
              value={input}
              onChange={e => setInput(e.target.value)}
              onKeyDown={(e: KeyboardEvent<HTMLTextAreaElement>) => {
                if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); void send(); }
              }}
              placeholder="message"
              rows={1}
              className="flex-1 resize-none bg-transparent py-1 text-[12px] text-[#d8d8d8] placeholder:text-[#282828] outline-none leading-relaxed"
              style={{ userSelect: 'text' }}
            />
            <button
              onClick={() => void send()}
              disabled={!input.trim() || busy}
              className="mb-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-xl bg-white text-black transition-all hover:bg-[#e8e8e8] active:scale-90 disabled:bg-[#111] disabled:text-[#2a2a2a]"
            >
              <Send size={11} />
            </button>
          </div>

          {/* Clear + model hint */}
          <div className="mt-2 flex items-center justify-between px-1">
            <button
              onClick={() => setMsgs([])}
              className="text-[9px] text-[#1e1e1e] transition-colors hover:text-[#444]"
            >
              clear
            </button>
            {config?.llm.model && (
              <span className="text-[9px] text-[#1c1c1c]">{config.llm.model}</span>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

// ── Message rendering ─────────────────────────────────────────

function parseContent(content: string) {
  const segs: Array<{ type: 'text' | 'code'; content: string; lang?: string }> = [];
  for (const part of content.split(/(```[\s\S]*?```)/g)) {
    if (part.startsWith('```')) {
      const nl = part.indexOf('\n');
      segs.push({
        type: 'code',
        content: nl > -1 ? part.slice(nl + 1, -3) : part.slice(3, -3),
        lang: nl > 3 ? part.slice(3, nl).trim() : undefined,
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
    <div className="group/code my-3 overflow-hidden rounded-xl bg-[#070707]">
      <div className="flex items-center justify-between px-4 py-2">
        <span className="text-[9px] text-[#2a2a2a] tracking-wider">{lang ?? 'code'}</span>
        <button
          onClick={() => { navigator.clipboard.writeText(code); setCopied(true); setTimeout(() => setCopied(false), 2000); }}
          className="opacity-0 group-hover/code:opacity-100 transition-opacity text-[#2e2e2e] hover:text-[#666]"
        >
          {copied ? <Check size={11} className="text-[#22c55e]" /> : <Copy size={11} />}
        </button>
      </div>
      <pre className="overflow-x-auto px-4 pb-4">
        <code className="text-[11px] leading-relaxed text-[#aaa]">{code}</code>
      </pre>
    </div>
  );
}

function Bubble({ msg: m }: { msg: Msg }) {
  if (m.role === 'system') {
    return (
      <div className="flex justify-center py-3">
        <span className="text-[9px] text-[#222] tracking-widest">{m.content}</span>
      </div>
    );
  }

  const isUser = m.role === 'user';
  const segs   = parseContent(m.content);

  if (isUser) {
    return (
      <div className="flex justify-end py-1">
        <div className="max-w-[75%] rounded-2xl rounded-br-sm bg-[#111] px-4 py-2.5 text-[12px] leading-relaxed text-[#ddd]">
          {segs.map((s, i) =>
            s.type === 'code'
              ? <CodeBlock key={i} code={s.content} lang={s.lang} />
              : <span key={i} className="whitespace-pre-wrap break-words">{s.content}</span>
          )}
        </div>
      </div>
    );
  }

  // assistant
  return (
    <div className="py-1">
      <div className="text-[12px] leading-relaxed text-[#999]">
        {segs.map((s, i) =>
          s.type === 'code'
            ? <CodeBlock key={i} code={s.content} lang={s.lang} />
            : <span key={i} className="whitespace-pre-wrap break-words">{s.content}</span>
        )}
      </div>
    </div>
  );
}
