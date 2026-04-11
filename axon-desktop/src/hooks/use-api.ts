import { useQuery } from '@tanstack/react-query';
import {
  getStatus, getPeers, getAgents, getTaskLog, getTaskStats,
  getTrust, getConfig, getModels,
} from '../lib/api';

export const useStatus    = () => useQuery({ queryKey: ['status'],     queryFn: ({ signal }) => getStatus(signal),    refetchInterval: 5000, retry: 1 });
export const usePeers     = () => useQuery({ queryKey: ['peers'],      queryFn: ({ signal }) => getPeers(signal),     refetchInterval: 5000, retry: 1 });
export const useAgents    = () => useQuery({ queryKey: ['agents'],     queryFn: ({ signal }) => getAgents(signal),    refetchInterval: 5000, retry: 1 });
export const useTaskLog   = () => useQuery({ queryKey: ['task-log'],   queryFn: ({ signal }) => getTaskLog(signal),   refetchInterval: 3000, retry: 1 });
export const useTaskStats = () => useQuery({ queryKey: ['task-stats'], queryFn: ({ signal }) => getTaskStats(signal), refetchInterval: 3000, retry: 1 });
export const useTrust     = () => useQuery({ queryKey: ['trust'],      queryFn: ({ signal }) => getTrust(signal),     refetchInterval: 10000, retry: 1 });
export const useConfig    = () => useQuery({ queryKey: ['config'],     queryFn: ({ signal }) => getConfig(signal),    retry: 1 });
export const useModels    = (provider: string) =>
  useQuery({ queryKey: ['models', provider], queryFn: ({ signal }) => getModels(provider, signal), enabled: !!provider, staleTime: 60_000, retry: 1 });
