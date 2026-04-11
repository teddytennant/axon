import { useState, useEffect, useRef } from 'react';
import { clsx } from 'clsx';
import { Check, Loader2, ChevronRight, RefreshCw, Eye, EyeOff } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { setAxonBase, getAxonBase, getConfig, validateApiKey, putLlmConfig, getModels } from '../lib/api';
import { markOnboarded } from '../lib/onboarding-events';
import { useQueryClient } from '@tanstack/react-query';
import type { LlmConfigSection, ModelResponse } from '../lib/types';

type Step = 'connect' | 'llm' | 'done';

const PROVIDERS = [
  { id: 'ollama',      label: 'Ollama',       endpoint: 'http://localhost:11434', needsKey: false },
  { id: 'openrouter',  label: 'OpenRouter',   endpoint: 'https://openrouter.ai/api/v1', needsKey: true },
  { id: 'xai',         label: 'xAI / Grok',  endpoint: 'https://api.x.ai/v1',   needsKey: true },
  { id: 'anthropic',   label: 'Anthropic',    endpoint: 'https://api.anthropic.com', needsKey: true },
  { id: 'custom',      label: 'Custom',       endpoint: '',                        needsKey: false },
] as const;

interface Props {
  open: boolean;
  onClose: () => void;
}

export function Onboarding({ open, onClose }: Props) {
  const [step, setStep] = useState<Step>('connect');
  const qc = useQueryClient();

  useEffect(() => { if (open) setStep('connect'); }, [open]);

  if (!open) return null;

  const finish = () => {
    markOnboarded();
    void qc.invalidateQueries();
    onClose();
  };

  return (
    <div className="fixed inset-0 z-[100] flex items-center justify-center bg-black/70 backdrop-blur-[3px] animate-fade-in">
      <div className="w-[480px] overflow-hidden rounded-2xl border border-[#222] bg-[#050505] shadow-2xl animate-slide-down">
        {/* Step indicator */}
        <div className="flex items-center gap-0 border-b border-[#141414]">
          {(['connect', 'llm', 'done'] as Step[]).map((s, i) => {
            const done    = stepIndex(step) > i;
            const current = step === s;
            return (
              <div
                key={s}
                className={clsx(
                  'flex flex-1 items-center justify-center gap-1.5 py-3 text-[9px] uppercase tracking-[0.18em] transition-colors',
                  current  ? 'text-white'   : done ? 'text-[#444]' : 'text-[#1e1e1e]',
                  i < 2 && 'border-r border-[#141414]',
                )}
              >
                {done
                  ? <Check size={9} className="text-[#22c55e]" />
                  : <span className={clsx('h-[4px] w-[4px] rounded-full', current ? 'bg-white' : 'bg-[#242424]')} />
                }
                {STEP_LABELS[s]}
              </div>
            );
          })}
        </div>

        {/* Step content */}
        <div className="p-8">
          {step === 'connect' && (
            <ConnectStep onNext={() => setStep('llm')} onSkip={finish} qc={qc} />
          )}
          {step === 'llm' && (
            <LlmStep onNext={() => setStep('done')} onBack={() => setStep('connect')} />
          )}
          {step === 'done' && (
            <DoneStep onFinish={finish} />
          )}
        </div>
      </div>
    </div>
  );
}

const STEP_LABELS: Record<Step, string> = {
  connect: 'Connect',
  llm:     'LLM',
  done:    'Ready',
};

function stepIndex(s: Step) {
  return { connect: 0, llm: 1, done: 2 }[s];
}

// ── Step 1: Connect ──────────────────────────────────────────

