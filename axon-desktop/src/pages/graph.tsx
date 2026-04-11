/**
 * Obsidian-style force-directed graph for the axon desktop app.
 *
 * Nodes are glowing circles (not DOM cards) rendered via SVG.
 * Agents are larger (r=20) with a cyan/status glow.
 * Peers are smaller (r=13) with a grey glow.
 * Edges curve with trust-weighted opacity.
 * Hovering a node shows a floating detail card.
 */

import { useState, useEffect, useRef, useCallback } from 'react';
import { clsx } from 'clsx';
import { Copy, Check, X, RefreshCw } from 'lucide-react';
import { useAgents, usePeers, useTaskLog, useTrust, useStatus } from '../hooks/use-api';
import { useWebSocket } from '../hooks/use-websocket';
import type { AgentInfo, PeerResponse, TaskLogEntry, TrustEntry, WsTasksData } from '../lib/types';

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
  /** Degree: number of edges connected (drives node size) */
  degree: number;
}

interface SimEdge {
  source: string;
  target: string;
  label: string;
  trust: number; // 0..1
}

// ——— Node sizing ———
const AGENT_R = 20;
const PEER_R = 13;

function nodeRadius(n: SimNode): number {
  const base = n.kind === 'agent' ? AGENT_R : PEER_R;
  const bonus = Math.min(n.degree * 1.5, 8);
  return base + bonus;
}

// ——— Simulation constants ———
const REPULSION = 22000;
const SPRING_K = 0.04;
const SPRING_REST = 200;
const DAMPING = 0.76;
const GRAVITY = 0.018;
const ALPHA_DECAY = 0.020;

// ——— Build graph ———

