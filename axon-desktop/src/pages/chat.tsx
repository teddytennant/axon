/**
 * Desktop chat page — identical to web version but uses the desktop API client.
 */
import { useState, useRef, useEffect, useCallback, type KeyboardEvent } from 'react';
import { clsx } from 'clsx';
import { Send, Trash2, Bot, User, Copy, Check } from 'lucide-react';
import { sendChatStream } from '../lib/api';
import { useConfig } from '../hooks/use-api';

interface LocalMessage {
  role: 'user' | 'assistant' | 'system';
  content: string;
  model?: string;
  timestamp: string;
}

export default function ChatPage() {
  const { data: config } = useConfig();
  const [messages, setMessages] = useState<LocalMessage[]>([]);
  const [input, setInput] = useState('');
  const [streaming, setStreaming] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const scrollToBottom = useCallback(() => {
    if (scrollRef.current) scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
  }, []);

  useEffect(() => { scrollToBottom(); }, [messages, scrollToBottom]);

  useEffect(() => {
    const ta = textareaRef.current;
    if (ta) { ta.style.height = 'auto'; ta.style.height = Math.min(ta.scrollHeight, 200) + 'px'; }
  }, [input]);

  const addSystem = (content: string) =>
    setMessages(p => [...p, { role: 'system', content, timestamp: new Date().toISOString() }]);

  const handleCommand = (cmd: string): boolean => {
    const [c] = cmd.trim().split(/\s+/);
    if (c === '/clear') { setMessages([]); return true; }
    if (c === '/help') { addSystem('Commands: /clear · /help'); return true; }
    return false;
  };

  const send = async () => {
    const trimmed = input.trim();
    if (!trimmed || streaming) return;
    if (trimmed.startsWith('/') && handleCommand(trimmed)) { setInput(''); return; }

    const history = messages.filter(m => m.role !== 'system');
    setMessages(p => [...p, { role: 'user', content: trimmed, timestamp: new Date().toISOString() }]);
    setInput('');
    setStreaming(true);
    const model = config?.llm.model;
    setMessages(p => [...p, { role: 'assistant', content: '', model, timestamp: new Date().toISOString() }]);

    try {
      for await (const chunk of sendChatStream({ messages: [...history.map(m => ({ role: m.role as 'user'|'assistant', content: m.content })), { role: 'user', content: trimmed }], model })) {
        setMessages(p => { const u = [...p]; const l = u[u.length - 1]; if (l?.role === 'assistant') u[u.length - 1] = { ...l, content: l.content + chunk }; return u; });
      }
    } catch (err) {
      setMessages(p => { const u = [...p]; const l = u[u.length - 1]; if (l?.role === 'assistant' && !l.content) u[u.length - 1] = { ...l, content: String(err) }; return u; });
    } finally {
      setStreaming(false);
    }
  };

  return (
    <div className="flex h-full flex-col bg-[#07070d]">
      <div ref={scrollRef} className="flex-1 overflow-auto px-5 py-4">
        {messages.length === 0 ? (
          <div className="flex h-full flex-col items-center justify-center gap-4">
            <div className="flex h-10 w-10 items-center justify-center rounded-xl bg-[#00c8c8]/10">
              <Bot size={18} className="text-[#00c8c8]" />
            </div>
            <p className="text-xs text-[#3a3a58]">Start a conversation · /help for commands</p>
          </div>
        ) : (
          <div className="mx-auto flex max-w-2xl flex-col gap-4">
            {messages.map((m, i) => <Bubble key={i} message={m} />)}
            {streaming && (
              <div className="flex items-center gap-1.5 pl-9">
                {[0, 120, 240].map(d => (
                  <span key={d} className="h-1 w-1 animate-bounce rounded-full bg-[#00c8c8]/60" style={{ animationDelay: `${d}ms` }} />
                ))}
              </div>
            )}
          </div>
        )}
      </div>

      <div className="border-t border-[#141424] bg-[#0a0a12] p-3">
        <div className="mx-auto flex max-w-2xl items-end gap-2">
          <button onClick={() => setMessages([])} className="flex h-8 w-8 items-center justify-center rounded-lg text-[#2e2e4a] hover:bg-[#141424] hover:text-[#6868a0] transition-colors">
            <Trash2 size={14} />
          </button>
          <textarea
            ref={textareaRef}
            value={input}
            onChange={e => setInput(e.target.value)}
            onKeyDown={(e: KeyboardEvent<HTMLTextAreaElement>) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); void send(); } }}
            placeholder="Message…"
            rows={1}
            className="flex-1 resize-none rounded-xl border border-[#1a1a2a] bg-[#0e0e18] px-3 py-2 text-xs text-[#e0e0f4] outline-none transition-colors placeholder:text-[#2e2e4a] focus:border-[#00c8c8]/30"
          />
          <button
            onClick={() => void send()}
            disabled={!input.trim() || streaming}
            className={clsx('flex h-8 w-8 items-center justify-center rounded-lg transition-colors', input.trim() && !streaming ? 'bg-[#00c8c8] text-[#07070d] hover:bg-[#00a8a8]' : 'bg-[#141424] text-[#2e2e4a]')}
          >
            <Send size={13} />
          </button>
        </div>
        {config?.llm.model && (
          <p className="mx-auto mt-1 max-w-2xl font-mono text-[8px] text-[#1e1e30]">{config.llm.model}</p>
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
      segs.push({ type: 'code', content: nl > -1 ? part.slice(nl + 1, -3) : part.slice(3, -3), lang: nl > 3 ? part.slice(3, nl).trim() : '' });
    } else if (part) {
      segs.push({ type: 'text', content: part });
    }
  }
  return segs;
}

