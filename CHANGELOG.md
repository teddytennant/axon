# Changelog

All notable changes to Axon are logged here. Dates are UTC.

Axon is in early development — breaking changes can happen in any release
until there's a tagged `v0.1.0`. The "Unreleased" section tracks work on
`main`.

## Unreleased

### Added
- `/transparent` (alias `/tp`) slash command in the chat TUI — toggles the
  popup and chat backdrop between the muted gray POPUP_BG and terminal
  transparent.
- Minimal braille spinners via [rattles](https://github.com/vyfor/rattles)
  in the chat "thinking" indicator, the model-picker loading state, and the
  onboarding wizard's model fetch.

### Changed
- Onboarding palette flattened to pure grayscale — green/yellow/blue
  accents replaced with shades of gray; bold/italic now carry emphasis.
  Muted red retained only for hard-error states.
- Chat TUI paints the full area with an opaque POPUP_BG by default so the
  `/transparent` toggle produces a visible change under translucent
  terminal configs (e.g. ghostty `background-opacity`).
- Dropped the triangle (`▲`) glyph from the chat header, onboarding logo,
  TUI dashboard header, and CLI status prints.

## 2026-04-21

### Refactored
- Consolidated LLM-provider helpers into `axon_web::providers`; CLI and
  desktop now share a single source of truth.
- Typed MCP `initialize` and `tools/list` responses; replaced several
  `serde_json::Value` bag-of-fields with concrete structs.
- Unified `RawUsage` / OpenAI usage fields; frontend TypeScript types
  regenerated to match.
- Broke the `protocol` ↔ `mcp` module cycle in `axon-core` and
  consolidated shared utilities.
- Extracted reusable page primitives for the desktop app.

### Fixed
- `serve-mcp` now propagates MCP result serialization errors instead of
  silently dropping them.
- `axon trust` surfaces hex-decode errors for peer-id arguments.
- `gossip` uses `checked_div` (clippy `manual_checked_ops`).

### Removed
- Dead code flagged by `cargo` warnings and `knip`, including
  `swarm_dispatch` references and the `sendChat` stub.

## 2026-04-16 — worktree agent sweep

Six parallel worktree agents hardened the codebase:

- Untangled the `orchestrate` module cycle via a dedicated `types.rs`.
- Strengthened weak types across the workspace.
- Removed deprecated/legacy/fallback code paths.
- Pruned gratuitous defensive error handling.
- Removed AI-generated slop comments.
- Consolidated duplicated type definitions.

## 2026-04-11 — UI revamp + desktop app

### Added
- **axon-desktop** — Tauri desktop app with frameless window, custom
  titlebar, command palette, full onboarding wizard, settings hub, and a
  clean chat view.
- **Agent graph view** — force-directed network visualization of the
  mesh.
- Workflows + Blackboard exposed in the TUI, web API, and React frontend.
- CRDT counters, peer trust, and throughput history wired into the sync
  tick.

### Changed
- Monochrome redesign across all UIs — black/white/gray with Inter font,
  no accent colors in the desktop and web surfaces.
- TUI revamped with a more vibrant layout before the subsequent
  monochrome pass.

## 2026-04-09 — chat TUI + orchestration

### Added
- `axon ask` (one-shot prompt) and `axon chat` (interactive TUI).
- Premium chat TUI with slash commands, async LLM, conversation history,
  predictive autocomplete, interactive model picker, background job
  scheduling, auto-agent mode, and orchestration commands.
- `orchestrate` module — agent scaffolding, workflow primitives
  (pipeline / fan-out / supervisor), shared Blackboard, capability-gated
  hooks, and lifecycle management with heartbeat.
- Onboarding wizard plus `auth` / `models` / `setup` CLI commands and a
  Settings tab.

### Changed
- Muted, darker monochrome palette adopted across all TUIs.
- Transparent TUI surfaces; compact provider list in onboarding.

## 2026-04-08 — provider catalog

### Added
- Support for 9 additional LLM providers (Anthropic, Gemini, Mistral,
  Groq, Together, DeepSeek, Fireworks, Cohere, Perplexity), later
  simplified to Ollama, xAI, OpenRouter, and Custom.
- TUI dashboard overhaul: richer mesh view, agent cards, CRDT state
  viewer.

## 2026-03-23 / 2026-03-24 — trust, negotiation, MCP

### Added
- **Config file support**, graceful shutdown, and node metrics.
- **Persistent task queue** (sled-backed) with crash recovery and
  automatic retries; background drain worker and task forwarding.
- **Identity-derived mutual TLS** certificates and a `/health` endpoint.
- **MCP tool registry** in `axon-core`, MCP Bridge Agent for
  decentralized tool gateway, ToolCatalog gossip, budget-constrained
  tool selection, end-to-end MCP integration tests, and the
  `axon serve-mcp [--mesh]` stdio gateway.
- **Negotiation protocol** — agent-to-agent task bidding
  (TaskOffer → bid collection → winner dispatch).
- **Trust / reputation system** — subjective, experience-based,
  decay-weighted per-peer scores; persistent sled-backed store; wired
  into transport and negotiation bid scoring.
- GitHub Actions CI pipeline and badge.

## 2026-03-22

### Fixed / hardened
- Saturating arithmetic across counters and stats (incl. `GCounter::value`).
- `avg_latency_ms` now computed over successful tasks only.
- Wildcard discover handled; closed connections pruned.
- Identity key file forced to `0600` permissions on Unix.
- `task_log` capped at 1000 entries; `Pong` messages handled.

## 2026-03-06 / 2026-02-25

### Fixed
- Replaced `unwrap()` in TLS config generation with proper error
  propagation.
- `hex_decode` no longer panics on odd-length or non-ASCII input.
- Peer identity verification and task timeouts added; `peers` command
  introduced.
- Nightly-only `is_multiple_of` removed from gossip eviction;
  division-by-zero guarded.

## 2026-02-22 — initial skeleton

First public drop. Included:

- Workspace layout: `axon-core`, `axon-sdk`, `axon-cli`.
- Binary protocol with capabilities and core message types.
- Ed25519 identity and key management.
- CRDTs: `GCounter`, `LWWRegister`, `ORSet`.
- Capability-based router (BestMatch, RoundRobin, Broadcast).
- QUIC transport with self-signed TLS; agent runtime with
  capability-based dispatch.
- Peer table plus mDNS LAN discovery and gossip for mesh-wide
  propagation.
- Multi-provider LLM system (Ollama, OpenAI, xAI, OpenRouter) and
  built-in agents: Echo, SystemInfo, LLM.
- TUI dashboard (mesh / agents / tasks / state / logs) and CLI entry
  point (`start`, `send`, `status`, `peers`, `identity`).
- mdBook documentation with architecture, agent, and operations guides.
- Integration tests covering two-node exchange, peer table, runtime,
  and CRDTs.
