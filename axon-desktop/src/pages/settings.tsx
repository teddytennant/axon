import { useState, useEffect } from 'react';
import { clsx } from 'clsx';
import { useQueryClient } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Check, Loader2, Copy, RefreshCw, Eye, EyeOff, Wand2 } from 'lucide-react';
import { setAxonBase, getAxonBase, putLlmConfig, validateApiKey, getModels } from '../lib/api';
import { useStatus, useConfig, useModels } from '../hooks/use-api';
import { onboardingEvents } from '../lib/onboarding-events';
import type { LlmConfigSection, ModelResponse } from '../lib/types';

const PROVIDERS = [
  { id: 'ollama',     label: 'Ollama',      endpoint: 'http://localhost:11434',       needsKey: false },
  { id: 'openrouter', label: 'OpenRouter',  endpoint: 'https://openrouter.ai/api/v1', needsKey: true  },
  { id: 'xai',        label: 'xAI / Grok', endpoint: 'https://api.x.ai/v1',          needsKey: true  },
  { id: 'anthropic',  label: 'Anthropic',   endpoint: 'https://api.anthropic.com',    needsKey: true  },
  { id: 'custom',     label: 'Custom',      endpoint: '',                             needsKey: false },
] as const;

export default function SettingsPage() {
  return (
    <div className="h-full overflow-auto">
      <div className="mx-auto max-w-xl p-6">
        <div className="mb-8 flex items-center justify-between">
          <h1 className="text-[11px] font-medium uppercase tracking-[0.25em] text-[#444]">Settings</h1>
          <button
            onClick={() => onboardingEvents.open()}
            className="flex items-center gap-1.5 rounded-lg border border-[#1a1a1a] bg-[#080808] px-3 py-1.5 text-[10px] text-[#444] transition-colors hover:border-[#252525] hover:text-[#888]"
          >
            <Wand2 size={11} />
            Setup wizard
          </button>
        </div>

        <div className="space-y-8">
          <ConnectionSection />
          <Divider />
          <NodeSection />
          <Divider />
          <LlmSection />
          <Divider />
          <McpSection />
          <Divider />
          <KeyboardSection />
        </div>
      </div>
    </div>
  );
}

// ── Connection ───────────────────────────────────────────────

function ConnectionSection() {
  const [url, setUrl]         = useState(getAxonBase());
  const [probing, setProbing] = useState(false);
  const [saved, setSaved]     = useState(false);
  const qc = useQueryClient();
  const { data: status, isError, isFetching } = useStatus();
  const online = !!status && !isError;

  async function autoProbe() {
    setProbing(true);
    try {
      const found = await invoke<string>('probe_axon_ports');
      if (found && found !== getAxonBase()) {
        setUrl(found);
        toast.success(`Node found at ${found}`);
      } else if (!found) {
        toast.error('No node found on common ports');
      }
    } catch { /* invoke may fail in dev */ }
    setProbing(false);
  }

  function save() {
    const trimmed = url.trim();
    if (!trimmed) return;
    setAxonBase(trimmed);
    void qc.invalidateQueries();
    setSaved(true);
    setTimeout(() => setSaved(false), 1500);
  }

  return (
    <section>
      <SectionHeader label="Connection" />

      <div className="space-y-3">
        <div className="flex items-center gap-2">
          <input
            value={url}
            onChange={e => setUrl(e.target.value)}
            onKeyDown={e => e.key === 'Enter' && save()}
            className="flex-1 rounded-lg border border-[#1e1e1e] bg-[#080808] px-3 py-2 font-mono text-[11px] text-[#aaa] placeholder:text-[#252525] outline-none transition-colors focus:border-[#2e2e2e] focus:bg-[#0c0c0c]"
            placeholder="http://localhost:3000"
            spellCheck={false}
            style={{ userSelect: 'text' }}
          />
          <ActionButton onClick={save} loading={false} done={saved} label="connect" doneLabel="saved" />
          <button
            onClick={() => void autoProbe()}
            disabled={probing}
            title="Auto-detect node"
            className="flex h-[38px] w-9 items-center justify-center rounded-lg border border-[#1e1e1e] bg-[#080808] text-[#333] transition-colors hover:border-[#252525] hover:text-[#777] disabled:opacity-40"
          >
            <RefreshCw size={12} className={probing ? 'animate-spin' : ''} />
          </button>
        </div>

        <StatusPill
          loading={isFetching}
          online={online}
          onlineLabel={`connected — ${status?.listen_addr ?? ''}`}
          offlineLabel="not connected"
        />

        <p className="font-mono text-[9px] text-[#1a1a1a]">axon start --web-port 3000</p>
      </div>
    </section>
  );
}

