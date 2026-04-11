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
      <div ref={scrollRef} className="flex-1 overflow-auto px-6 py-4">
        {messages.length === 0 ? (
          <div className="flex h-full flex-col items-center justify-center gap-4">
            <div className="flex h-12 w-12 items-center justify-center rounded-2xl bg-[#00c8c8]/10">
              <Bot size={22} className="text-[#00c8c8]" />
            </div>
            <div className="text-center">
              <p className="text-sm font-medium text-[#888]">Start a conversation</p>
              <p className="mt-1 text-xs text-[#444]">Type below · /help for commands</p>
            </div>
          </div>
        ) : (
          <div className="mx-auto flex max-w-3xl flex-col gap-5">
            {messages.map((msg, i) => <MessageBubble key={i} message={msg} />)}
            {streaming && (
              <div className="flex items-center gap-2 pl-11 text-xs text-[#444]">
                <span className="flex gap-0.5">
                  <span className="h-1 w-1 animate-bounce rounded-full bg-[#00c8c8]" style={{ animationDelay: '0ms' }} />
                  <span className="h-1 w-1 animate-bounce rounded-full bg-[#00c8c8]" style={{ animationDelay: '120ms' }} />
                  <span className="h-1 w-1 animate-bounce rounded-full bg-[#00c8c8]" style={{ animationDelay: '240ms' }} />
                </span>
              </div>
            )}
          </div>
        )}
      </div>

      <div className="border-t border-[#1a1a1a] bg-[#0a0a0a] p-4">
        <div className="mx-auto flex max-w-3xl items-end gap-2.5">
          <button
            onClick={() => setMessages([])}
            className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg text-[#444] transition-colors hover:bg-[#141414] hover:text-[#666]"
            title="Clear chat"
          >
            <Trash2 size={15} />
          </button>
          <div className="relative flex-1">
            <textarea
              ref={textareaRef}
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="Message..."
              rows={1}
              className="w-full resize-none rounded-xl border border-[#222] bg-[#111] px-4 py-2.5 text-sm text-[#f5f5f5] outline-none transition-colors placeholder:text-[#444] focus:border-[#00c8c8]/30 focus:ring-1 focus:ring-[#00c8c8]/20"
            />
          </div>
          <button
            onClick={() => void handleSend()}
            disabled={!input.trim() || streaming}
            className={clsx(
              'flex h-9 w-9 shrink-0 items-center justify-center rounded-lg transition-colors',
              input.trim() && !streaming
                ? 'bg-[#00c8c8] text-[#0a0a0a] hover:bg-[#00b0b0]'
                : 'bg-[#141414] text-[#444] cursor-not-allowed',
            )}
          >
            <Send size={15} />
          </button>
        </div>
        <div className="mx-auto mt-1.5 flex max-w-3xl items-center gap-2 px-1">
          <span className="text-[10px] text-[#333]">Enter to send · Shift+Enter for newline</span>
          {config?.llm.model && (
            <span className="ml-auto font-mono text-[10px] text-[#333]">{config.llm.model}</span>
          )}
        </div>
      </div>
    </div>
  );
}

// ——— Helpers ———

function parseContent(content: string) {
  // Split on fenced code blocks
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
    <div className="my-2 overflow-hidden rounded-lg border border-[#1a1a1a]">
      <div className="flex items-center justify-between bg-[#141414] px-3 py-1.5">
        <span className="font-mono text-[10px] text-[#555]">{lang || 'code'}</span>
        <button
          onClick={copy}
          className="text-[#444] transition-colors hover:text-[#888]"
        >
          {copied ? <Check size={12} className="text-[#50dc78]" /> : <Copy size={12} />}
        </button>
      </div>
      <pre className="overflow-x-auto bg-[#0d0d0d] px-4 py-3">
        <code className="font-mono text-[11px] leading-relaxed text-[#d4d4d4]">{code}</code>
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
        <div className="rounded-full border border-[#1a1a1a] bg-[#111] px-3 py-1 text-[10px] text-[#555]">
          {message.content}
        </div>
      </div>
    );
  }

  return (
    <div className={clsx('flex items-start gap-3', isUser && 'flex-row-reverse')}>
      {/* Avatar */}
      <div className={clsx(
        'flex h-7 w-7 shrink-0 items-center justify-center rounded-lg',
        isUser ? 'bg-[#00c8c8]/10 text-[#00c8c8]' : 'bg-[#181818] text-[#666]',
      )}>
        {isUser ? <User size={13} /> : <Bot size={13} />}
      </div>

      {/* Bubble */}
      <div className={clsx(
        'max-w-[80%] rounded-xl px-4 py-3 text-sm leading-relaxed',
        isUser
          ? 'bg-[#00c8c8]/10 text-[#f5f5f5]'
          : 'bg-[#111] text-[#f5f5f5]',
      )}>
        {!isUser && message.model && (
          <p className="mb-1 font-mono text-[10px] text-[#444]">{message.model}</p>
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
        <p className={clsx('mt-1.5 text-[9px]', isUser ? 'text-[#00c8c8]/40 text-right' : 'text-[#333]')}>
          {new Date(message.timestamp).toLocaleTimeString()}
        </p>
      </div>
    </div>
  );
}