function buildEdges(
  agents: AgentInfo[],
  peers: PeerResponse[],
  trust: TrustEntry[],
): SimEdge[] {
  const edges: SimEdge[] = [];
  const seen = new Set<string>();
  const trustMap = new Map(trust.map(t => [t.peer_id, t.overall]));

  const add = (a: string, b: string, label: string) => {
    const key = a < b ? `${a}\0${b}` : `${b}\0${a}`;
    if (!seen.has(key)) {
      seen.add(key);
      const t = trustMap.get(b) ?? trustMap.get(a) ?? 0.5;
      edges.push({ source: a, target: b, label, trust: t });
    }
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

function buildDegrees(nodes: string[], edges: SimEdge[]): Map<string, number> {
  const deg = new Map<string, number>(nodes.map(n => [n, 0]));
  for (const e of edges) {
    deg.set(e.source, (deg.get(e.source) ?? 0) + 1);
    deg.set(e.target, (deg.get(e.target) ?? 0) + 1);
  }
  return deg;
}

function mergeNodes(
  agents: AgentInfo[],
  peers: PeerResponse[],
  edges: SimEdge[],
  current: SimNode[],
  cx: number,
  cy: number,
): SimNode[] {
  const allIds = [...agents.map(a => a.name), ...peers.map(p => p.peer_id)];
  const deg = buildDegrees(allIds, edges);
  const prev = new Map(current.map(n => [n.id, n]));
  const total = agents.length + peers.length;
  const r = Math.max(120, Math.min(cx, cy) * 0.4);
  let idx = 0;

  const spawn = (id: string, kind: NodeKind): Omit<SimNode, 'agent' | 'peer' | 'degree'> => {
    const p = prev.get(id);
    if (p) return { id, kind, x: p.x, y: p.y, vx: p.vx, vy: p.vy, fx: p.fx, fy: p.fy };
    const angle = total > 1 ? (idx / total) * Math.PI * 2 : 0;
    idx++;
    return {
      id, kind,
      x: cx + Math.cos(angle) * r + (Math.random() - 0.5) * 40,
      y: cy + Math.sin(angle) * r + (Math.random() - 0.5) * 40,
      vx: 0, vy: 0, fx: null, fy: null,
    };
  };

  return [
    ...agents.map(ag => ({
      ...spawn(ag.name, 'agent' as NodeKind),
      agent: ag,
      degree: deg.get(ag.name) ?? 0,
    })),
    ...peers.map(pe => ({
      ...spawn(pe.peer_id, 'peer' as NodeKind),
      peer: pe,
      degree: deg.get(pe.peer_id) ?? 0,
    })),
  ];
}

// ——— Sim hook ———

function useSim(
  agents: AgentInfo[],
  peers: PeerResponse[],
  trust: TrustEntry[],
  cx: number,
  cy: number,
) {
  const nodesRef = useRef<SimNode[]>([]);
  const edgesRef = useRef<SimEdge[]>([]);
  const alphaRef = useRef(1.0);
  const centerRef = useRef({ cx, cy });
  const [snap, setSnap] = useState<{ nodes: SimNode[]; edges: SimEdge[] }>({ nodes: [], edges: [] });

  useEffect(() => { centerRef.current = { cx, cy }; }, [cx, cy]);

  useEffect(() => {
    edgesRef.current = buildEdges(agents, peers, trust);
    nodesRef.current = mergeNodes(agents, peers, edgesRef.current, nodesRef.current, cx, cy);
    alphaRef.current = Math.max(alphaRef.current, 0.5);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [agents, peers, trust]);

  const kickAlpha = useCallback(() => { alphaRef.current = Math.max(alphaRef.current, 0.8); }, []);

  useEffect(() => {
    let live = true;
    const tick = () => {
      if (!live) return;
      const ns = nodesRef.current;
      const es = edgesRef.current;
      const { cx: ccx, cy: ccy } = centerRef.current;
      const alpha = alphaRef.current;

      if (ns.some(n => n.fx !== null))
        alphaRef.current = Math.max(alphaRef.current, 0.08);

      if (alpha > 0.001 && ns.length > 0) {
        // Repulsion
        for (let i = 0; i < ns.length; i++) {
          for (let j = i + 1; j < ns.length; j++) {
            const dx = ns[j].x - ns[i].x || 0.01;
            const dy = ns[j].y - ns[i].y || 0.01;
            const dist2 = Math.max(dx * dx + dy * dy, 1);
            const dist = Math.sqrt(dist2);
            const f = (REPULSION * alpha) / dist2;
            const fx = (dx / dist) * f;
            const fy = (dy / dist) * f;
            if (ns[i].fx === null) { ns[i].vx -= fx; ns[i].vy -= fy; }
            if (ns[j].fx === null) { ns[j].vx += fx; ns[j].vy += fy; }
          }
        }
        // Springs
        const nm = new Map(ns.map(n => [n.id, n]));
        for (const e of es) {
          const s = nm.get(e.source);
          const t = nm.get(e.target);
          if (!s || !t) continue;
          const dx = t.x - s.x, dy = t.y - s.y;
          const dist = Math.max(Math.sqrt(dx * dx + dy * dy), 1);
          const stretch = (dist - SPRING_REST) * SPRING_K * alpha;
          const fx = (dx / dist) * stretch, fy = (dy / dist) * stretch;
          if (s.fx === null) { s.vx += fx; s.vy += fy; }
          if (t.fx === null) { t.vx -= fx; t.vy -= fy; }
        }
        // Gravity
        for (const n of ns) {
          if (n.fx === null) {
            n.vx += (ccx - n.x) * GRAVITY * alpha;
            n.vy += (ccy - n.y) * GRAVITY * alpha;
          }
        }
        // Integrate
        for (const n of ns) {
          if (n.fx !== null) { n.x = n.fx; n.vx = 0; }
          else { n.vx *= DAMPING; n.x += n.vx; }
          if (n.fy !== null) { n.y = n.fy; n.vy = 0; }
          else { n.vy *= DAMPING; n.y += n.vy; }
        }
        alphaRef.current = alpha * (1 - ALPHA_DECAY);
      }

      setSnap({ nodes: ns.map(n => ({ ...n })), edges: es });
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
    return () => { live = false; };
  }, []);

  return { nodes: snap.nodes, edges: snap.edges, simRef: nodesRef, kickAlpha };
}

// ——— Color helpers ———

function agentStatusColor(status: string): string {
  return { idle: '#22c55e', busy: '#f59e0b', err: '#ef4444' }[status.toLowerCase()] ?? '#555555';
}

// ——— Main page ———

export default function GraphPage() {
  const { data: initAgents } = useAgents();
  const { data: initPeers } = usePeers();
  const { data: initTasks } = useTaskLog();
  const { data: initTrust } = useTrust();
  const { data: status } = useStatus();
  const { subscribe } = useWebSocket();

  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [peers, setPeers] = useState<PeerResponse[]>([]);
  const [tasks, setTasks] = useState<TaskLogEntry[]>([]);
  const [trust, setTrust] = useState<TrustEntry[]>([]);

  useEffect(() => { if (initAgents) setAgents(initAgents); }, [initAgents]);
  useEffect(() => { if (initPeers) setPeers(initPeers); }, [initPeers]);
  useEffect(() => { if (initTasks) setTasks(initTasks); }, [initTasks]);
  useEffect(() => { if (initTrust) setTrust(initTrust); }, [initTrust]);
  useEffect(() => subscribe('agents', d => setAgents(d as AgentInfo[])), [subscribe]);
  useEffect(() => subscribe('peers', d => setPeers(d as PeerResponse[])), [subscribe]);
  useEffect(() => subscribe('tasks', d => setTasks((d as WsTasksData).recent)), [subscribe]);
  useEffect(() => subscribe('trust', d => setTrust(d as TrustEntry[])), [subscribe]);

  // Canvas size
  const svgRef = useRef<SVGSVGElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ w: 1000, h: 700 });
  useEffect(() => {
    const obs = new ResizeObserver(([e]) =>
      setSize({ w: e.contentRect.width, h: e.contentRect.height }),
    );
    if (containerRef.current) obs.observe(containerRef.current);
    return () => obs.disconnect();
  }, []);

  const cx = size.w / 2, cy = size.h / 2;
  const { nodes, edges, simRef, kickAlpha } = useSim(agents, peers, trust, cx, cy);

  // Pan + zoom
  const [vp, setVp] = useState({ x: 0, y: 0, scale: 1 });
  const vpRef = useRef(vp);
  useEffect(() => { vpRef.current = vp; }, [vp]);

  const panRef = useRef({ active: false, sx: 0, sy: 0, tx: 0, ty: 0 });
  const dragRef = useRef<{ id: string | null; sx: number; sy: number; moved: boolean }>({
    id: null, sx: 0, sy: 0, moved: false,
  });

  const [selected, setSelected] = useState<string | null>(null);
  const [hovered, setHovered] = useState<string | null>(null);

  // Wheel zoom
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const fn = (e: WheelEvent) => {
      e.preventDefault();
      const f = e.deltaY < 0 ? 1.14 : 1 / 1.14;
      setVp(prev => {
        const ns = Math.min(Math.max(prev.scale * f, 0.08), 8);
        const rect = el.getBoundingClientRect();
        const mx = e.clientX - rect.left;
        const my = e.clientY - rect.top;
        return { scale: ns, x: mx - (mx - prev.x) * (ns / prev.scale), y: my - (my - prev.y) * (ns / prev.scale) };
      });
    };
    el.addEventListener('wheel', fn, { passive: false });
    return () => el.removeEventListener('wheel', fn);
  }, []);

  // Mouse move/up
  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      if (panRef.current.active) {
        setVp(p => ({ ...p, x: panRef.current.tx + (e.clientX - panRef.current.sx), y: panRef.current.ty + (e.clientY - panRef.current.sy) }));
      }
      if (dragRef.current.id) {
        const dx = e.clientX - dragRef.current.sx;
        const dy = e.clientY - dragRef.current.sy;
        if (Math.abs(dx) + Math.abs(dy) > 3) dragRef.current.moved = true;
        const node = simRef.current.find(n => n.id === dragRef.current.id);
        if (node) {
          const s = vpRef.current.scale;
          node.fx = (node.fx ?? node.x) + dx / s;
          node.fy = (node.fy ?? node.y) + dy / s;
        }
        dragRef.current.sx = e.clientX;
        dragRef.current.sy = e.clientY;
      }
    };
    const onUp = () => {
      panRef.current.active = false;
      if (dragRef.current.id) {
        if (!dragRef.current.moved) setSelected(dragRef.current.id);
        const node = simRef.current.find(n => n.id === dragRef.current.id);
        if (node) { node.fx = null; node.fy = null; }
        kickAlpha();
        dragRef.current.id = null;
      }
    };
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
    return () => { window.removeEventListener('mousemove', onMove); window.removeEventListener('mouseup', onUp); };
  }, [simRef, kickAlpha]);

  const onBgDown = useCallback((e: React.MouseEvent) => {
    if ((e.target as Element).closest('[data-node]')) return;
    setSelected(null);
    panRef.current = { active: true, sx: e.clientX, sy: e.clientY, tx: vp.x, ty: vp.y };
  }, [vp]);

  const onNodeDown = useCallback((e: React.MouseEvent, id: string) => {
    e.stopPropagation();
    dragRef.current = { id, sx: e.clientX, sy: e.clientY, moved: false };
    const node = simRef.current.find(n => n.id === id);
    if (node) { node.fx = node.x; node.fy = node.y; }
  }, [simRef]);

  const activeEdgeSet = new Set(
    selected ? edges.filter(e => e.source === selected || e.target === selected).map(e => `${e.source}\0${e.target}`) : [],
  );
  const hoveredEdgeSet = new Set(
    hovered ? edges.filter(e => e.source === hovered || e.target === hovered).map(e => `${e.source}\0${e.target}`) : [],
  );

  const selectedNode = nodes.find(n => n.id === selected) ?? null;

  // Fit view
  const fitView = useCallback(() => {
    if (nodes.length === 0) { setVp({ x: 0, y: 0, scale: 1 }); return; }
    const xs = nodes.map(n => n.x), ys = nodes.map(n => n.y);
    const minX = Math.min(...xs) - 60, maxX = Math.max(...xs) + 60;
    const minY = Math.min(...ys) - 60, maxY = Math.max(...ys) + 60;
    const sw = maxX - minX, sh = maxY - minY;
    const scale = Math.min(size.w / sw, size.h / sh, 2) * 0.9;
    const x = (size.w - sw * scale) / 2 - minX * scale;
    const y = (size.h - sh * scale) / 2 - minY * scale;
    setVp({ x, y, scale });
  }, [nodes, size]);

  const isConnected = status !== undefined;

  return (
    <div className="flex h-full overflow-hidden bg-[#000000]">
      {/* Main canvas */}
      <div
        ref={containerRef}
        className="relative flex-1 overflow-hidden cursor-grab active:cursor-grabbing select-none"
        onMouseDown={onBgDown}
      >
        {/* Subtle grid — pure lines, no blobs */}
        <svg className="absolute inset-0 w-full h-full pointer-events-none" style={{ opacity: 0.06 }}>
          <defs>
            <pattern id="grid" width="40" height="40" patternUnits="userSpaceOnUse">
              <path d="M 40 0 L 0 0 0 40" fill="none" stroke="#fff" strokeWidth="0.5" />
            </pattern>
          </defs>
          <rect width="100%" height="100%" fill="url(#grid)" />
        </svg>

        {nodes.length === 0 && (
          <div className="absolute inset-0 flex flex-col items-center justify-center gap-3 pointer-events-none">
            <div className="text-center">
              <p className="font-mono text-sm text-[#2a2a2a]">
                {isConnected ? 'no agents or peers' : 'connecting to axon…'}
              </p>
              {!isConnected && (
                <p className="mt-1 font-mono text-[10px] text-[#1e1e1e]">
                  make sure axon is running on localhost:3000
                </p>
              )}
            </div>
          </div>
        )}

        {/* SVG graph */}
        <svg
          ref={svgRef}
          className="absolute inset-0 w-full h-full"
          style={{ overflow: 'visible' }}
        >
          <defs>
            <style>{`
              .edge-flow { animation: edge-flow 1.6s linear infinite; }
              @keyframes edge-flow { from { stroke-dashoffset: 20; } to { stroke-dashoffset: 0; } }
            `}</style>
          </defs>

          <g transform={`translate(${vp.x},${vp.y}) scale(${vp.scale})`}>
            {/* ——— Edges ——— */}
            {edges.map(edge => {
              const s = nodes.find(n => n.id === edge.source);
              const t = nodes.find(n => n.id === edge.target);
              if (!s || !t) return null;
              const key = `${edge.source}\0${edge.target}`;
              const isActive = activeEdgeSet.has(key);
              const isHov = hoveredEdgeSet.has(key);
              const highlight = isActive || isHov;

              // Slightly curved bezier
              const mx = (s.x + t.x) / 2 + (t.y - s.y) * 0.08;
              const my = (s.y + t.y) / 2 - (t.x - s.x) * 0.08;
              const d = `M ${s.x} ${s.y} Q ${mx} ${my} ${t.x} ${t.y}`;

              return (
                <g key={key}>
                  {/* Glow halo for highlighted edges */}
                  {highlight && (
                    <path
                      d={d}
                      fill="none"
                      stroke="#888888"
                      strokeWidth={8}
                      strokeOpacity={0.06}
                    />
                  )}
                  <path
                    d={d}
                    fill="none"
                    stroke={highlight ? '#888888' : `rgba(100,100,100,${0.1 + edge.trust * 0.2})`}
                    strokeWidth={highlight ? 1.5 : 0.8 + edge.trust * 0.6}
                    strokeOpacity={highlight ? 0.7 : 0.6}
                    strokeDasharray={highlight ? '6 6' : undefined}
                    className={highlight ? 'edge-flow' : undefined}
                    strokeLinecap="round"
                  />
                </g>
              );
            })}

            {/* ——— Nodes ——— */}
            {nodes.map(node => {
              const r = nodeRadius(node);
              const isSelected = selected === node.id;
              const isHov = hovered === node.id;
              const isHighlighted = isSelected || isHov;

              let color: string;

              if (node.kind === 'agent' && node.agent) {
                color = agentStatusColor(node.agent.status);
              } else {
                color = isHighlighted ? '#555555' : '#333333';
              }

              const label = node.kind === 'agent'
                ? node.agent?.name ?? node.id
                : node.id.slice(0, 10) + '…';

              return (
                <g
                  key={node.id}
                  data-node="true"
                  style={{ cursor: 'pointer' }}
                  onMouseDown={e => onNodeDown(e, node.id)}
                  onClick={e => { e.stopPropagation(); setSelected(node.id); }}
                  onMouseEnter={() => setHovered(node.id)}
                  onMouseLeave={() => setHovered(null)}
                >
                  {/* Selection ring */}
                  {isSelected && (
                    <circle
                      cx={node.x} cy={node.y}
                      r={r + 6}
                      fill="none"
                      stroke="#ffffff"
                      strokeWidth={0.5}
                      strokeOpacity={0.2}
                    />
                  )}

                  {/* Main node circle */}
                  <circle
                    cx={node.x} cy={node.y}
                    r={r}
                    fill={isHighlighted ? `${color}18` : `${color}0a`}
                    stroke={color}
                    strokeWidth={isHighlighted ? 1.2 : 0.7}
                    strokeOpacity={isHighlighted ? 0.9 : 0.6}
                  />

                  {/* Inner dot for agents */}
                  {node.kind === 'agent' && (
                    <circle
                      cx={node.x} cy={node.y}
                      r={4}
                      fill={color}
                      fillOpacity={isHighlighted ? 0.9 : 0.5}
                    />
                  )}

                  {/* Node label */}
                  <text
                    x={node.x}
                    y={node.y + r + 14}
                    textAnchor="middle"
                    fontSize={isHighlighted ? 10 : 9}
                    fill={isHighlighted ? '#aaaaaa' : '#3a3a3a'}
                    fontFamily="JetBrains Mono, monospace"
                    style={{ pointerEvents: 'none' }}
                  >
                    {label}
                  </text>

                  {/* Capability count badge */}
                  {node.degree > 0 && (
                    <g>
                      <circle
                        cx={node.x + r - 1}
                        cy={node.y - r + 1}
                        r={6}
                        fill="#0e0e0e"
                        stroke="#1e1e1e"
                        strokeWidth={0.5}
                      />
                      <text
                        x={node.x + r - 1}
                        y={node.y - r + 1 + 3.5}
                        textAnchor="middle"
                        fontSize={6.5}
                        fill="#555555"
                        fontFamily="JetBrains Mono, monospace"
                        style={{ pointerEvents: 'none' }}
                      >
                        {node.degree}
                      </text>
                    </g>
                  )}
                </g>
              );
            })}
          </g>
        </svg>

        {/* Controls — top right */}
        <div className="absolute top-3 right-3 flex items-center gap-1 z-10">
          {[
            { label: '+', fn: () => setVp(p => ({ ...p, scale: Math.min(p.scale * 1.2, 8) })), title: 'Zoom in' },
            { label: '−', fn: () => setVp(p => ({ ...p, scale: Math.max(p.scale / 1.2, 0.08) })), title: 'Zoom out' },
            { label: '⌂', fn: fitView, title: 'Fit view  f' },
          ].map(({ label, fn, title }) => (
            <button
              key={label}
              onClick={e => { e.stopPropagation(); fn(); }}
              title={title}
              className="flex h-6 w-6 items-center justify-center rounded border border-[#161616] bg-[#000] font-mono text-[10px] text-[#333] transition-colors hover:border-[#222] hover:text-[#666]"
            >
              {label}
            </button>
          ))}
        </div>

        {/* Status + stats — top left */}
        <div className="absolute top-3 left-3 z-10 flex items-center gap-3">
          <div className="flex items-center gap-1.5">
            <span className={clsx(
              'h-[4px] w-[4px] rounded-full',
              isConnected ? 'bg-[#22c55e]' : 'bg-[#333]',
            )} />
            <span className="font-mono text-[9px] text-[#2a2a2a]">
              {isConnected ? 'live' : 'offline'}
            </span>
          </div>
          {nodes.length > 0 && (
            <span className="font-mono text-[9px] text-[#222]">
              {agents.length}a · {peers.length}p · {edges.length}e
            </span>
          )}
        </div>

        {/* Legend — bottom left */}
        <div className="absolute bottom-3 left-3 z-10 flex items-center gap-3">
          <LegendDot color="#22c55e" label="idle" />
          <LegendDot color="#f59e0b" label="busy" />
          <LegendDot color="#333" label="peer" />
        </div>

        {/* Scale — bottom right */}
        <div className="absolute bottom-3 right-3 z-10 flex items-center gap-1">
          <span className="font-mono text-[8px] text-[#1e1e1e]">{Math.round(vp.scale * 100)}%</span>
        </div>

        {/* Hover tooltip — appears near cursor, not edge of screen */}
        {hovered && hovered !== selected && (() => {
          const node = nodes.find(n => n.id === hovered);
          if (!node) return null;
          return (
            <HoverTooltip node={node} vp={vp} containerRef={containerRef} />
          );
        })()}
      </div>

      {/* Detail panel */}
      {selectedNode && (
        <DetailPanel
          node={selectedNode}
          tasks={tasks}
          trust={trust}
          onClose={() => setSelected(null)}
        />
      )}
    </div>
  );
}

