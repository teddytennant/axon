import { useState, useRef, useEffect, useCallback, type KeyboardEvent } from 'react';
import { clsx } from 'clsx';
import { Send, Trash2, Copy, Check } from 'lucide-react';
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
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, []);

  useEffect(() => { scrollToBottom(); }, [messages, scrollToBottom]);

  useEffect(() => {
    const ta = textareaRef.current;
    if (ta) {
      ta.style.height = 'auto';
      ta.style.height = Math.min(ta.scrollHeight, 200) + 'px';
    }
  }, [input]);

  const addSystemMsg = (content: string) => {
    setMessages((prev) => [...prev, { role: 'system', content, timestamp: new Date().toISOString() }]);
  };

  const handleSlashCommand = (cmd: string): boolean => {
    const parts = cmd.trim().split(/\s+/);
    switch (parts[0].toLowerCase()) {
      case '/clear': setMessages([]); return true;
      case '/help': addSystemMsg('Commands: /clear — clear history · /help — show this · /model — set model'); return true;
      case '/model': addSystemMsg(parts[1] ? `Model: ${parts[1]}` : 'Usage: /model <name>'); return true;
      default: return false;
    }
  };

  const handleSend = async () => {
    const trimmed = input.trim();
    if (!trimmed || streaming) return;

    if (trimmed.startsWith('/')) {
      if (handleSlashCommand(trimmed)) { setInput(''); return; }
    }

    const history = messages.filter((m) => m.role !== 'system');
    const userMsg: LocalMessage = { role: 'user', content: trimmed, timestamp: new Date().toISOString() };
    setMessages((prev) => [...prev, userMsg]);
    setInput('');
    setStreaming(true);

    const model = config?.llm.model || undefined;
    const assistantMsg: LocalMessage = { role: 'assistant', content: '', model, timestamp: new Date().toISOString() };
    setMessages((prev) => [...prev, assistantMsg]);

    try {
      const chatMessages = [
        ...history.map((m) => ({ role: m.role as 'user' | 'assistant' | 'system', content: m.content })),
        { role: 'user' as const, content: trimmed },
      ];

      for await (const chunk of sendChatStream({ messages: chatMessages, model })) {
        setMessages((prev) => {
          const updated = [...prev];
          const last = updated[updated.length - 1];
          if (last?.role === 'assistant') {
            updated[updated.length - 1] = { ...last, content: last.content + chunk };
          }
          return updated;
        });
      }
    } catch (err) {
      setMessages((prev) => {
        const updated = [...prev];
        const last = updated[updated.length - 1];
        if (last?.role === 'assistant' && !last.content) {
          updated[updated.length - 1] = { ...last, content: `Error: ${String(err)}` };
        }
        return updated;
      });
    } finally {
      setStreaming(false);
    }
  };

  const handleKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); void handleSend(); }
  };

  return (
    <div className="flex h-full flex-col">
      <div ref={scrollRef} className="flex-1 overflow-auto px-6 py-6">
        {messages.length === 0 ? (
          <div className="flex h-full flex-col items-center justify-center gap-3">
            <p className="text-sm text-[#3a3a3a]">Start a conversation</p>
            <p className="text-xs text-[#2a2a2a]">/help for commands</p>
          </div>
        ) : (
          <div className="mx-auto flex max-w-2xl flex-col gap-6">
            {messages.map((msg, i) => <MessageBubble key={i} message={msg} />)}
            {streaming && (
              <div className="flex items-center gap-1.5 pl-0">
                <span className="flex gap-0.5">
                  <span className="h-[3px] w-[3px] animate-bounce rounded-full bg-[#444]" style={{ animationDelay: '0ms' }} />
                  <span className="h-[3px] w-[3px] animate-bounce rounded-full bg-[#444]" style={{ animationDelay: '120ms' }} />
                  <span className="h-[3px] w-[3px] animate-bounce rounded-full bg-[#444]" style={{ animationDelay: '240ms' }} />
                </span>
              </div>
            )}
          </div>
        )}
      </div>

      <div className="border-t border-[#1c1c1c] bg-[#000000] p-4">
        <div className="mx-auto flex max-w-2xl items-end gap-2">
          <button
            onClick={() => setMessages([])}
            className="flex h-8 w-8 shrink-0 items-center justify-center rounded text-[#3a3a3a] transition-colors hover:bg-[#141414] hover:text-[#6b6b6b]"
            title="Clear chat"
          >
            <Trash2 size={13} />
          </button>
          <div className="relative flex-1">
            <textarea
              ref={textareaRef}
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="Message..."
              rows={1}
              className="w-full resize-none rounded border border-[#1c1c1c] bg-[#0c0c0c] px-3 py-2 text-sm text-white outline-none transition-colors placeholder:text-[#3a3a3a] hover:border-[#2a2a2a] focus:border-[#2a2a2a]"
            />
          </div>
          <button
            onClick={() => void handleSend()}
            disabled={!input.trim() || streaming}
            className={clsx(
              'flex h-8 w-8 shrink-0 items-center justify-center rounded transition-colors',
              input.trim() && !streaming
                ? 'bg-white text-black hover:bg-[#e0e0e0]'
                : 'text-[#2a2a2a] cursor-not-allowed',
            )}
          >
            <Send size={13} />
          </button>
        </div>
        <div className="mx-auto mt-1.5 flex max-w-2xl items-center gap-2 px-1">
          <span className="text-[10px] text-[#2a2a2a]">↵ send · ⇧↵ newline</span>
          {config?.llm.model && (
            <span className="ml-auto font-mono text-[10px] text-[#2a2a2a]">{config.llm.model}</span>
          )}
        </div>
      </div>
    </div>
  );
}