function ConnectStep({ onNext, onSkip, qc }: { onNext: () => void; onSkip: () => void; qc: ReturnType<typeof useQueryClient> }) {
  const [url, setUrl]             = useState(getAxonBase());
  const [testing, setTesting]     = useState(false);
  const [detecting, setDetecting] = useState(false);
  const [status, setStatus]       = useState<'idle' | 'ok' | 'err'>('idle');
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => { inputRef.current?.focus(); }, []);

  async function test() {
    setTesting(true);
    setStatus('idle');
    try {
      const trimmed = url.trim();
      setAxonBase(trimmed);
      const res = await fetch(`${trimmed}/api/status`);
      if (!res.ok) throw new Error();
      setStatus('ok');
    } catch {
      setStatus('err');
    } finally {
      setTesting(false);
    }
  }

  async function autoDetect() {
    setDetecting(true);
    setStatus('idle');
    try {
      const found = await invoke<string>('probe_axon_ports');
      if (found) {
        setUrl(found);
        setAxonBase(found);
        setStatus('ok');
      } else {
        setStatus('err');
      }
    } catch {
      setStatus('err');
    } finally {
      setDetecting(false);
    }
  }

  function proceed() {
    const trimmed = url.trim();
    if (trimmed) { setAxonBase(trimmed); void qc.invalidateQueries(); }
    onNext();
  }

  return (
    <div>
      <StepHeading title="Connect to your node" subtitle="Enter the address where axon is running." />

      <div className="mt-6 space-y-3">
        <div className="flex gap-2">
          <input
            ref={inputRef}
            value={url}
            onChange={e => { setUrl(e.target.value); setStatus('idle'); }}
            onKeyDown={e => e.key === 'Enter' && void test()}
            placeholder="http://localhost:3000"
            className={clsx(
              'flex-1 rounded-lg border bg-[#080808] px-3 py-2.5 font-mono text-[12px] text-[#ccc] placeholder:text-[#252525] outline-none transition-colors',
              status === 'ok'  ? 'border-[#22c55e]/40' : status === 'err' ? 'border-[#ef4444]/40' : 'border-[#1e1e1e] focus:border-[#2e2e2e]',
            )}
            style={{ userSelect: 'text' }}
          />
          <button
            onClick={() => void test()}
            disabled={testing}
            className="flex h-[42px] items-center gap-1.5 rounded-lg border border-[#1e1e1e] bg-[#0a0a0a] px-3 text-[11px] text-[#555] transition-colors hover:border-[#2e2e2e] hover:text-[#aaa] disabled:opacity-40"
          >
            {testing ? <Loader2 size={12} className="animate-spin" /> : null}
            test
          </button>
        </div>

        {status === 'ok' && (
          <p className="flex items-center gap-1.5 text-[11px] text-[#22c55e]">
            <Check size={11} /> Connected
          </p>
        )}
        {status === 'err' && (
          <p className="text-[11px] text-[#ef4444]">Could not reach node at that address.</p>
        )}

        <button
          onClick={() => void autoDetect()}
          disabled={detecting}
          className="flex items-center gap-1.5 text-[11px] text-[#333] transition-colors hover:text-[#666]"
        >
          {detecting
            ? <Loader2 size={11} className="animate-spin" />
            : <RefreshCw size={11} />
          }
          auto-detect on common ports
        </button>
      </div>

      <div className="mt-8 flex items-center justify-between">
        <button onClick={onSkip} className="text-[10px] text-[#282828] transition-colors hover:text-[#555]">
          skip setup
        </button>
        <button
          onClick={proceed}
          className="flex items-center gap-1.5 rounded-lg bg-white px-4 py-2 text-[11px] font-medium text-black transition-all hover:bg-[#e8e8e8] active:scale-95"
        >
          Next <ChevronRight size={12} />
        </button>
      </div>

      <p className="mt-4 font-mono text-[9px] text-[#1c1c1c]">axon start --web-port 3000</p>
    </div>
  );
}

// ── Step 2: LLM ──────────────────────────────────────────────