// ── Node identity ────────────────────────────────────────────

function NodeSection() {
  const { data: status } = useStatus();
  const [copied, setCopied] = useState(false);

  if (!status) return null;

  function copy(t: string) {
    navigator.clipboard.writeText(t);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  }

  const uptimeSecs = status.uptime_secs;
  const uptime = uptimeSecs < 60
    ? `${uptimeSecs}s`
    : uptimeSecs < 3600
    ? `${Math.floor(uptimeSecs / 60)}m ${uptimeSecs % 60}s`
    : `${Math.floor(uptimeSecs / 3600)}h ${Math.floor((uptimeSecs % 3600) / 60)}m`;

  return (
    <section>
      <SectionHeader label="Node" />
      <div className="space-y-2">
        <InfoRow label="Peer ID">
          <div className="flex items-center gap-2">
            <span className="flex-1 truncate font-mono text-[10px] text-[#666]">{status.peer_id}</span>
            <button onClick={() => copy(status.peer_id)} className="shrink-0 text-[#252525] transition-colors hover:text-[#666]">
              {copied ? <Check size={11} className="text-[#22c55e]" /> : <Copy size={11} />}
            </button>
          </div>
        </InfoRow>
        <InfoRow label="Listen">
          <span className="font-mono text-[10px] text-[#666]">{status.listen_addr}</span>
        </InfoRow>
        <InfoRow label="Version">
          <span className="font-mono text-[10px] text-[#555]">{status.version}</span>
        </InfoRow>
        <InfoRow label="Uptime">
          <span className="font-mono text-[10px] text-[#555]">{uptime}</span>
        </InfoRow>
        <InfoRow label="Provider">
          <span className="font-mono text-[10px] text-[#555]">{status.provider} / {status.model}</span>
        </InfoRow>
      </div>
    </section>
  );
}

// ── LLM config ───────────────────────────────────────────────

