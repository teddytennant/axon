import { useState, useEffect, useRef, useCallback } from 'react';
import { Share2, X, Copy, Check, ZoomIn, ZoomOut, Home, Maximize2 } from 'lucide-react';
import { clsx } from 'clsx';
import { useAgents, usePeers, useTaskLog } from '../hooks/use-api';
import { useWebSocket } from '../hooks/use-websocket';
import type { AgentInfo, PeerResponse, TaskLogEntry, WsTasksData } from '../lib/types';

// ——— Types ———

type NodeKind = 'agent' | 'peer';

interface SimNode {
  id: string;
  kind: NodeKind;
  agent?: AgentInfo;
  peer?: PeerResponse;
  x: number;
  y: number;
  vx: number;
  vy: number;
  fx: number | null;
  fy: number | null;
}

interface SimEdge {
  source: string;
  target: string;
  label: string;
}

// ——— Card dimensions ———
const AW = 184;
const AH = 100;
const PW = 160;
const PH = 72;

// ——— Simulation constants ———
const REPULSION = 18000;
const SPRING_K = 0.045;
const SPRING_REST = 240;
const DAMPING = 0.78;
const GRAVITY = 0.020;
const ALPHA_DECAY = 0.0228;

// ——— Helpers ———

function buildEdges(agents: AgentInfo[], peers: PeerResponse[]): SimEdge[] {
  const edges: SimEdge[] = [];
  const seen = new Set<string>();
  const add = (a: string, b: string, label: string) => {
    const key = a < b ? `${a}\0${b}` : `${b}\0${a}`;
    if (!seen.has(key)) { seen.add(key); edges.push({ source: a, target: b, label }); }
  };
  for (const ag of agents) {
    for (const pe of peers) {
      const shared = ag.capabilities.filter(c => pe.capabilities.includes(c));
      if (shared.length > 0) add(ag.name, pe.peer_id, shared[0]);
    }
  }
  for (let i = 0; i < agents.length; i++) {
    for (let j = i + 1; j < agents.length; j++) {
      const shared = agents[i].capabilities.filter(c => agents[j].capabilities.includes(c));
      if (shared.length > 0) add(agents[i].name, agents[j].name, shared[0]);
    }
  }
  return edges;
}

function mergeNodes(
  agents: AgentInfo[],
  peers: PeerResponse[],
  current: SimNode[],
  cx: number,
  cy: number,
): SimNode[] {
  const prev = new Map(current.map(n => [n.id, n]));
  const total = agents.length + peers.length;
  const r = Math.max(100, Math.min(cx, cy) * 0.42);
  let idx = 0;
  const spawn = (id: string, kind: NodeKind) => {
    const p = prev.get(id);
    if (p) return { id, kind, x: p.x, y: p.y, vx: p.vx, vy: p.vy, fx: p.fx, fy: p.fy };
    const angle = total > 1 ? (idx / total) * Math.PI * 2 : 0;
    idx++;
    return {
      id, kind,
      x: cx + Math.cos(angle) * r + (Math.random() - 0.5) * 30,
      y: cy + Math.sin(angle) * r + (Math.random() - 0.5) * 30,
      vx: 0, vy: 0, fx: null, fy: null,
    };
  };
  return [
    ...agents.map(ag => ({ ...spawn(ag.name, 'agent' as NodeKind), agent: ag })),
    ...peers.map(pe => ({ ...spawn(pe.peer_id, 'peer' as NodeKind), peer: pe })),
  ];
}

// ——— Force simulation hook ———

interface SimReturn {
  nodes: SimNode[];
  edges: SimEdge[];
  simRef: React.RefObject<SimNode[]>;
  kickAlpha: () => void;
}

