import { BrowserRouter, Routes, Route } from 'react-router';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { Toaster } from 'sonner';
import { AppLayout } from './components/layout/app-layout';
import { CommandPalette } from './components/command-palette';
import GraphPage from './pages/graph';

import { lazy, Suspense } from 'react';

const ChatPage      = lazy(() => import('./pages/chat'));
const MeshPage      = lazy(() => import('./pages/mesh'));
const AgentsPage    = lazy(() => import('./pages/agents'));
const TasksPage     = lazy(() => import('./pages/tasks'));
const WorkflowsPage = lazy(() => import('./pages/workflows'));
const TrustPage     = lazy(() => import('./pages/trust'));
const SettingsPage  = lazy(() => import('./pages/settings'));

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 4000,
      retry: 1,
      refetchOnWindowFocus: false,
    },
  },
});

function PageFallback() {
  return (
    <div className="flex h-full items-center justify-center">
      <span className="font-mono text-[10px] text-[#222] animate-pulse">·</span>
    </div>
  );
}

export function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <CommandPalette />
        <Routes>
          <Route element={<AppLayout />}>
            <Route index element={<GraphPage />} />
            <Route path="chat"      element={<Suspense fallback={<PageFallback />}><ChatPage /></Suspense>} />
            <Route path="mesh"      element={<Suspense fallback={<PageFallback />}><MeshPage /></Suspense>} />
            <Route path="agents"    element={<Suspense fallback={<PageFallback />}><AgentsPage /></Suspense>} />
            <Route path="tasks"     element={<Suspense fallback={<PageFallback />}><TasksPage /></Suspense>} />
            <Route path="workflows" element={<Suspense fallback={<PageFallback />}><WorkflowsPage /></Suspense>} />
            <Route path="trust"     element={<Suspense fallback={<PageFallback />}><TrustPage /></Suspense>} />
            <Route path="settings"  element={<Suspense fallback={<PageFallback />}><SettingsPage /></Suspense>} />
          </Route>
        </Routes>
        <Toaster
          theme="dark"
          position="bottom-right"
          toastOptions={{
            style: {
              background: '#0a0a0a',
              border: '1px solid #1e1e1e',
              color: '#b5b5b5',
              fontFamily: 'JetBrains Mono, monospace',
              fontSize: '11px',
              borderRadius: '8px',
              boxShadow: '0 8px 32px rgba(0,0,0,0.8)',
            },
          }}
        />
      </BrowserRouter>
    </QueryClientProvider>
  );
}
