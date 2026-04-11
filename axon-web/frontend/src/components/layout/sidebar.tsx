import { useState, useCallback } from "react";
import { NavLink } from "react-router";
import { clsx } from "clsx";
import {
  MessageSquare,
  Network,
  Bot,
  ListTodo,
  GitBranch,
  Database,
  Shield,
  Wrench,
  Settings,
  Terminal,
  Share2,
  PanelLeftClose,
  PanelLeft,
  Copy,
  Check,
} from "lucide-react";
import { useStatus } from "../../hooks/use-api";

const navItems = [
  { to: "/", icon: MessageSquare, label: "Chat" },
  { to: "/mesh", icon: Network, label: "Mesh" },
  { to: "/agents", icon: Bot, label: "Agents" },
  { to: "/graph", icon: Share2, label: "Graph" },
  { to: "/tasks", icon: ListTodo, label: "Tasks" },
  { to: "/workflows", icon: GitBranch, label: "Workflows" },
  { to: "/blackboard", icon: Database, label: "Blackboard" },
  { to: "/trust", icon: Shield, label: "Trust" },
  { to: "/tools", icon: Wrench, label: "Tools" },
  { to: "/settings", icon: Settings, label: "Settings" },
  { to: "/logs", icon: Terminal, label: "Logs" },
] as const;

export function Sidebar() {
  const [collapsed, setCollapsed] = useState(false);
  const [copied, setCopied] = useState(false);
  const { data: status } = useStatus();

  const peerId = status?.peer_id ?? "";
  const truncatedId = peerId
    ? `${peerId.slice(0, 8)}...${peerId.slice(-4)}`
    : "---";

  const copyPeerId = useCallback(() => {
    if (!peerId) return;
    navigator.clipboard.writeText(peerId);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }, [peerId]);

  return (
    <aside
      className={clsx(
        "flex flex-col h-screen border-r border-[#1c1c1c] bg-[#000000] transition-all duration-200",
        collapsed ? "w-12" : "w-52",
      )}
    >
      {/* Logo */}
      <div className="flex items-center justify-between h-12 px-4 border-b border-[#1c1c1c]">
        {!collapsed && (
          <span className="font-mono text-xs font-semibold tracking-[0.2em] text-white">
            AXON
          </span>
        )}
        <button
          onClick={() => setCollapsed(!collapsed)}
          className="p-1.5 rounded text-[#3a3a3a] hover:text-[#6b6b6b] hover:bg-[#141414] transition-colors cursor-pointer ml-auto"
        >
          {collapsed ? <PanelLeft size={14} /> : <PanelLeftClose size={14} />}
        </button>
      </div>

      {/* Navigation */}
      <nav className="flex-1 py-2 px-1.5 space-y-px overflow-y-auto">
        {navItems.map(({ to, icon: Icon, label }) => (
          <NavLink
            key={to}
            to={to}
            end={to === "/"}
            className={({ isActive }) =>
              clsx(
                "relative flex items-center gap-2.5 rounded px-3 py-2 text-sm transition-colors",
                isActive
                  ? "text-white"
                  : "text-[#3a3a3a] hover:text-[#aaaaaa] hover:bg-[#0c0c0c]",
                collapsed && "justify-center px-0",
              )
            }
          >
            {({ isActive }) => (
              <>
                {isActive && !collapsed && (
                  <span className="absolute left-0 top-1.5 bottom-1.5 w-[2px] rounded-full bg-white" />
                )}
                <Icon size={15} className="shrink-0" strokeWidth={isActive ? 2 : 1.5} />
                {!collapsed && (
                  <span className={clsx("text-[13px]", isActive ? "font-medium" : "font-normal")}>
                    {label}
                  </span>
                )}
              </>
            )}
          </NavLink>
        ))}
      </nav>

      {/* Peer ID */}
      <div className="border-t border-[#1c1c1c] p-2">
        {collapsed ? (
          <button
            onClick={copyPeerId}
            className="flex items-center justify-center w-full p-2 rounded text-[#3a3a3a] hover:text-[#6b6b6b] hover:bg-[#0c0c0c] transition-colors cursor-pointer"
            title={peerId || "No peer ID"}
          >
            {copied ? <Check size={12} className="text-[#22c55e]" /> : <Copy size={12} />}
          </button>
        ) : (
          <button
            onClick={copyPeerId}
            className="flex items-center gap-2 w-full px-2 py-1.5 rounded text-[10px] text-[#3a3a3a] hover:text-[#6b6b6b] hover:bg-[#0c0c0c] transition-colors cursor-pointer group"
            title={peerId || "No peer ID"}
          >
            <span className="font-mono truncate">{truncatedId}</span>
            {copied ? (
              <Check size={10} className="shrink-0 text-[#22c55e]" />
            ) : (
              <Copy
                size={10}
                className="shrink-0 opacity-0 group-hover:opacity-100 transition-opacity"
              />
            )}
          </button>
        )}
      </div>
    </aside>
  );
}