function useSim(agents: AgentInfo[], peers: PeerResponse[], cx: number, cy: number): SimReturn {
  const nodesRef = useRef<SimNode[]>([]);
  const edgesRef = useRef<SimEdge[]>([]);
  const alphaRef = useRef(1.0);
  const centerRef = useRef({ cx, cy });
  const [renderState, setRenderState] = useState<{ nodes: SimNode[]; edges: SimEdge[] }>({
    nodes: [], edges: [],
  });

  useEffect(() => { centerRef.current = { cx, cy }; }, [cx, cy]);

  useEffect(() => {
    edgesRef.current = buildEdges(agents, peers);
    nodesRef.current = mergeNodes(agents, peers, nodesRef.current, cx, cy);
    alphaRef.current = Math.max(alphaRef.current, 0.5);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [agents, peers]);

  const kickAlpha = useCallback(() => { alphaRef.current = Math.max(alphaRef.current, 0.8); }, []);

  useEffect(() => {
    let live = true;
    const tick = () => {
      if (!live) return;
      const ns = nodesRef.current;
      const es = edgesRef.current;
      const { cx: ccx, cy: ccy } = centerRef.current;
      const alpha = alphaRef.current;

      const anyPinned = ns.some(n => n.fx !== null);
      if (anyPinned) alphaRef.current = Math.max(alphaRef.current, 0.08);

      if (alpha > 0.001 && ns.length > 0) {
        for (let i = 0; i < ns.length; i++) {
          for (let j = i + 1; j < ns.length; j++) {
            const dx = ns[j].x - ns[i].x || 0.01;
            const dy = ns[j].y - ns[i].y || 0.01;
            const dist2 = Math.max(dx * dx + dy * dy, 1);
            const dist = Math.sqrt(dist2);
            const force = (REPULSION * alpha) / dist2;
            const fx = (dx / dist) * force;
            const fy = (dy / dist) * force;
            if (ns[i].fx === null) { ns[i].vx -= fx; ns[i].vy -= fy; }
            if (ns[j].fx === null) { ns[j].vx += fx; ns[j].vy += fy; }
          }
        }

        const nm = new Map(ns.map(n => [n.id, n]));
        for (const e of es) {
          const s = nm.get(e.source);
          const t = nm.get(e.target);
          if (!s || !t) continue;
          const dx = t.x - s.x;
          const dy = t.y - s.y;
          const dist = Math.max(Math.sqrt(dx * dx + dy * dy), 1);
          const stretch = (dist - SPRING_REST) * SPRING_K * alpha;
          const fx = (dx / dist) * stretch;
          const fy = (dy / dist) * stretch;
          if (s.fx === null) { s.vx += fx; s.vy += fy; }
          if (t.fx === null) { t.vx -= fx; t.vy -= fy; }
        }

        for (const n of ns) {
          if (n.fx === null) {
            n.vx += (ccx - n.x) * GRAVITY * alpha;
            n.vy += (ccy - n.y) * GRAVITY * alpha;
          }
        }

        for (const n of ns) {
          if (n.fx !== null) { n.x = n.fx; n.vx = 0; }
          else { n.vx *= DAMPING; n.x += n.vx; }
          if (n.fy !== null) { n.y = n.fy; n.vy = 0; }
          else { n.vy *= DAMPING; n.y += n.vy; }
        }

        alphaRef.current = alpha * (1 - ALPHA_DECAY);
      }

      setRenderState({ nodes: ns.map(n => ({ ...n })), edges: es });
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
    return () => { live = false; };
  }, []);

  return { nodes: renderState.nodes, edges: renderState.edges, simRef: nodesRef, kickAlpha };
}

// ——— Page ———

export default function GraphPage() {
  const { data: initAgents } = useAgents();
  const { data: initPeers } = usePeers();
  const { data: initTasks } = useTaskLog();
  const { subscribe } = useWebSocket();

  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [peers, setPeers] = useState<PeerResponse[]>([]);
  const [tasks, setTasks] = useState<TaskLogEntry[]>([]);

  useEffect(() => { if (initAgents) setAgents(initAgents); }, [initAgents]);
  useEffect(() => { if (initPeers) setPeers(initPeers); }, [initPeers]);
  useEffect(() => { if (initTasks) setTasks(initTasks); }, [initTasks]);
  useEffect(() => subscribe('agents', d => setAgents(d as AgentInfo[])), [subscribe]);
  useEffect(() => subscribe('peers', d => setPeers(d as PeerResponse[])), [subscribe]);
  useEffect(() => subscribe('tasks', d => setTasks((d as WsTasksData).recent)), [subscribe]);

  const containerRef = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ w: 800, h: 600 });
  useEffect(() => {
    const obs = new ResizeObserver(([e]) => setSize({ w: e.contentRect.width, h: e.contentRect.height }));
    if (containerRef.current) obs.observe(containerRef.current);
    return () => obs.disconnect();
  }, []);

  const cx = size.w / 2;
  const cy = size.h / 2;

  const { nodes, edges, simRef, kickAlpha } = useSim(agents, peers, cx, cy);

  // ——— Pan / zoom ———
  const [transform, setTransform] = useState({ x: 0, y: 0, scale: 1 });
  const transformRef = useRef(transform);
  useEffect(() => { transformRef.current = transform; }, [transform]);

  const panState = useRef({ active: false, sx: 0, sy: 0, tx: 0, ty: 0 });
  const dragState = useRef<{ id: string | null; sx: number; sy: number; moved: boolean }>({
    id: null, sx: 0, sy: 0, moved: false,
  });

  const [selected, setSelected] = useState<string | null>(null);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const f = e.deltaY < 0 ? 1.12 : 1 / 1.12;
      setTransform(prev => {
        const ns = Math.min(Math.max(prev.scale * f, 0.1), 6);
        const rect = el.getBoundingClientRect();
        const mx = e.clientX - rect.left;
        const my = e.clientY - rect.top;
        return { scale: ns, x: mx - (mx - prev.x) * (ns / prev.scale), y: my - (my - prev.y) * (ns / prev.scale) };
      });
    };
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  }, []);

  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      if (panState.current.active) {
        setTransform(prev => ({ ...prev, x: panState.current.tx + (e.clientX - panState.current.sx), y: panState.current.ty + (e.clientY - panState.current.sy) }));
      }
      if (dragState.current.id) {
        const dx = e.clientX - dragState.current.sx;
        const dy = e.clientY - dragState.current.sy;
        if (Math.abs(dx) + Math.abs(dy) > 4) dragState.current.moved = true;
        const node = simRef.current.find(n => n.id === dragState.current.id);
        if (node) {
          const s = transformRef.current.scale;
          node.fx = (node.fx ?? node.x) + dx / s;
          node.fy = (node.fy ?? node.y) + dy / s;
        }
        dragState.current.sx = e.clientX;
        dragState.current.sy = e.clientY;
      }
    };
    const onUp = () => {
      panState.current.active = false;
      if (dragState.current.id) {
        if (!dragState.current.moved) setSelected(dragState.current.id);
        const node = simRef.current.find(n => n.id === dragState.current.id);
        if (node) { node.fx = null; node.fy = null; }
        kickAlpha();
        dragState.current.id = null;
      }
    };
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
    return () => { window.removeEventListener('mousemove', onMove); window.removeEventListener('mouseup', onUp); };
  }, [simRef, kickAlpha]);

  const onBgMouseDown = useCallback((e: React.MouseEvent) => {
    if ((e.target as HTMLElement).closest('[data-node]')) return;
    panState.current = { active: true, sx: e.clientX, sy: e.clientY, tx: transform.x, ty: transform.y };
  }, [transform]);

  const onNodeMouseDown = useCallback((e: React.MouseEvent, id: string) => {
    e.stopPropagation();
    dragState.current = { id, sx: e.clientX, sy: e.clientY, moved: false };
    const node = simRef.current.find(n => n.id === id);
    if (node) { node.fx = node.x; node.fy = node.y; }
  }, [simRef]);

  const selectedNode = nodes.find(n => n.id === selected) ?? null;

  const activeEdgeKeys = new Set(
    selected
      ? edges.filter(e => e.source === selected || e.target === selected).map(e => `${e.source}\0${e.target}`)
      : [],
  );

  const zoom = (factor: number) =>
    setTransform(p => ({ ...p, scale: Math.min(Math.max(p.scale * factor, 0.1), 6) }));

  return (
    <div className="flex h-full overflow-hidden">
      {/* Canvas */}
      <div
        ref={containerRef}
        className="relative flex-1 overflow-hidden cursor-grab active:cursor-grabbing select-none"
        style={{ background: '#070709' }}
        onMouseDown={onBgMouseDown}
        onClick={() => setSelected(null)}
      >
        {/* Dot-grid background */}
        <div
          className="absolute inset-0 pointer-events-none"
          style={{
            backgroundImage: 'radial-gradient(circle, #1c1c24 1px, transparent 1px)',
            backgroundSize: '28px 28px',
          }}
        />

        {nodes.length === 0 && (
          <div className="absolute inset-0 flex flex-col items-center justify-center gap-3 text-[#252530]">
            <Share2 size={36} />
            <p className="font-mono text-sm">no agents or peers</p>
          </div>
        )}

        {/* Transform wrapper */}
        <div
          className="absolute inset-0"
          style={{
            transform: `translate(${transform.x}px,${transform.y}px) scale(${transform.scale})`,
            transformOrigin: '0 0',
          }}
        >
          {/* SVG edge layer */}
          <svg
            className="absolute pointer-events-none"
            style={{ left: 0, top: 0, width: 1, height: 1, overflow: 'visible' }}
          >
            <defs>
              <style>{`
                .edge-flow { animation: flow-dash 0.9s linear infinite; }
                @keyframes flow-dash { from { stroke-dashoffset: 18; } to { stroke-dashoffset: 0; } }
              `}</style>
              {/* Glow filter for active edges */}
              <filter id="glow" x="-50%" y="-50%" width="200%" height="200%">
                <feGaussianBlur stdDeviation="3" result="blur" />
                <feMerge><feMergeNode in="blur" /><feMergeNode in="SourceGraphic" /></feMerge>
              </filter>
            </defs>

            {/* Base edges */}
            {edges.map(edge => {
              const s = nodes.find(n => n.id === edge.source);
              const t = nodes.find(n => n.id === edge.target);
              if (!s || !t) return null;
              const key = `${edge.source}\0${edge.target}`;
              const active = activeEdgeKeys.has(key);
              return (
                <g key={key}>
                  {/* Glow layer for active edges */}
                  {active && (
                    <line
                      x1={s.x} y1={s.y} x2={t.x} y2={t.y}
                      stroke="#00c8c8"
                      strokeWidth={6}
                      strokeOpacity={0.06}
                    />
                  )}
                  <line
                    x1={s.x} y1={s.y} x2={t.x} y2={t.y}
                    stroke={active ? '#00c8c8' : '#1e1e28'}
                    strokeWidth={active ? 1.5 : 1}
                    strokeOpacity={active ? 0.65 : 1}
                    strokeDasharray={active ? '6 6' : undefined}
                    className={active ? 'edge-flow' : undefined}
                  />
                </g>
              );
            })}
          </svg>

          {/* Node cards */}
          {nodes.map(node => {
            const w = node.kind === 'agent' ? AW : PW;
            const h = node.kind === 'agent' ? AH : PH;
            const isSelected = selected === node.id;
            return (
              <div
                key={node.id}
                data-node="true"
                className={clsx(
                  'absolute rounded-xl border cursor-pointer transition-shadow duration-150',
                  isSelected
                    ? 'border-[#00c8c8]/50 shadow-[0_0_0_1px_rgba(0,200,200,0.2),0_0_24px_rgba(0,200,200,0.12)]'
                    : node.kind === 'agent'
                      ? 'border-[#1e1e28] hover:border-[#2c2c3c] hover:shadow-[0_4px_20px_rgba(0,0,0,0.4)]'
                      : 'border-[#181820] hover:border-[#26263a] hover:shadow-[0_4px_20px_rgba(0,0,0,0.3)]',
                )}
                style={{
                  left: node.x - w / 2,
                  top: node.y - h / 2,
                  width: w,
                  background: node.kind === 'agent'
                    ? 'linear-gradient(135deg, #0e0e16 0%, #0c0c14 100%)'
                    : 'linear-gradient(135deg, #0b0b11 0%, #0a0a0f 100%)',
                }}
                onMouseDown={e => onNodeMouseDown(e, node.id)}
                onClick={e => { e.stopPropagation(); setSelected(node.id); }}
              >
                {node.kind === 'agent' && node.agent ? (
                  <AgentCard agent={node.agent} />
                ) : node.peer ? (
                  <PeerCard peer={node.peer} />
                ) : null}
              </div>
            );
          })}
        </div>

        {/* Toolbar — top right */}
        <div className="absolute top-4 right-4 flex flex-col gap-1 z-10">
          {[
            { icon: ZoomIn, fn: () => zoom(1.25), title: 'Zoom in' },
            { icon: ZoomOut, fn: () => zoom(1 / 1.25), title: 'Zoom out' },
            { icon: Home, fn: () => setTransform({ x: 0, y: 0, scale: 1 }), title: 'Reset view' },
            { icon: Maximize2, fn: () => zoom(1 / transform.scale), title: 'Fit' },
          ].map(({ icon: Icon, fn, title }) => (
            <button
              key={title}
              onClick={e => { e.stopPropagation(); fn(); }}
              title={title}
              className="flex h-7 w-7 items-center justify-center rounded-lg border border-[#1e1e28] bg-[#0e0e16]/90 text-[#444] backdrop-blur-sm transition-colors hover:border-[#2c2c3c] hover:text-[#888]"
            >
              <Icon size={13} />
            </button>
          ))}
        </div>

        {/* Scale indicator — top left */}
        <div className="absolute top-4 left-4 z-10">
          <span className="font-mono text-[9px] text-[#2a2a38]">
            {Math.round(transform.scale * 100)}%
          </span>
        </div>

        {/* Legend — bottom left */}
        <div className="absolute bottom-4 left-4 z-10 flex items-center gap-3">
          <LegendItem color="#00c8c8" label="agent" />
          <LegendItem color="#3a3a50" label="peer" />
          <span className="font-mono text-[9px] text-[#252530]">── shared capability</span>
        </div>

        {/* Node count — bottom center */}
        {nodes.length > 0 && (
          <div className="absolute bottom-4 left-1/2 -translate-x-1/2 z-10">
            <span className="font-mono text-[9px] text-[#2a2a38]">
              {agents.length} agent{agents.length !== 1 ? 's' : ''} · {peers.length} peer{peers.length !== 1 ? 's' : ''}
            </span>
          </div>
        )}
      </div>

      {/* Detail panel */}
      {selectedNode && (
        <DetailPanel node={selectedNode} tasks={tasks} onClose={() => setSelected(null)} />
      )}
    </div>
  );
}

