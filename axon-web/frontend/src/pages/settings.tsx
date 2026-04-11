import { useState } from 'react';
import { useConfig } from '../hooks/use-api';
import { updateConfig, updateLlmConfig, setKey, validateKey } from '../lib/api';
import { useQueryClient } from '@tanstack/react-query';
import { toast } from 'sonner';
import { Card, CardHeader, CardTitle } from '../components/ui/card';
import { Input } from '../components/ui/input';
import { Button } from '../components/ui/button';
import { Badge } from '../components/ui/badge';
import { Skeleton } from '../components/ui/skeleton';
import { Save, Key, CheckCircle2 } from 'lucide-react';

export default function SettingsPage() {
  const { data: config, isLoading } = useConfig();
  const queryClient = useQueryClient();

  const [listen, setListen] = useState('');
  const [provider, setProvider] = useState('');
  const [endpoint, setEndpoint] = useState('');
  const [model, setModel] = useState('');
  const [apiKey, setApiKey] = useState('');
  const [validating, setValidating] = useState(false);
  const [saving, setSaving] = useState(false);
  const [initialized, setInitialized] = useState(false);

  if (config && !initialized) {
    setListen(config.node.listen);
    setProvider(config.llm.provider);
    setEndpoint(config.llm.endpoint);
    setModel(config.llm.model);
    setInitialized(true);
  }

  const saveNodeConfig = async () => {
    setSaving(true);
    try {
      await updateConfig({ node: { ...config!.node, listen } });
      await queryClient.invalidateQueries({ queryKey: ['config'] });
      toast.success('Node config saved');
    } catch {
      toast.error('Failed to save node config');
    } finally {
      setSaving(false);
    }
  };

  const saveLlmConfig = async () => {
    setSaving(true);
    try {
      await updateLlmConfig({ provider, endpoint, api_key: config?.llm.api_key ?? '', model });
      await queryClient.invalidateQueries({ queryKey: ['config'] });
      toast.success('LLM config saved');
    } catch {
      toast.error('Failed to save LLM config');
    } finally {
      setSaving(false);
    }
  };

  const handleSetKey = async () => {
    if (!apiKey.trim()) return;
    setValidating(true);
    try {
      const { valid } = await validateKey(provider, apiKey);
      if (valid) {
        await setKey(provider, apiKey);
        await queryClient.invalidateQueries({ queryKey: ['config'] });
        setApiKey('');
        toast.success('API key saved and validated');
      } else {
        toast.error('Invalid API key');
      }
    } catch {
      toast.error('Failed to validate key');
    } finally {
      setValidating(false);
    }
  };

  if (isLoading) {
    return (
      <div className="max-w-xl space-y-4 p-6">
        <Skeleton className="h-44" />
        <Skeleton className="h-44" />
        <Skeleton className="h-28" />
      </div>
    );
  }

  const hasKey = !!config?.llm.api_key;

  return (
    <div className="max-w-xl space-y-4 p-6">
      <h1 className="text-sm font-medium text-white">Settings</h1>

      {/* Node Config */}
      <Card>
        <CardHeader>
          <CardTitle>Node</CardTitle>
        </CardHeader>
        <div className="space-y-4">
          <div>
            <label className="mb-1.5 block text-xs text-[#6b6b6b]">Listen Address</label>
            <Input value={listen} onChange={(e) => setListen(e.target.value)} className="font-mono" />
          </div>
          <div className="flex items-center gap-3">
            <p className="text-xs text-[#3a3a3a]">Headless: {config?.node.headless ? 'yes' : 'no'}</p>
            {config?.node.web_port && (
              <p className="text-xs text-[#3a3a3a]">Web port: {config.node.web_port}</p>
            )}
          </div>
          <Button onClick={saveNodeConfig} disabled={saving}>
            <Save size={13} />
            Save
          </Button>
        </div>
      </Card>

      {/* LLM Config */}
      <Card>
        <CardHeader>
          <CardTitle>LLM</CardTitle>
          <Badge variant={hasKey ? 'success' : 'warning'}>
            {hasKey ? (
              <><CheckCircle2 size={10} className="mr-1" />key set</>
            ) : (
              'no key'
            )}
          </Badge>
        </CardHeader>
        <div className="space-y-4">
          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="mb-1.5 block text-xs text-[#6b6b6b]">Provider</label>
              <Input value={provider} onChange={(e) => setProvider(e.target.value)} className="font-mono" />
            </div>
            <div>
              <label className="mb-1.5 block text-xs text-[#6b6b6b]">Model</label>
              <Input value={model} onChange={(e) => setModel(e.target.value)} className="font-mono" />
            </div>
          </div>
          <div>
            <label className="mb-1.5 block text-xs text-[#6b6b6b]">Endpoint</label>
            <Input value={endpoint} onChange={(e) => setEndpoint(e.target.value)} className="font-mono" placeholder="https://api.openai.com/v1" />
          </div>
          <Button onClick={saveLlmConfig} disabled={saving}>
            <Save size={13} />
            Save
          </Button>
        </div>
      </Card>

      {/* API Key */}
      <Card>
        <CardHeader>
          <CardTitle>API Key</CardTitle>
        </CardHeader>
        <div className="flex items-end gap-3">
          <div className="flex-1">
            <label className="mb-1.5 block text-xs text-[#6b6b6b]">{provider || 'Provider'} API Key</label>
            <Input
              type="password"
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              placeholder="sk-..."
              className="font-mono"
            />
          </div>
          <Button onClick={handleSetKey} disabled={validating || !apiKey.trim()}>
            <Key size={13} />
            {validating ? 'Validating...' : 'Set Key'}
          </Button>
        </div>
      </Card>

      {/* MCP Servers */}
      {config?.mcp.servers && config.mcp.servers.length > 0 && (
        <Card>
          <CardHeader>
            <CardTitle>MCP Servers</CardTitle>
          </CardHeader>
          <div className="space-y-1.5">
            {config.mcp.servers.map((srv) => (
              <div key={srv.name} className="flex items-center justify-between rounded border border-[#1c1c1c] px-3 py-2">
                <span className="font-mono text-xs text-white">{srv.name}</span>
                <span className="font-mono text-xs text-[#3a3a3a]">{srv.command}</span>
              </div>
            ))}
          </div>
        </Card>
      )}
    </div>
  );
}
