import { Outlet, useLocation } from 'react-router';
import { Sidebar } from './sidebar';
import { Winbar } from './winbar';

export function AppLayout() {
  const location = useLocation();

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
    </div>
  );
}