// ——— Node card components ———

function AgentCard({ agent }: { agent: AgentInfo }) {
  const dotColor = { idle: '#50dc78', busy: '#f0c83c', err: '#f05050' }[agent.status.toLowerCase()] ?? '#888';
  const successRate = agent.tasks_handled > 0
    ? Math.round((agent.tasks_succeeded / agent.tasks_handled) * 100)
    : null;

  return (
    <div>
      {/* Top accent bar — color matches status */}
      <div
        className="h-0.5 rounded-t-xl opacity-70"
        style={{ background: `linear-gradient(90deg, ${dotColor}60, transparent)` }}
      />
      <div className="p-3">
        <div className="mb-1.5 flex items-center justify-between gap-2">
          <span className="truncate text-[11px] font-semibold leading-none text-[#e4e4f0]">{agent.name}</span>
          <span
            className="h-2 w-2 shrink-0 rounded-full ring-1 ring-black/50"
            style={{ backgroundColor: dotColor, boxShadow: `0 0 6px ${dotColor}60` }}
          />
        </div>
        <p className="mb-2 truncate font-mono text-[9px] text-[#3a3a50]">
          {agent.provider_type ? `${agent.provider_type} · ${agent.model_name}` : 'no model'}
        </p>
        {agent.capabilities.length > 0 && (
          <div className="mb-2 flex flex-wrap gap-1">
            {agent.capabilities.slice(0, 3).map(c => (
              <span key={c} className="rounded-md bg-[#141420] px-1.5 py-0.5 font-mono text-[8px] text-[#4a4a68]">{c}</span>
            ))}
            {agent.capabilities.length > 3 && (
              <span className="self-center text-[8px] text-[#2e2e40]">+{agent.capabilities.length - 3}</span>
            )}
          </div>
        )}
        <div className="flex gap-3 border-t border-[#141420] pt-1.5">
          <MiniStat label="tasks" value={String(agent.tasks_handled)} />
          <MiniStat label="ok" value={successRate !== null ? `${successRate}%` : '—'} />
          <MiniStat label="ms" value={agent.avg_latency_ms > 0 ? String(agent.avg_latency_ms) : '—'} />
        </div>
      </div>
    </div>
  );
}

