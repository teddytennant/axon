import { BrowserRouter, Routes, Route } from "react-router";
import { AppLayout } from "./components/layout/app-layout";
import ChatPage from "./pages/chat";
import MeshPage from "./pages/mesh";
import AgentsPage from "./pages/agents";
import TasksPage from "./pages/tasks";
import TrustPage from "./pages/trust";
import ToolsPage from "./pages/tools";
import SettingsPage from "./pages/settings";
import LogsPage from "./pages/logs";

export function App() {
  return (
    <BrowserRouter>
      <Routes>
        <Route element={<AppLayout />}>
          <Route index element={<ChatPage />} />
          <Route path="mesh" element={<MeshPage />} />
          <Route path="agents" element={<AgentsPage />} />
          <Route path="tasks" element={<TasksPage />} />
          <Route path="trust" element={<TrustPage />} />
          <Route path="tools" element={<ToolsPage />} />
          <Route path="settings" element={<SettingsPage />} />
          <Route path="logs" element={<LogsPage />} />
        </Route>
      </Routes>
    </BrowserRouter>
  );
}