// ——— Hover tooltip (lightweight, shown on hover) ———

function HoverTooltip({
  node,
  vp,
  containerRef,
}: {
  node: SimNode;
  vp: { x: number; y: number; scale: number };
  containerRef: React.RefObject<HTMLDivElement | null>;
}) {
  // Position the tooltip near the node in screen space
  const screenX = node.x * vp.scale + vp.x;
  const screenY = node.y * vp.scale + vp.y;
  const r = nodeRadius(node) * vp.scale + 12;

  const containerWidth = containerRef.current?.clientWidth ?? 1000;
  const tooltipWidth = 160;
  let left = screenX + r;
  if (left + tooltipWidth > containerWidth - 8) left = screenX - r - tooltipWidth;

  if (node.kind === 'agent' && node.agent) {
    const a = node.agent;
    const color = agentStatusColor(a.status);
    return (
      <div
        className="absolute z-20 pointer-events-none animate-fade-in"
        style={{ left, top: screenY - 40, width: tooltipWidth }}
      >
        <div className="rounded border border-[#181818] bg-[#080808]/98 px-3 py-2 shadow-xl">
          <div className="mb-1 flex items-center gap-1.5">
            <span className="h-1.5 w-1.5 rounded-full" style={{ backgroundColor: color }} />
            <span className="truncate text-[11px] font-semibold text-[#aaaaaa]">{a.name}</span>
          </div>
          {a.provider_type && (
            <p className="font-mono text-[9px] text-[#3a3a3a]">{a.provider_type} · {a.model_name}</p>
          )}
          <p className="mt-1 font-mono text-[9px] text-[#3a3a3a]">{a.capabilities.slice(0, 3).join(' · ')}</p>
        </div>
      </div>
    );
  }

  if (node.kind === 'peer' && node.peer) {
    const p = node.peer;
    return (
      <div
        className="absolute z-20 pointer-events-none animate-fade-in"
        style={{ left, top: screenY - 30, width: tooltipWidth }}
      >
        <div className="rounded border border-[#181818] bg-[#080808]/98 px-3 py-2 shadow-xl">
          <p className="font-mono text-[9px] text-[#555555]">{p.peer_id.slice(0, 20)}…</p>
          <p className="mt-0.5 font-mono text-[8px] text-[#3a3a3a]">{p.addr}</p>
          <p className="mt-0.5 text-[8px] text-[#2a2a2a]">{p.last_seen_ago}</p>
        </div>
      </div>
    );
  }

  return null;
}

