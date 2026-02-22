# TUI Dashboard

The interactive terminal dashboard shows real-time mesh status.

## Launch

```bash
axon-cli start  # TUI is the default
```

To disable: `axon-cli start --headless`

## Tabs

| Key | Tab | Contents |
|-----|-----|----------|
| `1` | Mesh | Connected peers, their addresses, capabilities, and last-seen times |
| `2` | Agents | Registered agents and their capabilities |
| `3` | Tasks | Task execution log with IDs, capabilities, status, duration, and source peer |
| `4` | State | CRDT state viewer |
| `5` | Logs | Event log with timestamps |

## Keybindings

| Key | Action |
|-----|--------|
| `Tab` / `Shift+Tab` | Next / previous tab |
| `1`-`5` | Jump to tab |
| `j` / `k` | Scroll down / up |
| `q` | Quit |

## Layout

```
┌─ Axon Mesh ─────────────────────────────────┐
│ [Mesh] [Agents] [Tasks] [State] [Logs]      │
├──────────────────────────────────────────────┤
│                                              │
│  Tab-specific content here                   │
│                                              │
├──────────────────────────────────────────────┤
│ Peer: a1b2c3d4 | 0.0.0.0:4242 | 3 agents   │
└──────────────────────────────────────────────┘
```