function CodeBlock({ code, lang }: { code: string; lang?: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <div className="my-1.5 overflow-hidden rounded-lg border border-[#141424]">
      <div className="flex items-center justify-between bg-[#0e0e18] px-3 py-1">
        <span className="font-mono text-[8px] text-[#3a3a58]">{lang || 'code'}</span>
        <button onClick={() => { navigator.clipboard.writeText(code); setCopied(true); setTimeout(() => setCopied(false), 2000); }} className="text-[#2e2e4a] hover:text-[#6868a0]">
          {copied ? <Check size={10} className="text-[#50dc78]" /> : <Copy size={10} />}
        </button>
      </div>
      <pre className="overflow-x-auto bg-[#08080f] px-3 py-2">
        <code className="font-mono text-[10px] leading-relaxed text-[#c8c8e8]">{code}</code>
      </pre>
    </div>
  );
}

function Bubble({ message: m }: { message: LocalMessage }) {
  const isUser = m.role === 'user';
  const isSystem = m.role === 'system';

  if (isSystem) {
    return (
      <div className="flex justify-center">
        <div className="rounded-full border border-[#141424] px-3 py-0.5 font-mono text-[9px] text-[#3a3a58]">{m.content}</div>
      </div>
    );
  }

  const segs = parseContent(m.content);
  return (
    <div className={clsx('flex items-start gap-2.5', isUser && 'flex-row-reverse')}>
      <div className={clsx('flex h-6 w-6 shrink-0 items-center justify-center rounded-lg', isUser ? 'bg-[#00c8c8]/10 text-[#00c8c8]' : 'bg-[#141424] text-[#4a4a70]')}>
        {isUser ? <User size={11} /> : <Bot size={11} />}
      </div>
      <div className={clsx('max-w-[80%] rounded-xl px-3 py-2.5 text-xs leading-relaxed', isUser ? 'bg-[#00c8c8]/8 text-[#e0e0f4]' : 'bg-[#0e0e18] text-[#e0e0f4]')}>
        {!isUser && m.model && <p className="mb-1 font-mono text-[8px] text-[#2e2e4a]">{m.model}</p>}
        {segs.map((s, i) => s.type === 'code' ? <CodeBlock key={i} code={s.content} lang={s.lang} /> : <span key={i} className="whitespace-pre-wrap break-words">{s.content}</span>)}
        <p className={clsx('mt-1 text-[8px]', isUser ? 'text-right text-[#00c8c8]/30' : 'text-[#1e1e30]')}>{new Date(m.timestamp).toLocaleTimeString()}</p>
      </div>
    </div>
  );
}