// ——— Detail panel (shown on click) ———

function DetailPanel({ node, tasks, trust, onClose }: {
  node: SimNode;
  tasks: TaskLogEntry[];
  trust: TrustEntry[];
  onClose: () => void;
}) {
  const [copied, setCopied] = useState(false);
  const copy = (t: string) => { navigator.clipboard.writeText(t); setCopied(true); setTimeout(() => setCopied(false), 2000); };

  if (node.kind === 'agent' && node.agent) {
    const a = node.agent;
    const color = agentStatusColor(a.status);
    const successRate = a.tasks_handled > 0 ? Math.round((a.tasks_succeeded / a.tasks_handled) * 100) : null;
    const relatedTasks = tasks.filter(t => a.capabilities.includes(t.capability)).slice(0, 10);

    return (
      <Panel onClose={onClose} accentColor={color}>
        <PanelHeader onClose={onClose}>
          <div className="flex items-center gap-2">
            <span className="h-2 w-2 rounded-full" style={{ backgroundColor: color }} />
            <span className="truncate font-semibold text-[#e0e0e0]">{a.name}</span>
          </div>
        </PanelHeader>
        <div className="flex-1 space-y-5 overflow-y-auto p-4">
          <Sec label="Status">
            <div className="flex items-center gap-3">
              <span className="font-mono text-xs font-bold" style={{ color }}>{a.status.toUpperCase()}</span>
              {a.lifecycle_state && (
                <span className="font-mono text-xs text-[#555555]">{a.lifecycle_state}</span>
              )}
            </div>
            {a.last_heartbeat_secs_ago != null && (
              <p className="mt-1 font-mono text-[9px] text-[#2a2a2a]">♡ {a.last_heartbeat_secs_ago}s ago</p>
            )}
          </Sec>
          {a.provider_type && (
            <Sec label="Model">
              <p className="font-mono text-xs text-[#555555]">{a.provider_type} / {a.model_name}</p>
            </Sec>
          )}
          <Sec label="Metrics">
            <div className="grid grid-cols-3 gap-2">
              {[
                { l: 'Tasks', v: String(a.tasks_handled) },
                { l: 'Success', v: successRate != null ? `${successRate}%` : '—' },
                { l: 'Avg ms', v: a.avg_latency_ms > 0 ? String(a.avg_latency_ms) : '—' },
              ].map(({ l, v }) => (
                <div key={l} className="rounded-lg border border-[#141414] bg-[#0a0a0a] p-2.5">
                  <p className="text-[8px] uppercase tracking-widest text-[#2a2a2a]">{l}</p>
                  <p className="mt-0.5 font-mono text-sm text-[#cccccc]">{v}</p>
                </div>
              ))}
            </div>
          </Sec>
          {a.capabilities.length > 0 && (
            <Sec label="Capabilities">
              <div className="flex flex-wrap gap-1.5">
                {a.capabilities.map(c => (
                  <span key={c} className="rounded-md border border-[#141414] bg-[#0a0a0a] px-2 py-0.5 font-mono text-[10px] text-[#555555]">{c}</span>
                ))}
              </div>
            </Sec>
          )}
          {relatedTasks.length > 0 && (
            <Sec label="Recent Tasks">
              <div className="space-y-1">
                {relatedTasks.map(t => <TaskRow key={t.id} task={t} />)}
              </div>
            </Sec>
          )}
        </div>
      </Panel>
    );
  }

  if (node.kind === 'peer' && node.peer) {
    const p = node.peer;
    const t = trust.find(e => e.peer_id === p.peer_id);
    return (
      <Panel onClose={onClose} accentColor="#333333">
        <PanelHeader onClose={onClose}>
          <div className="flex items-center gap-2">
            <span className="h-2 w-2 rounded-full bg-[#3a3a3a]" />
            <span className="font-semibold text-[#e0e0e0]">Peer</span>
          </div>
        </PanelHeader>
        <div className="flex-1 space-y-4 overflow-y-auto p-4">
          <Sec label="Peer ID">
            <div className="flex items-start gap-2">
              <p className="flex-1 break-all font-mono text-[10px] leading-relaxed text-[#555555]">{p.peer_id}</p>
              <button onClick={() => copy(p.peer_id)} className="mt-0.5 shrink-0 text-[#2a2a2a] transition-colors hover:text-[#888]">
                {copied ? <Check size={12} className="text-[#22c55e]" /> : <Copy size={12} />}
              </button>
            </div>
          </Sec>
          <Sec label="Address">
            <p className="font-mono text-xs text-[#555555]">{p.addr}</p>
          </Sec>
          <Sec label="Last Seen">
            <p className="text-xs text-[#555555]">{p.last_seen_ago}</p>
          </Sec>
          {t && (
            <Sec label="Trust">
              <TrustBar score={t.overall} />
              <div className="mt-2 grid grid-cols-2 gap-1.5 text-[9px]">
                {[
                  { l: 'Reliability', v: t.reliability },
                  { l: 'Accuracy', v: t.accuracy },
                  { l: 'Availability', v: t.availability },
                  { l: 'Quality', v: t.quality },
                ].map(({ l, v }) => (
                  <div key={l} className="flex items-center justify-between rounded bg-[#0a0a0a] px-2 py-1">
                    <span className="text-[#2a2a2a]">{l}</span>
                    <span className="font-mono text-[#555555]">{Math.round(v * 100)}%</span>
                  </div>
                ))}
              </div>
            </Sec>
          )}
          {p.capabilities.length > 0 && (
            <Sec label="Capabilities">
              <div className="flex flex-wrap gap-1.5">
                {p.capabilities.map(c => (
                  <span key={c} className="rounded-md border border-[#141414] bg-[#0a0a0a] px-2 py-0.5 font-mono text-[10px] text-[#555555]">{c}</span>
                ))}
              </div>
            </Sec>
          )}
        </div>
      </Panel>
    );
  }

  return null;
}

