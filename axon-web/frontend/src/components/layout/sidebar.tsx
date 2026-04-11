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
        "flex flex-col h-screen border-r border-[#1a1a1a] bg-[#0a0a0a] transition-all duration-200",
        collapsed ? "w-14" : "w-56",
      )}
    >
      {/* Logo */}
      <div className="flex items-center justify-between h-14 px-4 border-b border-[#1a1a1a]">
        {!collapsed && (
          <div className="flex items-center gap-2">
            <div className="flex h-6 w-6 items-center justify-center rounded-md bg-[#00c8c8]/10">
              <Share2 size={12} className="text-[#00c8c8]" strokeWidth={2.5} />
            </div>
            <span className="font-mono text-sm font-semibold tracking-widest text-[#00c8c8]">
              AXON
            </span>
          </div>
        )}
        <button
          onClick={() => setCollapsed(!collapsed)}
          className="p-1.5 rounded-md text-[#444] hover:text-[#888] hover:bg-[#181818] transition-colors cursor-pointer"
        >
          {collapsed ? <PanelLeft size={15} /> : <PanelLeftClose size={15} />}
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
                "relative flex items-center gap-2.5 rounded-md px-3 py-2 text-sm transition-colors",
                isActive
                  ? "bg-[#00c8c8]/8 text-[#00c8c8]"
                  : "text-[#666] hover:text-[#ccc] hover:bg-[#141414]",
                collapsed && "justify-center px-0",
              )
            }
          >
            {({ isActive }) => (
              <>
                {/* Left accent bar for active item */}
                {isActive && !collapsed && (
                  <span className="absolute left-0 top-1 bottom-1 w-0.5 rounded-full bg-[#00c8c8]" />
                )}
                <Icon size={16} className="shrink-0" strokeWidth={isActive ? 2 : 1.75} />
                {!collapsed && <span className={clsx("font-medium", isActive && "font-semibold")}>{label}</span>}
              </>
            )}
          </NavLink>
        ))}
      </nav>

      {/* Peer ID */}
      <div className="border-t border-[#1a1a1a] p-2">
        {collapsed ? (
          <button
            onClick={copyPeerId}
            className="flex items-center justify-center w-full p-2 rounded-md text-[#444] hover:text-[#666] hover:bg-[#141414] transition-colors cursor-pointer"
            title={peerId || "No peer ID"}
          >
            {copied ? <Check size={13} className="text-[#50dc78]" /> : <Copy size={13} />}
          </button>
        ) : (
          <button
            onClick={copyPeerId}
            className="flex items-center gap-2 w-full px-2 py-1.5 rounded-md text-[10px] text-[#444] hover:text-[#666] hover:bg-[#141414] transition-colors cursor-pointer group"
            title={peerId || "No peer ID"}
          >
            <span className="font-mono truncate">{truncatedId}</span>
            {copied ? (
              <Check size={11} className="shrink-0 text-[#50dc78]" />
            ) : (
              <Copy
                size={11}
                className="shrink-0 opacity-0 group-hover:opacity-100 transition-opacity"
              />
            )}
          </button>
        )}
      </div>
    </aside>
  );
}
