import { Outlet, useLocation } from 'react-router';
import { Sidebar } from './sidebar';
import { Titlebar } from './titlebar';

export function AppLayout() {
  const location = useLocation();
  // Graph page gets full height without titlebar chrome
  const isGraph = location.pathname === '/';

  return (
    <div className="flex h-screen overflow-hidden bg-[#07070d]">
      <Sidebar />
      <div className="flex flex-1 flex-col overflow-hidden">
        {!isGraph && <Titlebar />}
        <main
          key={location.pathname}
          className="flex-1 overflow-hidden animate-fade-up"
        >
          <Outlet />
        </main>
      </div>
    </div>
  );
}