// ——— Reusable panel components ———

function Panel({ children, accentColor, onClose }: {
  children: React.ReactNode;
  accentColor: string;
  onClose: () => void;
}) {
  return (
    <div
      className="flex w-[280px] shrink-0 flex-col overflow-hidden border-l border-[#1c1c1c] bg-[#000] animate-fade-in"
      onClick={e => e.stopPropagation()}
    >
      <div className="h-px" style={{ background: `linear-gradient(90deg, ${accentColor}40, transparent)` }} />
      {children}
    </div>
  );
}

function PanelHeader({ children, onClose }: { children: React.ReactNode; onClose: () => void }) {
  return (
    <div className="flex items-center justify-between border-b border-[#141414] px-4 py-3">
      <div className="min-w-0 flex-1">{children}</div>
      <button onClick={onClose} className="ml-2 shrink-0 text-[#2a2a2a] transition-colors hover:text-[#888]">
        <X size={14} />
      </button>
    </div>
  );
}

function Sec({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <p className="mb-2 text-[8px] uppercase tracking-widest text-[#2a2a2a]">{label}</p>
      {children}
    </div>
  );
}

function TrustBar({ score }: { score: number }) {
  const pct = Math.round(score * 100);
  const color = score >= 0.6 ? '#22c55e' : score >= 0.4 ? '#f59e0b' : '#ef4444';
  return (
    <div className="flex items-center gap-2">
      <div className="relative flex-1 h-1.5 bg-[#111] overflow-hidden">
        <div className="absolute left-0 top-0 h-full transition-all" style={{ width: `${pct}%`, backgroundColor: color }} />
      </div>
      <span className="font-mono text-[10px]" style={{ color }}>{pct}%</span>
    </div>
  );
}