function PeerCard({ peer }: { peer: PeerResponse }) {
  return (
    <div className="p-3">
      <div className="mb-1 flex items-center justify-between gap-2">
        <div className="flex items-center gap-1.5">
          <span className="h-1.5 w-1.5 rounded-full bg-[#3a3a50]" />
          <span className="truncate font-mono text-[10px] text-[#4a4a68]">{peer.peer_id.slice(0, 14)}…</span>
        </div>
        <span className="shrink-0 text-[9px] text-[#2a2a38]">{peer.last_seen_ago}</span>
      </div>
      <p className="mb-2 truncate font-mono text-[9px] text-[#303040]">{peer.addr}</p>
      {peer.capabilities.length > 0 && (
        <div className="flex flex-wrap gap-1">
          {peer.capabilities.slice(0, 4).map(c => (
            <span key={c} className="rounded-md bg-[#111118] px-1.5 py-0.5 font-mono text-[8px] text-[#3a3a50]">{c}</span>
          ))}
        </div>
      )}
    </div>
  );
}

function MiniStat({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <p className="text-[8px] uppercase tracking-widest text-[#2e2e40]">{label}</p>
      <p className="font-mono text-[9px] text-[#6868a0]">{value}</p>
    </div>
  );
}

function LegendItem({ color, label }: { color: string; label: string }) {
  return (
    <div className="flex items-center gap-1.5">
      <span
        className="h-2 w-2 rounded-sm border"
        style={{ backgroundColor: `${color}18`, borderColor: `${color}60` }}
      />
      <span className="font-mono text-[9px] text-[#2a2a38]">{label}</span>
    </div>
  );
}