function LlmStep({ onNext, onBack }: { onNext: () => void; onBack: () => void }) {
  const [provider,    setProvider]    = useState('ollama');
  const [endpoint,    setEndpoint]    = useState('http://localhost:11434');
  const [apiKey,      setApiKey]      = useState('');
  const [model,       setModel]       = useState('');
  const [models,      setModels]      = useState<ModelResponse[]>([]);
  const [showKey,     setShowKey]     = useState(false);
  const [validating,  setValidating]  = useState(false);
  const [keyStatus,   setKeyStatus]   = useState<'idle' | 'ok' | 'err'>('idle');
  const [saving,      setSaving]      = useState(false);
  const [loadingModels, setLoadingModels] = useState(false);

  // Load existing config
  useEffect(() => {
    getConfig().then(c => {
      setProvider(c.llm.provider || 'ollama');
      setEndpoint(c.llm.endpoint || '');
      setModel(c.llm.model || '');
      if (c.llm.api_key && c.llm.api_key !== '***') setApiKey(c.llm.api_key);
    }).catch(() => { /* node may not be connected */ });
  }, []);

  // When provider changes, reset endpoint to default
  function selectProvider(p: string) {
    setProvider(p);
    setKeyStatus('idle');
    const def = PROVIDERS.find(x => x.id === p);
    if (def && def.endpoint) setEndpoint(def.endpoint);
  }

  async function refreshModels() {
    setLoadingModels(true);
    try {
      const list = await getModels(provider);
      setModels(list);
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
      const llm: LlmConfigSection = { provider, endpoint, api_key: apiKey, model };
      await putLlmConfig(llm);
      onNext();
    } catch { /* */ }
    setSaving(false);
  }

  const needsKey = PROVIDERS.find(p => p.id === provider)?.needsKey ?? false;

  return (
    <div>
      <StepHeading title="Configure LLM" subtitle="Choose your AI provider and model." />

      <div className="mt-6 space-y-4">
        {/* Provider */}
        <Field label="Provider">
          <div className="grid grid-cols-3 gap-1.5">
            {PROVIDERS.map(p => (
              <button
                key={p.id}
                onClick={() => selectProvider(p.id)}
                className={clsx(
                  'rounded-lg border px-3 py-2 text-[10px] text-left transition-colors',
                  provider === p.id
                    ? 'border-[#383838] bg-[#111] text-white'
                    : 'border-[#1a1a1a] bg-[#080808] text-[#444] hover:border-[#252525] hover:text-[#777]',
                )}
              >
                {p.label}
              </button>
            ))}
          </div>
        </Field>

        {/* Endpoint */}
        <Field label="Endpoint">
          <input
            value={endpoint}
            onChange={e => setEndpoint(e.target.value)}
            className="w-full rounded-lg border border-[#1e1e1e] bg-[#080808] px-3 py-2 font-mono text-[11px] text-[#aaa] placeholder:text-[#252525] outline-none transition-colors focus:border-[#2e2e2e]"
            placeholder="http://localhost:11434"
            style={{ userSelect: 'text' }}
          />
        </Field>

        {/* API Key (only when needed) */}
        {needsKey && (
          <Field label="API Key">
            <div className="flex gap-2">
              <div className="relative flex-1">
                <input
                  type={showKey ? 'text' : 'password'}
                  value={apiKey}
                  onChange={e => { setApiKey(e.target.value); setKeyStatus('idle'); }}
                  placeholder="sk-…"
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
                  className="absolute right-2.5 top-1/2 -translate-y-1/2 text-[#2e2e2e] transition-colors hover:text-[#666]"
                >
                  {showKey ? <EyeOff size={12} /> : <Eye size={12} />}
                </button>
              </div>
              {apiKey && (
                <button
                  onClick={() => void validate()}
                  disabled={validating}
                  className="flex h-[38px] items-center gap-1.5 rounded-lg border border-[#1e1e1e] bg-[#0a0a0a] px-3 text-[10px] text-[#555] transition-colors hover:border-[#2e2e2e] hover:text-[#aaa] disabled:opacity-40"
                >
                  {validating
                    ? <Loader2 size={11} className="animate-spin" />
                    : keyStatus === 'ok' ? <Check size={11} className="text-[#22c55e]" /> : null
                  }
                  validate
                </button>
              )}
            </div>
            {keyStatus === 'err' && <p className="mt-1 text-[10px] text-[#ef4444]">Invalid key</p>}
          </Field>
        )}

        {/* Model */}
        <Field label="Model">
          <div className="flex gap-2">
            {models.length > 0 ? (
              <select
                value={model}
                onChange={e => setModel(e.target.value)}
                className="flex-1 rounded-lg border border-[#1e1e1e] bg-[#080808] px-3 py-2 font-mono text-[11px] text-[#aaa] outline-none transition-colors focus:border-[#2e2e2e] appearance-none cursor-pointer"
                style={{ userSelect: 'text' }}
              >
                {models.map(m => (
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
              title="Fetch available models"
              className="flex h-[38px] w-9 items-center justify-center rounded-lg border border-[#1e1e1e] bg-[#0a0a0a] text-[#333] transition-colors hover:border-[#2e2e2e] hover:text-[#777] disabled:opacity-40"
            >
              <RefreshCw size={12} className={loadingModels ? 'animate-spin' : ''} />
            </button>
          </div>
        </Field>
      </div>

      <div className="mt-8 flex items-center justify-between">
        <button onClick={onBack} className="text-[10px] text-[#282828] transition-colors hover:text-[#555]">
          ← back
        </button>
        <button
          onClick={() => void save()}
          disabled={saving}
          className="flex items-center gap-1.5 rounded-lg bg-white px-4 py-2 text-[11px] font-medium text-black transition-all hover:bg-[#e8e8e8] active:scale-95 disabled:opacity-50"
        >
          {saving ? <Loader2 size={12} className="animate-spin text-black" /> : null}
          Save &amp; continue <ChevronRight size={12} />
        </button>
      </div>
    </div>
  );
}

// ── Step 3: Done ─────────────────────────────────────────────

function DoneStep({ onFinish }: { onFinish: () => void }) {
  return (
    <div className="text-center">
      <div className="mb-6 flex justify-center">
        <div className="flex h-14 w-14 items-center justify-center rounded-2xl border border-[#1e1e1e] bg-[#0a0a0a]">
          <span className="text-[28px]">◆</span>
        </div>
      </div>

      <h2 className="text-[15px] font-medium text-white">You're all set</h2>
      <p className="mt-2 text-[11px] text-[#444]">
        Axon is connected and configured.<br />
        Press <kbd className="rounded border border-[#222] bg-[#111] px-1 py-px text-[9px] text-[#555]">1</kbd> to see the agent graph.
      </p>

      <div className="mt-8 grid grid-cols-3 gap-3 text-[9px]">
        {[
          { key: '1–8', desc: 'Navigate pages' },
          { key: '⌘K',  desc: 'Command palette' },
          { key: '2',   desc: 'Chat with agents' },
        ].map(({ key, desc }) => (
          <div key={key} className="rounded-lg border border-[#161616] bg-[#080808] px-3 py-2.5 text-center">
            <kbd className="block text-[11px] text-[#555]">{key}</kbd>
            <span className="mt-1 block text-[#2a2a2a]">{desc}</span>
          </div>
        ))}
      </div>

      <button
        onClick={onFinish}
        className="mt-8 w-full rounded-lg bg-white py-2.5 text-[12px] font-medium text-black transition-all hover:bg-[#e8e8e8] active:scale-[0.99]"
      >
        Open Axon
      </button>
    </div>
  );
}

// ── Shared UI ─────────────────────────────────────────────────

function StepHeading({ title, subtitle }: { title: string; subtitle: string }) {
  return (
    <div>
      <h2 className="text-[15px] font-medium text-white">{title}</h2>
      <p className="mt-1 text-[11px] text-[#3a3a3a]">{subtitle}</p>
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <p className="mb-1.5 text-[9px] font-medium uppercase tracking-[0.18em] text-[#2e2e2e]">{label}</p>
      {children}
    </div>
  );
}