function TaskRow({ task }: { task: TaskLogEntry }) {
  const color = { completed: '#22c55e', running: '#ffffff', failed: '#ef4444', pending: '#555555', cancelled: '#f59e0b' }[task.status] ?? '#3a3a3a';
  const dur = task.duration_ms > 0
    ? task.duration_ms < 1000 ? `${task.duration_ms}ms` : `${(task.duration_ms / 1000).toFixed(1)}s`
    : '—';
  return (
    <div className="flex items-center justify-between gap-2 rounded-lg bg-[#0a0a0a] px-2.5 py-1.5">
      <div className="flex min-w-0 items-center gap-2">
        <span className="h-1.5 w-1.5 shrink-0 rounded-full" style={{ backgroundColor: color }} />
        <span className="truncate font-mono text-[10px] text-[#555555]">{task.capability}</span>
      </div>
      <span className="shrink-0 font-mono text-[9px] text-[#2a2a2a]">{dur}</span>
    </div>
  );
}

function LegendDot({ color, label }: { color: string; label: string }) {
  return (
    <div className="flex items-center gap-1.5">
      <div
        className="h-2 w-2 rounded-full border"
        style={{ backgroundColor: `${color}18`, borderColor: `${color}60` }}
      />
      <span className="font-mono text-[8px] text-[#2a2a2a]">{label}</span>
    </div>
  );
}
