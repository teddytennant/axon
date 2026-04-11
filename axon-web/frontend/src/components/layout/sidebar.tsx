import { useState, useCallback } from "react";
import { NavLink } from "react-router";
import { clsx } from "clsx";
import {
  MessageSquare,
  Network,
  Bot,
  ListTodo,
  Shield,
  Wrench,
  Settings,
  Terminal,
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
  { to: "/tasks", icon: ListTodo, label: "Tasks" },
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
        "flex flex-col h-screen border-r border-[#222222] bg-[#0a0a0a] transition-all duration-200",
        collapsed ? "w-14" : "w-60",
      )}
    >
      {/* Logo */}
      <div className="flex items-center justify-between h-14 px-4 border-b border-[#222222]">
        {!collapsed && (
          <span className="font-mono text-sm font-semibold tracking-widest text-[#00c8c8]">
            AXON
          </span>
        )}
        <button
          onClick={() => setCollapsed(!collapsed)}
          className="p-1 rounded-lg text-[#555555] hover:text-[#f5f5f5] hover:bg-[#181818] transition-colors cursor-pointer"
        >
          {collapsed ? <PanelLeft size={18} /> : <PanelLeftClose size={18} />}
        </button>
      </div>

      {/* Navigation */}
      <nav className="flex-1 py-3 px-2 space-y-0.5 overflow-y-auto">
        {navItems.map(({ to, icon: Icon, label }) => (
          <NavLink
            key={to}
            to={to}
            end={to === "/"}
            className={({ isActive }) =>
              clsx(
                "flex items-center gap-3 rounded-lg px-3 py-2 text-sm transition-colors",
                isActive
                  ? "bg-[#00c8c8]/10 text-[#00c8c8]"
                  : "text-[#888888] hover:text-[#f5f5f5] hover:bg-[#181818]",
                collapsed && "justify-center px-0",
              )
            }
          >
            <Icon size={18} className="shrink-0" />
            {!collapsed && <span>{label}</span>}
          </NavLink>
        ))}
      </nav>

      {/* Peer ID */}
      <div className="border-t border-[#222222] p-3">
        {collapsed ? (
          <button
            onClick={copyPeerId}
            className="flex items-center justify-center w-full p-1 rounded-lg text-[#555555] hover:text-[#888888] hover:bg-[#181818] transition-colors cursor-pointer"
            title={peerId || "No peer ID"}
          >
            {copied ? <Check size={14} /> : <Copy size={14} />}
          </button>
        ) : (
          <button
            onClick={copyPeerId}
            className="flex items-center gap-2 w-full px-2 py-1.5 rounded-lg text-xs text-[#555555] hover:text-[#888888] hover:bg-[#181818] transition-colors cursor-pointer group"
            title={peerId || "No peer ID"}
          >
            <span className="font-mono truncate">{truncatedId}</span>
            {copied ? (
              <Check size={12} className="shrink-0 text-[#50dc78]" />
            ) : (
              <Copy
                size={12}
                className="shrink-0 opacity-0 group-hover:opacity-100 transition-opacity"
              />
            )}
          </button>
        )}
      </div>
    </aside>
  );
}
