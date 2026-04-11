import { useLocation, Outlet } from "react-router";
import { Sidebar } from "./sidebar";
import { Header } from "./header";

export function AppLayout() {
  const location = useLocation();

  return (
    <div className="flex h-screen overflow-hidden bg-[#0a0a0a]">
      <Sidebar />
      <div className="flex flex-1 flex-col overflow-hidden">
        <Header />
        <main
          key={location.pathname}
          className="flex-1 overflow-auto animate-fade-up"
        >
          <Outlet />
        </main>
      </div>
    </div>
  );
}
