import { useState, useRef, useEffect, useCallback, type KeyboardEvent } from 'react';
import { clsx } from 'clsx';
import { Send, Trash2, HelpCircle, Sparkles } from 'lucide-react';
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
      case '/help': addSystemMsg('Commands: /clear — clear history  /help — show this'); return true;
      case '/model': addSystemMsg(parts[1] ? `Model set to: ${parts[1]}` : 'Usage: /model <name>'); return true;
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
          updated[updated.length - 1] = { ...last, content: String(err) };
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
      <div ref={scrollRef} className="flex-1 overflow-auto p-6">
        {messages.length === 0 ? (
          <div className="flex h-full flex-col items-center justify-center gap-3">
            <Sparkles size={32} className="text-[#555]" />
            <p className="text-sm text-[#555]">Start a conversation</p>
            <p className="text-xs text-[#333]">Type a message below · /help for commands</p>
          </div>
        ) : (
          <div className="mx-auto flex max-w-3xl flex-col gap-4">
            {messages.map((msg, i) => <MessageBubble key={i} message={msg} />)}
            {streaming && (
              <div className="flex items-center gap-2 text-xs text-[#555]">
                <div className="h-1.5 w-1.5 animate-pulse rounded-full bg-[#00c8c8]" />
                Generating...
              </div>
            )}
          </div>
        )}
      </div>

      <div className="border-t border-[#222] bg-[#0a0a0a] p-4">
        <div className="mx-auto flex max-w-3xl items-end gap-3">
          <button
            onClick={() => setMessages([])}
            className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg text-[#555] transition-colors hover:bg-[#181818] hover:text-[#888]"
            title="Clear chat"
          >
            <Trash2 size={16} />
          </button>
          <div className="relative flex-1">
            <textarea
              ref={textareaRef}
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="Send a message..."
              rows={1}
              className="w-full resize-none rounded-lg border border-[#222] bg-[#111] px-4 py-2.5 text-sm text-[#f5f5f5] outline-none transition-colors placeholder:text-[#555] focus:border-[#333]"
            />
          </div>
          <button
            onClick={() => void handleSend()}
            disabled={!input.trim() || streaming}
            className={clsx(
              'flex h-10 w-10 shrink-0 items-center justify-center rounded-lg transition-colors',
              input.trim() && !streaming ? 'bg-[#00c8c8] text-[#0a0a0a] hover:bg-[#00a8a8]' : 'bg-[#181818] text-[#555]',
            )}
          >
            <Send size={16} />
          </button>
        </div>
        <div className="mx-auto mt-2 flex max-w-3xl items-center gap-3">
          <button onClick={() => handleSlashCommand('/help')} className="flex items-center gap-1 text-[10px] text-[#555] hover:text-[#888]">
            <HelpCircle size={10} />/help
          </button>
          <span className="text-[10px] text-[#333]">Shift+Enter for newline</span>
          {config?.llm.model && (
            <span className="ml-auto font-mono text-[10px] text-[#555]">{config.llm.model}</span>
          )}
        </div>
      </div>
    </div>
  );
}

function MessageBubble({ message }: { message: LocalMessage }) {
  const isUser = message.role === 'user';
  const isSystem = message.role === 'system';
  return (
    <div className={clsx('flex', isUser ? 'justify-end' : 'justify-start')}>
      <div className={clsx(
        'max-w-[85%] rounded-lg px-4 py-3 text-sm leading-relaxed',
        isUser && 'bg-[#00c8c8]/10 text-[#f5f5f5]',
        !isUser && !isSystem && 'bg-[#111] text-[#f5f5f5]',
        isSystem && 'border border-[#222] bg-[#181818] text-[#888]',
      )}>
        {!isUser && !isSystem && message.model && (
          <p className="mb-1.5 font-mono text-[10px] text-[#555]">{message.model}</p>
        )}
        <div className="whitespace-pre-wrap break-words">{message.content}</div>
        <p className="mt-1.5 text-[10px] text-[#555]">
          {new Date(message.timestamp).toLocaleTimeString()}
        </p>
      </div>
    </div>
  );
}