function LlmSection() {
  const { data: config, refetch } = useConfig();
  const qc = useQueryClient();

  const [provider,   setProvider]   = useState('ollama');
  const [endpoint,   setEndpoint]   = useState('http://localhost:11434');
  const [apiKey,     setApiKey]     = useState('');
  const [model,      setModel]      = useState('');
  const [showKey,    setShowKey]    = useState(false);
  const [validating, setValidating] = useState(false);
  const [keyStatus,  setKeyStatus]  = useState<'idle' | 'ok' | 'err'>('idle');
  const [saving,     setSaving]     = useState(false);
  const [saved,      setSaved]      = useState(false);
  const [loadingModels, setLoadingModels] = useState(false);
  const [modelList,  setModelList]  = useState<ModelResponse[]>([]);

  useEffect(() => {
    if (!config) return;
    setProvider(config.llm.provider || 'ollama');
    setEndpoint(config.llm.endpoint || '');
    setModel(config.llm.model || '');
    // api_key is masked as "***" by server — keep blank so user can set a new one
  }, [config]);

  function selectProvider(p: string) {
    setProvider(p);
    setKeyStatus('idle');
    const def = PROVIDERS.find(x => x.id === p);
    if (def?.endpoint) setEndpoint(def.endpoint);
  }

  async function refreshModels() {
    setLoadingModels(true);
    try {
      const list = await getModels(provider);
      setModelList(list);
      if (list.length && !model) setModel(list[0]?.id ?? '');
    } catch { /* */ }
    setLoadingModels(false);
  }

  async function validate() {
    setValidating(true);
    setKeyStatus('idle');
    try {
      const r = await validateApiKey(provider, apiKey);
      setKeyStatus(r.valid ? 'ok' : 'err');
    } catch {
      setKeyStatus('err');
    }
    setValidating(false);
  }

  async function save() {
    setSaving(true);
    try {
      const llm: LlmConfigSection = {
        provider,
        endpoint,
        api_key: apiKey || '***', // keep existing key if blank
        model,
      };
      await putLlmConfig(llm);
      void refetch();
      void qc.invalidateQueries({ queryKey: ['config'] });
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
      toast.success('LLM config saved');
    } catch (e) {
      toast.error(`Save failed: ${String(e)}`);
    }
    setSaving(false);
  }

  const needsKey = PROVIDERS.find(p => p.id === provider)?.needsKey ?? false;
  const hasExistingKey = config?.llm.api_key === '***';

  return (
    <section>
      <SectionHeader label="LLM" />

      <div className="space-y-4">
        {/* Provider pills */}
        <div>
          <FieldLabel>Provider</FieldLabel>
          <div className="mt-1.5 flex flex-wrap gap-1.5">
            {PROVIDERS.map(p => (
              <button
                key={p.id}
                onClick={() => selectProvider(p.id)}
                className={clsx(
                  'rounded-md border px-2.5 py-1.5 text-[10px] transition-colors',
                  provider === p.id
                    ? 'border-[#383838] bg-[#111] text-white'
                    : 'border-[#1a1a1a] bg-[#080808] text-[#444] hover:border-[#252525] hover:text-[#777]',
                )}
              >
                {p.label}
              </button>
            ))}
          </div>
        </div>

        {/* Endpoint */}
        <div>
          <FieldLabel>Endpoint</FieldLabel>
          <input
            value={endpoint}
            onChange={e => setEndpoint(e.target.value)}
            className="mt-1.5 w-full rounded-lg border border-[#1e1e1e] bg-[#080808] px-3 py-2 font-mono text-[11px] text-[#aaa] placeholder:text-[#252525] outline-none transition-colors focus:border-[#2e2e2e]"
            style={{ userSelect: 'text' }}
          />
        </div>

        {/* API Key */}
        {needsKey && (
          <div>
            <FieldLabel>
              API Key
              {hasExistingKey && <span className="ml-2 text-[9px] text-[#2e2e2e]">(key saved — enter new to replace)</span>}
            </FieldLabel>
            <div className="mt-1.5 flex gap-2">
              <div className="relative flex-1">
                <input
                  type={showKey ? 'text' : 'password'}
                  value={apiKey}
                  onChange={e => { setApiKey(e.target.value); setKeyStatus('idle'); }}
                  placeholder={hasExistingKey ? '••••••••' : 'sk-…'}
                  className={clsx(
                    'w-full rounded-lg border bg-[#080808] px-3 py-2 pr-8 font-mono text-[11px] text-[#aaa] placeholder:text-[#252525] outline-none transition-colors',
                    keyStatus === 'ok'  ? 'border-[#22c55e]/40' :
                    keyStatus === 'err' ? 'border-[#ef4444]/40' :
                    'border-[#1e1e1e] focus:border-[#2e2e2e]',
                  )}
                  style={{ userSelect: 'text' }}
                />
                <button
                  onClick={() => setShowKey(v => !v)}
                  className="absolute right-2.5 top-1/2 -translate-y-1/2 text-[#2e2e2e] transition-colors hover:text-[#555]"
                >
                  {showKey ? <EyeOff size={12} /> : <Eye size={12} />}
                </button>
              </div>
              {apiKey && (
                <ActionButton
                  onClick={() => void validate()}
                  loading={validating}
                  done={keyStatus === 'ok'}
                  label="validate"
                  doneLabel="valid"
                  error={keyStatus === 'err'}
                />
              )}
            </div>
            {keyStatus === 'err' && <p className="mt-1 text-[10px] text-[#ef4444]">Invalid key</p>}
          </div>
        )}

        {/* Model */}
        <div>
          <FieldLabel>Model</FieldLabel>
          <div className="mt-1.5 flex gap-2">
            {modelList.length > 0 ? (
              <select
                value={model}
                onChange={e => setModel(e.target.value)}
                className="flex-1 rounded-lg border border-[#1e1e1e] bg-[#080808] px-3 py-2 font-mono text-[11px] text-[#aaa] outline-none transition-colors focus:border-[#2e2e2e] cursor-pointer appearance-none"
                style={{ userSelect: 'text' }}
              >
                {modelList.map(m => (
                  <option key={m.id} value={m.id}>{m.name || m.id}</option>
                ))}
              </select>
            ) : (
              <input
                value={model}
                onChange={e => setModel(e.target.value)}
                placeholder="llama4-maverick"
                className="flex-1 rounded-lg border border-[#1e1e1e] bg-[#080808] px-3 py-2 font-mono text-[11px] text-[#aaa] placeholder:text-[#252525] outline-none transition-colors focus:border-[#2e2e2e]"
                style={{ userSelect: 'text' }}
              />
            )}
            <button
              onClick={() => void refreshModels()}
              disabled={loadingModels}
              title="Fetch models from provider"
              className="flex h-[38px] w-9 items-center justify-center rounded-lg border border-[#1e1e1e] bg-[#080808] text-[#333] transition-colors hover:border-[#252525] hover:text-[#777] disabled:opacity-40"
            >
              <RefreshCw size={12} className={loadingModels ? 'animate-spin' : ''} />
            </button>
          </div>
        </div>

        {/* Save */}
        <div className="flex justify-end pt-1">
          <ActionButton
            onClick={() => void save()}
            loading={saving}
            done={saved}
            label="Save LLM settings"
            doneLabel="Saved"
            primary
          />
        </div>
      </div>
    </section>
  );
}