// ——— Detail panel ———

function DetailPanel({ node, tasks, onClose }: {
  node: SimNode;
  tasks: TaskLogEntry[];
  onClose: () => void;
}) {
  const [copied, setCopied] = useState(false);
  const copy = (text: string) => {
    navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  if (node.kind === 'agent' && node.agent) {
    const a = node.agent;
    const dotColor = { idle: '#50dc78', busy: '#f0c83c', err: '#f05050' }[a.status.toLowerCase()] ?? '#888';
    const lcColor = { Running: '#50dc78', Paused: '#f0c83c', Stopped: '#f05050', Created: '#444' }[a.lifecycle_state] ?? '#444';
    const successRate = a.tasks_handled > 0 ? Math.round((a.tasks_succeeded / a.tasks_handled) * 100) : null;
    const relatedTasks = tasks.filter(t => a.capabilities.includes(t.capability)).slice(0, 12);

    return (
      <div className="flex w-72 shrink-0 flex-col overflow-hidden border-l border-[#141420] bg-[#08080f] animate-fade-in">
        {/* Header with accent */}
        <div className="relative border-b border-[#141420]">
          <div className="h-0.5" style={{ background: `linear-gradient(90deg, ${dotColor}50, transparent)` }} />
          <div className="flex items-center justify-between px-4 py-3">
            <div className="flex min-w-0 items-center gap-2">
              <span
                className="h-2 w-2 shrink-0 rounded-full"
                style={{ backgroundColor: dotColor, boxShadow: `0 0 8px ${dotColor}50` }}
              />
              <span className="truncate text-sm font-semibold text-[#e4e4f0]">{a.name}</span>
            </div>
            <CloseBtn onClose={onClose} />
          </div>
        </div>

        <div className="flex-1 space-y-5 overflow-y-auto p-4">
          {a.provider_type && (
            <Sec label="Model">
              <p className="font-mono text-xs text-[#5a5a80]">{a.provider_type} / {a.model_name}</p>
            </Sec>
          )}
          <Sec label="Status">
            <div className="flex items-center gap-3">
              <span className="font-mono text-xs font-semibold" style={{ color: dotColor }}>{a.status.toUpperCase()}</span>
              {a.lifecycle_state && (
                <span className="font-mono text-xs" style={{ color: lcColor }}>{a.lifecycle_state}</span>
              )}
            </div>
            {a.last_heartbeat_secs_ago != null && (
              <p className="mt-1 font-mono text-[10px] text-[#333345]">heartbeat {a.last_heartbeat_secs_ago}s ago</p>
            )}
          </Sec>
          <Sec label="Metrics">
            <div className="grid grid-cols-3 gap-2">
              {[
                { l: 'Tasks', v: String(a.tasks_handled) },
                { l: 'Success', v: successRate !== null ? `${successRate}%` : '—' },
                { l: 'Avg ms', v: a.avg_latency_ms > 0 ? String(a.avg_latency_ms) : '—' },
              ].map(({ l, v }) => (
                <div key={l} className="rounded-lg border border-[#141420] bg-[#0e0e18] p-2.5">
                  <p className="text-[9px] uppercase tracking-widest text-[#333345]">{l}</p>
                  <p className="mt-0.5 font-mono text-sm text-[#d4d4f0]">{v}</p>
                </div>
              ))}
            </div>
          </Sec>
          {a.capabilities.length > 0 && (
            <Sec label="Capabilities">
              <div className="flex flex-wrap gap-1.5">
                {a.capabilities.map(c => (
                  <span key={c} className="rounded-md border border-[#141420] bg-[#0e0e18] px-2 py-0.5 font-mono text-[10px] text-[#5a5a80]">{c}</span>
                ))}
              </div>
            </Sec>
          )}
          <Sec label="Recent Activity">
            {relatedTasks.length === 0 ? (
              <p className="text-[10px] text-[#252530]">No recent tasks</p>
            ) : (
              <div className="space-y-1">
                {relatedTasks.map(t => <TaskRow key={t.id} task={t} />)}
              </div>
            )}
          </Sec>
        </div>
      </div>
    );
  }

  if (node.kind === 'peer' && node.peer) {
    const p = node.peer;
    return (
      <div className="flex w-72 shrink-0 flex-col overflow-hidden border-l border-[#141420] bg-[#08080f] animate-fade-in">
        <div className="flex items-center justify-between border-b border-[#141420] px-4 py-3">
          <div className="flex items-center gap-2">
            <span className="h-2 w-2 rounded-full bg-[#3a3a50]" />
            <span className="text-sm font-semibold text-[#e4e4f0]">Peer</span>
          </div>
          <CloseBtn onClose={onClose} />
        </div>
        <div className="flex-1 space-y-4 overflow-y-auto p-4">
          <Sec label="Peer ID">
            <div className="flex items-start gap-2">
              <p className="flex-1 break-all font-mono text-[10px] leading-relaxed text-[#5a5a80]">{p.peer_id}</p>
              <button onClick={() => copy(p.peer_id)} className="mt-0.5 shrink-0 text-[#333345] transition-colors hover:text-[#888]">
                {copied ? <Check size={12} className="text-[#50dc78]" /> : <Copy size={12} />}
              </button>
            </div>
          </Sec>
          <Sec label="Address">
            <p className="font-mono text-xs text-[#5a5a80]">{p.addr}</p>
          </Sec>
          <Sec label="Last Seen">
            <p className="text-xs text-[#5a5a80]">{p.last_seen_ago}</p>
          </Sec>
          {p.capabilities.length > 0 && (
            <Sec label="Capabilities">
              <div className="flex flex-wrap gap-1.5">
                {p.capabilities.map(c => (
                  <span key={c} className="rounded-md border border-[#141420] bg-[#0e0e18] px-2 py-0.5 font-mono text-[10px] text-[#5a5a80]">{c}</span>
                ))}
              </div>
            </Sec>
          )}
        </div>
      </div>
    );
  }

  return null;
}

function Sec({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <p className="mb-2 text-[9px] uppercase tracking-widest text-[#2e2e40]">{label}</p>
      {children}
    </div>
  );
}

function CloseBtn({ onClose }: { onClose: () => void }) {
  return (
    <button onClick={onClose} className="text-[#333345] transition-colors hover:text-[#888]">
      <X size={14} />
    </button>
  );
}

function TaskRow({ task }: { task: TaskLogEntry }) {
  const color = { completed: '#50dc78', running: '#00c8c8', failed: '#f05050', pending: '#666688', cancelled: '#f0c83c' }[task.status] ?? '#444';
  const dur = task.duration_ms > 0
    ? task.duration_ms < 1000 ? `${task.duration_ms}ms` : `${(task.duration_ms / 1000).toFixed(1)}s`
    : '—';
  return (
    <div className="flex items-center justify-between gap-2 rounded-lg bg-[#0c0c14] px-2.5 py-1.5">
      <div className="flex min-w-0 items-center gap-2">
        <span className="h-1.5 w-1.5 shrink-0 rounded-full" style={{ backgroundColor: color }} />
        <span className="truncate font-mono text-[10px] text-[#5a5a80]">{task.capability}</span>
      </div>
      <span className="shrink-0 font-mono text-[9px] text-[#333345]">{dur}</span>
    </div>
  );
}
