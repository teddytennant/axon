import { useState, useEffect } from 'react';
import { Outlet, useLocation } from 'react-router';
import { Sidebar } from './sidebar';
import { Winbar } from './winbar';
import { Onboarding } from '../onboarding';
import { onboardingEvents, isOnboarded } from '../../lib/onboarding-events';
import { useStatus } from '../../hooks/use-api';

export function AppLayout() {
  const location  = useLocation();
  const { data: status, isError, isFetched } = useStatus();
  const [showOnboarding, setShowOnboarding] = useState(false);

  // Show onboarding automatically on first launch when node is not reachable
  useEffect(() => {
    if (isFetched && !status && isError && !isOnboarded()) {
      setShowOnboarding(true);
    }
  }, [isFetched, status, isError]);

  // Listen for "open setup" events from settings page
  useEffect(() => {
    return onboardingEvents.onOpen(() => setShowOnboarding(true));
  }, []);

  return (
    <div className="flex h-screen flex-col overflow-hidden bg-[#000]">
      <Winbar />
      <div className="flex flex-1 overflow-hidden">
        <Sidebar />
        <main
          key={location.pathname}
          className="flex-1 overflow-hidden animate-fade-up"
        >
          <Outlet />
        </main>
      </div>

      <Onboarding
        open={showOnboarding}
        onClose={() => setShowOnboarding(false)}
      />
    </div>
  );
}