// ── MCP servers ──────────────────────────────────────────────

function McpSection() {
  const { data: config } = useConfig();
  const servers = config?.mcp.servers ?? [];

  return (
    <section>
      <SectionHeader label="MCP Servers" />

      {servers.length === 0 ? (
        <p className="text-[11px] text-[#1e1e1e]">
          No MCP servers configured.{' '}
          <span className="font-mono text-[10px] text-[#1a1a1a]">~/.config/axon/config.toml</span>
        </p>
      ) : (
        <div className="space-y-2">
          {servers.map(s => (
            <div key={s.name} className="flex items-center justify-between rounded-lg border border-[#1a1a1a] bg-[#080808] px-3 py-2.5">
              <div className="min-w-0">
                <p className="text-[11px] text-[#888]">{s.name}</p>
                <p className="mt-0.5 truncate font-mono text-[9px] text-[#2e2e2e]">
                  {s.command} {s.args.join(' ')}
                </p>
              </div>
              <span className="ml-3 shrink-0 font-mono text-[9px] text-[#2a2a2a]">{s.timeout_secs}s</span>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

// ── Keyboard shortcuts ───────────────────────────────────────

function KeyboardSection() {
  return (
    <section>
      <SectionHeader label="Keyboard" />
      <div className="space-y-2">
        {[
          { key: '⌘K',  desc: 'Command palette' },
          { key: '1–8', desc: 'Navigate to page' },
        ].map(({ key, desc }) => (
          <div key={key} className="flex items-center justify-between">
            <span className="text-[11px] text-[#333]">{desc}</span>
            <kbd className="rounded-md border border-[#1a1a1a] bg-[#0a0a0a] px-2 py-1 font-mono text-[9px] text-[#484848]">
              {key}
            </kbd>
          </div>
        ))}
      </div>
    </section>
  );
}

// ── Shared UI ─────────────────────────────────────────────────

function SectionHeader({ label }: { label: string }) {
  return (
    <p className="mb-4 text-[9px] font-medium uppercase tracking-[0.2em] text-[#363636]">{label}</p>
  );
}

function FieldLabel({ children }: { children: React.ReactNode }) {
  return (
    <p className="text-[9px] font-medium uppercase tracking-[0.15em] text-[#2a2a2a]">{children}</p>
  );
}

function Divider() {
  return <div className="border-t border-[#0f0f0f]" />;
}

function InfoRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-4 rounded-md px-3 py-1.5 odd:bg-[#050505]">
      <span className="shrink-0 text-[9px] uppercase tracking-[0.12em] text-[#2a2a2a]">{label}</span>
      <div className="min-w-0 text-right">{children}</div>
    </div>
  );
}

function StatusPill({ loading, online, onlineLabel, offlineLabel }: {
  loading: boolean;
  online: boolean;
  onlineLabel: string;
  offlineLabel: string;
}) {
  return (
    <div className={clsx(
      'flex items-center gap-2 rounded-md px-2.5 py-1.5 text-[10px] w-fit',
      online ? 'bg-[rgba(34,197,94,0.05)]' : 'bg-[#080808]',
    )}>
      {loading
        ? <Loader2 size={9} className="animate-spin text-[#333]" />
        : <span className={clsx('h-[5px] w-[5px] rounded-full', online ? 'bg-[#22c55e]' : 'bg-[#2a2a2a]')} />
      }
      <span className={online ? 'text-[#555]' : 'text-[#2e2e2e]'}>
        {online ? onlineLabel : offlineLabel}
      </span>
    </div>
  );
}

function ActionButton({
  onClick, loading, done, label, doneLabel, error, primary,
}: {
  onClick: () => void;
  loading: boolean;
  done: boolean;
  label: string;
  doneLabel: string;
  error?: boolean;
  primary?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      disabled={loading}
      className={clsx(
        'flex items-center gap-1.5 rounded-lg px-3 text-[10px] transition-all active:scale-95 disabled:opacity-40 h-[38px]',
        primary
          ? 'bg-white text-black hover:bg-[#e8e8e8] font-medium'
          : 'border border-[#1e1e1e] bg-[#080808] text-[#555] hover:border-[#2e2e2e] hover:text-[#aaa]',
        error && 'border-[#ef4444]/40 text-[#ef4444]',
      )}
    >
      {loading
        ? <Loader2 size={11} className={clsx('animate-spin', primary ? 'text-black' : '')} />
        : done
        ? <Check size={11} className={primary ? 'text-black' : 'text-[#22c55e]'} />
        : null
      }
      {done ? doneLabel : label}
    </button>
  );
}
