import { useQuery } from '@tanstack/react-query';
import {
  getStatus, getPeers, getAgents, getTaskLog, getTaskStats,
  getTrust, getTools, getModels, getConfig,
} from '../lib/api';

export const useStatus = () => useQuery({ queryKey: ['status'], queryFn: getStatus, refetchInterval: 5000 });
export const usePeers = () => useQuery({ queryKey: ['peers'], queryFn: getPeers, refetchInterval: 5000 });
export const useAgents = () => useQuery({ queryKey: ['agents'], queryFn: getAgents, refetchInterval: 5000 });
export const useTaskLog = () => useQuery({ queryKey: ['task-log'], queryFn: getTaskLog, refetchInterval: 3000 });
export const useTaskStats = () => useQuery({ queryKey: ['task-stats'], queryFn: getTaskStats, refetchInterval: 3000 });
export const useTrust = () => useQuery({ queryKey: ['trust'], queryFn: getTrust, refetchInterval: 10000 });
export const useTools = () => useQuery({ queryKey: ['tools'], queryFn: getTools });
export const useModels = (provider: string) => useQuery({ queryKey: ['models', provider], queryFn: () => getModels(provider), enabled: !!provider });
export const useConfig = () => useQuery({ queryKey: ['config'], queryFn: getConfig });
