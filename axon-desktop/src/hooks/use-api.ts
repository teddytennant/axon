import { useQuery } from '@tanstack/react-query';
import {
  getStatus, getPeers, getAgents, getTaskLog, getTaskStats,
  getTrust, getConfig,
} from '../lib/api';

export const useStatus = () => useQuery({ queryKey: ['status'], queryFn: getStatus, refetchInterval: 5000, retry: 2 });
export const usePeers = () => useQuery({ queryKey: ['peers'], queryFn: getPeers, refetchInterval: 5000, retry: 2 });
export const useAgents = () => useQuery({ queryKey: ['agents'], queryFn: getAgents, refetchInterval: 5000, retry: 2 });
export const useTaskLog = () => useQuery({ queryKey: ['task-log'], queryFn: getTaskLog, refetchInterval: 3000, retry: 2 });
export const useTaskStats = () => useQuery({ queryKey: ['task-stats'], queryFn: getTaskStats, refetchInterval: 3000, retry: 2 });
export const useTrust = () => useQuery({ queryKey: ['trust'], queryFn: getTrust, refetchInterval: 10000, retry: 2 });
export const useConfig = () => useQuery({ queryKey: ['config'], queryFn: getConfig, retry: 2 });