// ——— Helpers ———

function parseContent(content: string) {
  const segments: Array<{ type: 'text' | 'code'; content: string; lang?: string }> = [];
  const parts = content.split(/(```[\s\S]*?```)/g);
  for (const part of parts) {
    if (part.startsWith('```')) {
      const firstNewline = part.indexOf('\n');
      const lang = firstNewline > 3 ? part.slice(3, firstNewline).trim() : '';
      const code = firstNewline > -1 ? part.slice(firstNewline + 1, -3) : part.slice(3, -3);
      segments.push({ type: 'code', content: code, lang });
    } else if (part) {
      segments.push({ type: 'text', content: part });
    }
  }
  return segments;
}

function CodeBlock({ code, lang }: { code: string; lang?: string }) {
  const [copied, setCopied] = useState(false);
  const copy = () => {
    navigator.clipboard.writeText(code);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };
  return (
    <div className="my-2 overflow-hidden rounded border border-[#1c1c1c]">
      <div className="flex items-center justify-between border-b border-[#1c1c1c] bg-[#0c0c0c] px-3 py-1.5">
        <span className="font-mono text-[10px] text-[#3a3a3a]">{lang || 'code'}</span>
        <button
          onClick={copy}
          className="text-[#3a3a3a] transition-colors hover:text-[#6b6b6b]"
        >
          {copied ? <Check size={11} className="text-[#22c55e]" /> : <Copy size={11} />}
        </button>
      </div>
      <pre className="overflow-x-auto bg-[#060606] px-4 py-3">
        <code className="font-mono text-[11px] leading-relaxed text-[#cccccc]">{code}</code>
      </pre>
    </div>
  );
}

function MessageBubble({ message }: { message: LocalMessage }) {
  const isUser = message.role === 'user';
  const isSystem = message.role === 'system';
  const segments = isSystem ? null : parseContent(message.content);

  if (isSystem) {
    return (
      <div className="flex justify-center">
        <div className="rounded border border-[#1c1c1c] px-3 py-1 text-[10px] text-[#3a3a3a]">
          {message.content}
        </div>
      </div>
    );
  }

  return (
    <div className={clsx('flex flex-col gap-1', isUser ? 'items-end' : 'items-start')}>
      <span className="text-[10px] text-[#2a2a2a]">
        {isUser ? 'you' : 'assistant'}
      </span>
      <div className={clsx(
        'max-w-[82%] rounded px-4 py-3 text-sm leading-relaxed',
        isUser
          ? 'bg-[#111111] border border-[#1c1c1c] text-white'
          : 'text-[#cccccc]',
      )}>
        {!isUser && message.model && (
          <p className="mb-2 font-mono text-[10px] text-[#2a2a2a]">{message.model}</p>
        )}
        <div>
          {segments?.map((seg, i) =>
            seg.type === 'code' ? (
              <CodeBlock key={i} code={seg.content} lang={seg.lang} />
            ) : (
              <span key={i} className="whitespace-pre-wrap break-words">{seg.content}</span>
            )
          )}
        </div>
        <p className="mt-2 text-[9px] text-[#2a2a2a]">
          {new Date(message.timestamp).toLocaleTimeString()}
        </p>
      </div>
    </div>
  );
}
