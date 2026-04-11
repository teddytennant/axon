use axon_core::{Capability, PeerInfo};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    prelude::*,
    widgets::{
        Block, Borders, Cell, List, ListItem, Padding, Paragraph, Row, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Sparkline, Table, Tabs, Wrap,
    },
};
use std::collections::VecDeque;
use std::io::stdout;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

// ---------------------------------------------------------------------------
// Tab enum
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Mesh,
    Agents,
    Tasks,
    State,
    Logs,
    Settings,
    Workflows,
}

impl Tab {
    pub fn all() -> Vec<Tab> {
        vec![
            Tab::Mesh,
            Tab::Agents,
            Tab::Tasks,
            Tab::State,
            Tab::Logs,
            Tab::Settings,
            Tab::Workflows,
        ]
    }

    pub fn title(&self) -> &str {
        match self {
            Tab::Mesh => "Mesh",
            Tab::Agents => "Agents",
            Tab::Tasks => "Tasks",
            Tab::State => "State",
            Tab::Logs => "Logs",
            Tab::Settings => "Settings",
            Tab::Workflows => "Workflows",
        }
    }

    pub fn icon(&self) -> &str {
        match self {
            Tab::Mesh => "\u{25c6}",      // diamond
            Tab::Agents => "\u{2726}",    // four-pointed star
            Tab::Tasks => "\u{25b6}",     // play triangle
            Tab::State => "\u{2637}",     // trigram
            Tab::Logs => "\u{2261}",      // triple bar
            Tab::Settings => "\u{2699}",  // gear
            Tab::Workflows => "\u{27a4}", // arrow
        }
    }

    pub fn index(&self) -> usize {
        match self {
            Tab::Mesh => 0,
            Tab::Agents => 1,
            Tab::Tasks => 2,
            Tab::State => 3,
            Tab::Logs => 4,
            Tab::Settings => 5,
            Tab::Workflows => 6,
        }
    }

    pub fn from_index(i: usize) -> Self {
        match i {
            0 => Tab::Mesh,
            1 => Tab::Agents,
            2 => Tab::Tasks,
            3 => Tab::State,
            4 => Tab::Logs,
            5 => Tab::Settings,
            6 => Tab::Workflows,
            _ => Tab::Mesh,
        }
    }

    pub fn count() -> usize {
        7
    }
}

// ---------------------------------------------------------------------------
// Log level filter for the Logs tab
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFilter {
    All,
    WarningsUp,
    ErrorsOnly,
}

impl LogFilter {
    fn next(self) -> Self {
        match self {
            LogFilter::All => LogFilter::WarningsUp,
            LogFilter::WarningsUp => LogFilter::ErrorsOnly,
            LogFilter::ErrorsOnly => LogFilter::All,
        }
    }

    fn label(self) -> &'static str {
        match self {
            LogFilter::All => "ALL",
            LogFilter::WarningsUp => "WARN+",
            LogFilter::ErrorsOnly => "ERR",
        }
    }

    fn matches(self, line: &str) -> bool {
        match self {
            LogFilter::All => true,
            LogFilter::WarningsUp => {
                let up = line.to_ascii_uppercase();
                up.contains("ERROR") || up.contains("WARN")
            }
            LogFilter::ErrorsOnly => {
                let up = line.to_ascii_uppercase();
                up.contains("ERROR")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Workflow snapshots for the Workflows tab
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct WorkflowSnapshot {
    pub id: String,
    pub pattern: String,
    pub steps_completed: usize,
    pub steps_total: usize,
    pub status: String,
    pub duration_ms: u64,
    pub started_at: String,
    pub steps: Vec<StepSnapshot>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct StepSnapshot {
    pub capability: String,
    pub status: String,
    pub latency_ms: u64,
    pub payload_bytes: usize,
}

// ---------------------------------------------------------------------------
// Agent info for rich agent cards
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct AgentInfo {
    pub name: String,
    pub capabilities: Vec<String>,
    pub provider_type: String,
    pub model_name: String,
    pub status: AgentStatus,
    pub tasks_handled: u64,
    pub tasks_succeeded: u64,
    pub avg_latency_ms: u64,
    pub lifecycle_state: String,
    pub last_heartbeat_secs_ago: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    Idle,
    #[allow(dead_code)]
    Processing,
    #[allow(dead_code)]
    Error,
}

impl AgentStatus {
    fn label(self) -> &'static str {
        match self {
            AgentStatus::Idle => "IDLE",
            AgentStatus::Processing => "BUSY",
            AgentStatus::Error => "ERR",
        }
    }

    fn color(self) -> Color {
        match self {
            AgentStatus::Idle => Color::Green,
            AgentStatus::Processing => Color::Yellow,
            AgentStatus::Error => Color::Red,
        }
    }
}

// ---------------------------------------------------------------------------
// DashboardState
// ---------------------------------------------------------------------------

/// Shared state between the TUI and the mesh node.
pub struct DashboardState {
    pub peer_id: String,
    pub listen_addr: String,
    pub peers: Vec<PeerInfo>,
    pub agent_names: Vec<String>,
    pub capabilities: Vec<Capability>,
    pub task_log: Vec<TaskLogEntry>,
    pub logs: VecDeque<String>,
    pub uptime_secs: u64,
    pub tasks_total: u64,
    pub tasks_failed: u64,

    // Rich agent info (populated by main.rs if available, else derived from agent_names)
    pub agent_info: Vec<AgentInfo>,

    // CRDT state fields — populated from node metrics on each sync tick.
    pub crdt_counters: Vec<(String, u64)>,
    pub crdt_registers: Vec<(String, String)>,
    pub crdt_sets: Vec<(String, Vec<String>)>,

    // Peer trust scores (peer_id_hex -> overall trust 0.0..1.0)
    // Populated from TrustStore on each sync tick.
    pub peer_trust: Vec<(String, f64)>,

    // Task throughput history (tasks completed per second, last 60 samples)
    pub throughput_history: VecDeque<u64>,

    // Settings tab info
    pub provider_name: String,
    pub model_name: String,
    pub config_path: String,
    pub mcp_server_count: usize,
    pub mcp_tool_count: usize,

    // Workflows tab: active + recently completed workflow snapshots
    pub active_workflows: Vec<WorkflowSnapshot>,
    pub completed_workflows: VecDeque<WorkflowSnapshot>,

    // Blackboard entries: (key, value_preview, timestamp_ms)
    // Populated from Blackboard::snapshot() on each sync tick.
    pub blackboard_entries: Vec<(String, String, u64)>,
}

#[derive(Debug, Clone)]
pub struct TaskLogEntry {
    pub id: String,
    pub capability: String,
    pub status: String,
    pub duration_ms: u64,
    pub peer: String,
}

impl DashboardState {
    pub fn new(peer_id: String, listen_addr: String) -> Self {
        Self {
            peer_id,
            listen_addr,
            peers: Vec::new(),
            agent_names: Vec::new(),
            capabilities: Vec::new(),
            task_log: Vec::new(),
            logs: VecDeque::new(),
            uptime_secs: 0,
            tasks_total: 0,
            tasks_failed: 0,
            agent_info: Vec::new(),
            crdt_counters: Vec::new(),
            crdt_registers: Vec::new(),
            crdt_sets: Vec::new(),
            peer_trust: Vec::new(),
            throughput_history: VecDeque::new(),
            provider_name: String::new(),
            model_name: String::new(),
            config_path: String::new(),
            mcp_server_count: 0,
            mcp_tool_count: 0,
            active_workflows: Vec::new(),
            completed_workflows: VecDeque::new(),
            blackboard_entries: Vec::new(),
        }
    }

    pub fn add_log(&mut self, msg: String) {
        self.logs.push_back(msg);
        if self.logs.len() > 1000 {
            self.logs.pop_front();
        }
    }
}

// ---------------------------------------------------------------------------
// Theme constants
// ---------------------------------------------------------------------------

const BRAND_CYAN: Color = Color::Rgb(110, 110, 118);
const BRAND_GREEN: Color = Color::Rgb(95, 140, 105);
const BRAND_YELLOW: Color = Color::Rgb(150, 135, 70);
const BRAND_RED: Color = Color::Rgb(150, 70, 70);
const BRAND_DIM: Color = Color::Rgb(70, 70, 78);
const BRAND_BG: Color = Color::Reset;
const ACCENT_BLUE: Color = Color::Rgb(100, 115, 150);
const SURFACE: Color = Color::Reset;

// ---------------------------------------------------------------------------
// Dashboard
// ---------------------------------------------------------------------------

pub struct Dashboard {
    state: Arc<RwLock<DashboardState>>,
    active_tab: Tab,
    scroll_offset: usize,
    log_filter: LogFilter,
}

impl Dashboard {
    pub fn new(state: Arc<RwLock<DashboardState>>) -> Self {
        Self {
            state,
            active_tab: Tab::Mesh,
            scroll_offset: 0,
            log_filter: LogFilter::All,
        }
    }

    pub async fn run(&mut self) -> std::io::Result<()> {
        enable_raw_mode()?;
        stdout().execute(EnterAlternateScreen)?;
        let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

        loop {
            let state = self.state.read().await;
            let active_tab = self.active_tab;
            let scroll = self.scroll_offset;
            let log_filter = self.log_filter;

            terminal.draw(|frame| {
                Self::render(frame, &state, active_tab, scroll, log_filter);
            })?;
            drop(state);

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if self.handle_key(key) {
                        break;
                    }
                }
            }
        }

        disable_raw_mode()?;
        stdout().execute(LeaveAlternateScreen)?;
        Ok(())
    }

    /// Returns true if the dashboard should exit.
    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('q') => return true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
            KeyCode::Tab => {
                let idx = (self.active_tab.index() + 1) % Tab::count();
                self.active_tab = Tab::from_index(idx);
                self.scroll_offset = 0;
            }
            KeyCode::BackTab => {
                let idx = (self.active_tab.index() + Tab::count() - 1) % Tab::count();
                self.active_tab = Tab::from_index(idx);
                self.scroll_offset = 0;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.scroll_offset = self.scroll_offset.saturating_add(1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
            }
            KeyCode::Char('g') => {
                self.scroll_offset = 0;
            }
            KeyCode::Char('G') => {
                self.scroll_offset = usize::MAX;
            }
            KeyCode::Char('f') if self.active_tab == Tab::Logs => {
                self.log_filter = self.log_filter.next();
            }
            KeyCode::Char('1') => {
                self.active_tab = Tab::Mesh;
                self.scroll_offset = 0;
            }
            KeyCode::Char('2') => {
                self.active_tab = Tab::Agents;
                self.scroll_offset = 0;
            }
            KeyCode::Char('3') => {
                self.active_tab = Tab::Tasks;
                self.scroll_offset = 0;
            }
            KeyCode::Char('4') => {
                self.active_tab = Tab::State;
                self.scroll_offset = 0;
            }
            KeyCode::Char('5') => {
                self.active_tab = Tab::Logs;
                self.scroll_offset = 0;
            }
            KeyCode::Char('6') => {
                self.active_tab = Tab::Settings;
                self.scroll_offset = 0;
            }
            KeyCode::Char('7') => {
                self.active_tab = Tab::Workflows;
                self.scroll_offset = 0;
            }
            _ => {}
        }
        false
    }

    // -----------------------------------------------------------------------
    // Main render
    // -----------------------------------------------------------------------

    fn render(
        frame: &mut Frame,
        state: &DashboardState,
        active_tab: Tab,
        scroll: usize,
        log_filter: LogFilter,
    ) {
        let area = frame.area();

        // Background fill
        frame.render_widget(Block::default().style(Style::default().bg(BRAND_BG)), area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Header
                Constraint::Length(3), // Tabs
                Constraint::Min(0),    // Content
                Constraint::Length(1), // Status bar
            ])
            .split(area);

        Self::render_header(frame, state, chunks[0]);
        Self::render_tabs(frame, active_tab, chunks[1]);

        match active_tab {
            Tab::Mesh => Self::render_mesh(frame, state, chunks[2], scroll),
            Tab::Agents => Self::render_agents(frame, state, chunks[2], scroll),
            Tab::Tasks => Self::render_tasks(frame, state, chunks[2], scroll),
            Tab::State => Self::render_state(frame, state, chunks[2], scroll),
            Tab::Logs => Self::render_logs(frame, state, chunks[2], scroll, log_filter),
            Tab::Settings => Self::render_settings(frame, state, chunks[2]),
            Tab::Workflows => Self::render_workflows(frame, state, chunks[2], scroll),
        }

        Self::render_status_bar(frame, active_tab, log_filter, chunks[3]);
    }

    // -----------------------------------------------------------------------
    // Header
    // -----------------------------------------------------------------------

    fn render_header(frame: &mut Frame, state: &DashboardState, area: Rect) {
        let uptime = format_uptime(state.uptime_secs);
        let success_rate = if state.tasks_total > 0 {
            let succeeded = state.tasks_total.saturating_sub(state.tasks_failed);
            format!(
                "{}%",
                (succeeded as f64 / state.tasks_total as f64 * 100.0) as u64
            )
        } else {
            "--".to_string()
        };

        let peer_short = if state.peer_id.len() >= 8 {
            &state.peer_id[..8]
        } else {
            &state.peer_id
        };

        let header_spans = vec![
            Span::styled(" \u{25b2} AXON ", Style::default().fg(BRAND_CYAN).bold()),
            Span::styled("\u{2502} ", Style::default().fg(BRAND_DIM)),
            Span::styled(peer_short, Style::default().fg(Color::White).bold()),
            Span::styled(
                format!(" @ {}", state.listen_addr),
                Style::default().fg(BRAND_DIM),
            ),
            Span::styled(" \u{2502} ", Style::default().fg(BRAND_DIM)),
            Span::styled(
                format!("{}", state.peers.len()),
                Style::default().fg(BRAND_GREEN).bold(),
            ),
            Span::styled(" peers", Style::default().fg(BRAND_DIM)),
            Span::styled(" \u{2502} ", Style::default().fg(BRAND_DIM)),
            Span::styled(
                format!("{}", state.tasks_total),
                Style::default().fg(ACCENT_BLUE).bold(),
            ),
            Span::styled(" tasks ", Style::default().fg(BRAND_DIM)),
            Span::styled(
                format!("({})", success_rate),
                Style::default().fg(if state.tasks_failed == 0 {
                    BRAND_GREEN
                } else {
                    BRAND_YELLOW
                }),
            ),
            Span::styled(" \u{2502} ", Style::default().fg(BRAND_DIM)),
            Span::styled("\u{25f7} ", Style::default().fg(BRAND_DIM)),
            Span::styled(uptime, Style::default().fg(Color::White)),
        ];

        let header = Paragraph::new(Line::from(header_spans)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BRAND_DIM))
                .style(Style::default().bg(SURFACE)),
        );
        frame.render_widget(header, area);
    }

    // -----------------------------------------------------------------------
    // Tabs
    // -----------------------------------------------------------------------

    fn render_tabs(frame: &mut Frame, active_tab: Tab, area: Rect) {
        let tab_titles: Vec<Line> = Tab::all()
            .iter()
            .enumerate()
            .map(|(i, t)| {
                if *t == active_tab {
                    Line::from(vec![
                        Span::styled(format!(" {} ", t.icon()), Style::default().fg(BRAND_CYAN)),
                        Span::styled(
                            format!("{} {} ", i + 1, t.title()),
                            Style::default().fg(BRAND_CYAN).bold(),
                        ),
                    ])
                } else {
                    Line::from(format!(" {} {} {} ", t.icon(), i + 1, t.title()))
                        .style(Style::default().fg(BRAND_DIM))
                }
            })
            .collect();

        let tabs = Tabs::new(tab_titles)
            .select(active_tab.index())
            .highlight_style(Style::default().fg(BRAND_CYAN).bold())
            .divider(Span::styled(" \u{2502} ", Style::default().fg(BRAND_DIM)))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(BRAND_DIM))
                    .style(Style::default().bg(SURFACE)),
            );
        frame.render_widget(tabs, area);
    }

    // -----------------------------------------------------------------------
    // Status bar
    // -----------------------------------------------------------------------

    fn render_status_bar(frame: &mut Frame, active_tab: Tab, log_filter: LogFilter, area: Rect) {
        let mut spans = vec![
            Span::styled(" q", Style::default().fg(BRAND_CYAN).bold()),
            Span::styled(" quit ", Style::default().fg(BRAND_DIM)),
            Span::styled("\u{2502}", Style::default().fg(BRAND_DIM)),
            Span::styled(" Tab", Style::default().fg(BRAND_CYAN).bold()),
            Span::styled(" switch ", Style::default().fg(BRAND_DIM)),
            Span::styled("\u{2502}", Style::default().fg(BRAND_DIM)),
            Span::styled(" j/k", Style::default().fg(BRAND_CYAN).bold()),
            Span::styled(" scroll ", Style::default().fg(BRAND_DIM)),
            Span::styled("\u{2502}", Style::default().fg(BRAND_DIM)),
            Span::styled(" 1-6", Style::default().fg(BRAND_CYAN).bold()),
            Span::styled(" jump ", Style::default().fg(BRAND_DIM)),
            Span::styled("\u{2502}", Style::default().fg(BRAND_DIM)),
            Span::styled(" g/G", Style::default().fg(BRAND_CYAN).bold()),
            Span::styled(" top/bot ", Style::default().fg(BRAND_DIM)),
        ];

        if active_tab == Tab::Logs {
            spans.push(Span::styled("\u{2502}", Style::default().fg(BRAND_DIM)));
            spans.push(Span::styled(" f", Style::default().fg(BRAND_CYAN).bold()));
            spans.push(Span::styled(
                format!(" filter:{} ", log_filter.label()),
                Style::default().fg(BRAND_DIM),
            ));
        }

        let bar =
            Paragraph::new(Line::from(spans)).style(Style::default().bg(SURFACE).fg(BRAND_DIM));
        frame.render_widget(bar, area);
    }

    // -----------------------------------------------------------------------
    // Mesh tab
    // -----------------------------------------------------------------------

    fn render_mesh(frame: &mut Frame, state: &DashboardState, area: Rect, scroll: usize) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(5), // Local node + network health
                Constraint::Min(0),    // Peer cards
            ])
            .split(area);

        // --- Local node info & network health ---
        Self::render_mesh_header(frame, state, chunks[0]);

        // --- Peer list ---
        if state.peers.is_empty() {
            let msg = Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  Scanning for peers...",
                    Style::default().fg(BRAND_DIM).italic(),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "  Waiting for mesh connections via mDNS discovery and gossip protocol.",
                    Style::default().fg(BRAND_DIM),
                )),
            ])
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(BRAND_DIM))
                    .title(Span::styled(
                        " Mesh Peers ",
                        Style::default().fg(BRAND_CYAN).bold(),
                    ))
                    .style(Style::default().bg(SURFACE)),
            );
            frame.render_widget(msg, chunks[1]);
            return;
        }

        // Build trust lookup
        let trust_map: std::collections::HashMap<&str, f64> = state
            .peer_trust
            .iter()
            .map(|(k, v)| (k.as_str(), *v))
            .collect();

        let rows: Vec<Row> = state
            .peers
            .iter()
            .skip(scroll)
            .map(|p| {
                let id_hex = peer_id_hex(&p.peer_id);
                let id_short = if id_hex.len() >= 8 {
                    &id_hex[..8]
                } else {
                    &id_hex
                };

                let trust_val = trust_map.get(id_hex.as_str()).copied().unwrap_or(0.5);
                let trust_color = trust_color(trust_val);
                let trust_label = format!("{:.0}%", trust_val * 100.0);

                let caps = p
                    .capabilities
                    .iter()
                    .map(|c: &Capability| c.tag())
                    .collect::<Vec<_>>()
                    .join(", ");

                let elapsed = elapsed_secs(p.last_seen);
                let seen_str = format_elapsed(elapsed);
                let seen_color = if elapsed < 10 {
                    BRAND_GREEN
                } else if elapsed < 60 {
                    BRAND_YELLOW
                } else {
                    BRAND_RED
                };

                Row::new(vec![
                    Cell::from(Span::styled(
                        format!(" {}", id_short),
                        Style::default().fg(trust_color).bold(),
                    )),
                    Cell::from(p.addr.clone()),
                    Cell::from(Span::styled(caps, Style::default().fg(ACCENT_BLUE))),
                    Cell::from(Span::styled(
                        trust_label,
                        Style::default().fg(trust_color).bold(),
                    )),
                    Cell::from(Span::styled(seen_str, Style::default().fg(seen_color))),
                ])
            })
            .collect();

        let table = Table::new(
            rows,
            [
                Constraint::Length(11),
                Constraint::Length(22),
                Constraint::Min(20),
                Constraint::Length(8),
                Constraint::Length(10),
            ],
        )
        .header(
            Row::new(vec![
                Cell::from(Span::styled(
                    " Peer",
                    Style::default().fg(BRAND_CYAN).bold(),
                )),
                Cell::from(Span::styled(
                    "Address",
                    Style::default().fg(BRAND_CYAN).bold(),
                )),
                Cell::from(Span::styled(
                    "Capabilities",
                    Style::default().fg(BRAND_CYAN).bold(),
                )),
                Cell::from(Span::styled(
                    "Trust",
                    Style::default().fg(BRAND_CYAN).bold(),
                )),
                Cell::from(Span::styled("Seen", Style::default().fg(BRAND_CYAN).bold())),
            ])
            .bottom_margin(1),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BRAND_DIM))
                .title(Span::styled(
                    " Mesh Peers ",
                    Style::default().fg(BRAND_CYAN).bold(),
                ))
                .style(Style::default().bg(SURFACE)),
        );

        frame.render_widget(table, chunks[1]);

        // Scrollbar
        if state.peers.len() > chunks[1].height.saturating_sub(4) as usize {
            let mut scrollbar_state = ScrollbarState::new(state.peers.len()).position(scroll);
            frame.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .style(Style::default().fg(BRAND_DIM)),
                chunks[1],
                &mut scrollbar_state,
            );
        }
    }

    fn render_mesh_header(frame: &mut Frame, state: &DashboardState, area: Rect) {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        // Local node card
        let peer_short = if state.peer_id.len() >= 12 {
            &state.peer_id[..12]
        } else {
            &state.peer_id
        };
        let caps_str = state
            .capabilities
            .iter()
            .map(|c| c.tag())
            .collect::<Vec<_>>()
            .join(", ");

        let local_card = Paragraph::new(vec![
            Line::from(vec![
                Span::styled(" \u{25c9} ", Style::default().fg(BRAND_GREEN)),
                Span::styled("LOCAL NODE", Style::default().fg(BRAND_GREEN).bold()),
                Span::styled(
                    format!("  {}", peer_short),
                    Style::default().fg(Color::White),
                ),
            ]),
            Line::from(vec![
                Span::styled("   addr: ", Style::default().fg(BRAND_DIM)),
                Span::styled(&state.listen_addr, Style::default().fg(Color::White)),
                Span::styled("  caps: ", Style::default().fg(BRAND_DIM)),
                Span::styled(
                    if caps_str.is_empty() {
                        "none".to_string()
                    } else {
                        caps_str
                    },
                    Style::default().fg(ACCENT_BLUE),
                ),
            ]),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BRAND_GREEN))
                .padding(Padding::horizontal(1))
                .style(Style::default().bg(SURFACE)),
        );
        frame.render_widget(local_card, cols[0]);

        // Network health summary
        let total_peers = state.peers.len();
        let avg_trust = if !state.peer_trust.is_empty() {
            let sum: f64 = state.peer_trust.iter().map(|(_, t)| t).sum();
            sum / state.peer_trust.len() as f64
        } else if total_peers > 0 {
            0.5 // default neutral
        } else {
            0.0
        };

        let trusted_count = state.peer_trust.iter().filter(|(_, t)| *t >= 0.7).count();

        let recent_count = state
            .peers
            .iter()
            .filter(|p| elapsed_secs(p.last_seen) < 30)
            .count();

        let connectivity = if total_peers > 0 {
            recent_count as f64 / total_peers as f64
        } else {
            0.0
        };

        let health_color = if connectivity >= 0.8 {
            BRAND_GREEN
        } else if connectivity >= 0.5 {
            BRAND_YELLOW
        } else {
            BRAND_RED
        };

        let health_card = Paragraph::new(vec![
            Line::from(vec![
                Span::styled(" NETWORK HEALTH", Style::default().fg(health_color).bold()),
                Span::styled(
                    format!("  {:.0}% connectivity", connectivity * 100.0),
                    Style::default().fg(health_color),
                ),
            ]),
            Line::from(vec![
                Span::styled("   peers: ", Style::default().fg(BRAND_DIM)),
                Span::styled(
                    format!("{}", total_peers),
                    Style::default().fg(Color::White).bold(),
                ),
                Span::styled("  trusted: ", Style::default().fg(BRAND_DIM)),
                Span::styled(
                    format!("{}", trusted_count),
                    Style::default().fg(BRAND_GREEN),
                ),
                Span::styled("  avg trust: ", Style::default().fg(BRAND_DIM)),
                Span::styled(
                    format!("{:.0}%", avg_trust * 100.0),
                    Style::default().fg(trust_color(avg_trust)),
                ),
                Span::styled("  active: ", Style::default().fg(BRAND_DIM)),
                Span::styled(
                    format!("{}/{}", recent_count, total_peers),
                    Style::default().fg(health_color),
                ),
            ]),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BRAND_DIM))
                .padding(Padding::horizontal(1))
                .style(Style::default().bg(SURFACE)),
        );
        frame.render_widget(health_card, cols[1]);
    }

    // -----------------------------------------------------------------------
    // Agents tab
    // -----------------------------------------------------------------------

    fn render_agents(frame: &mut Frame, state: &DashboardState, area: Rect, scroll: usize) {
        // If we have rich agent info, use that; otherwise fall back to names+caps
        if !state.agent_info.is_empty() {
            Self::render_agents_rich(frame, state, area, scroll);
        } else {
            Self::render_agents_basic(frame, state, area, scroll);
        }
    }

    fn render_agents_rich(frame: &mut Frame, state: &DashboardState, area: Rect, scroll: usize) {
        // Each agent card takes 4 lines + 1 spacing
        let card_height: u16 = 4;
        let available = area.height.saturating_sub(2); // borders
        let visible_cards = (available / card_height) as usize;
        let total = state.agent_info.len();
        let scroll = scroll.min(total.saturating_sub(visible_cards));

        let mut lines: Vec<Line> = Vec::new();

        for (i, agent) in state.agent_info.iter().enumerate().skip(scroll) {
            if lines.len() >= available as usize {
                break;
            }

            let status_indicator = Span::styled(
                format!(" {} ", agent.status.label()),
                Style::default()
                    .fg(Color::Black)
                    .bg(agent.status.color())
                    .bold(),
            );

            let success_rate = if agent.tasks_handled > 0 {
                format!(
                    "{:.0}%",
                    agent.tasks_succeeded as f64 / agent.tasks_handled as f64 * 100.0
                )
            } else {
                "--".to_string()
            };

            // Line 1: name + status + provider
            lines.push(Line::from(vec![
                Span::styled(format!("  {:>2}. ", i + 1), Style::default().fg(BRAND_DIM)),
                Span::styled(&agent.name, Style::default().fg(Color::White).bold()),
                Span::raw("  "),
                status_indicator,
                Span::styled(
                    if agent.provider_type.is_empty() {
                        String::new()
                    } else {
                        format!("  [{}]", agent.provider_type)
                    },
                    Style::default().fg(BRAND_DIM),
                ),
                Span::styled(
                    if agent.model_name.is_empty() {
                        String::new()
                    } else {
                        format!(" {}", agent.model_name)
                    },
                    Style::default().fg(ACCENT_BLUE),
                ),
            ]));

            // Line 2: capabilities
            let caps_str = agent.capabilities.join(", ");
            lines.push(Line::from(vec![
                Span::styled("       caps: ", Style::default().fg(BRAND_DIM)),
                Span::styled(caps_str, Style::default().fg(BRAND_CYAN)),
            ]));

            // Line 3: stats + lifecycle
            let lifecycle_color = match agent.lifecycle_state.as_str() {
                "Running" => BRAND_GREEN,
                "Paused" => BRAND_YELLOW,
                "Stopped" => BRAND_RED,
                _ => BRAND_DIM,
            };
            let heartbeat_str = match agent.last_heartbeat_secs_ago {
                Some(s) => format!("{}s ago", s),
                None => "--".to_string(),
            };
            lines.push(Line::from(vec![
                Span::styled("       tasks: ", Style::default().fg(BRAND_DIM)),
                Span::styled(
                    format!("{}", agent.tasks_handled),
                    Style::default().fg(Color::White),
                ),
                Span::styled("  success: ", Style::default().fg(BRAND_DIM)),
                Span::styled(success_rate, Style::default().fg(BRAND_GREEN)),
                Span::styled("  avg latency: ", Style::default().fg(BRAND_DIM)),
                Span::styled(
                    format!("{}ms", agent.avg_latency_ms),
                    Style::default().fg(Color::White),
                ),
                if !agent.lifecycle_state.is_empty() {
                    Span::styled(
                        format!("  \u{2502} {} ", agent.lifecycle_state),
                        Style::default().fg(lifecycle_color).bold(),
                    )
                } else {
                    Span::raw("")
                },
                if agent.last_heartbeat_secs_ago.is_some() {
                    Span::styled(
                        format!(" hb: {}", heartbeat_str),
                        Style::default().fg(BRAND_DIM),
                    )
                } else {
                    Span::raw("")
                },
            ]));

            // Separator
            lines.push(Line::from(Span::styled(
                "      \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
                Style::default().fg(Color::Rgb(50, 50, 56)),
            )));
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BRAND_DIM))
            .title(Span::styled(
                format!(" Agents ({}) ", state.agent_info.len()),
                Style::default().fg(BRAND_CYAN).bold(),
            ))
            .style(Style::default().bg(SURFACE));

        let paragraph = Paragraph::new(lines).block(block);
        frame.render_widget(paragraph, area);
    }

    fn render_agents_basic(frame: &mut Frame, state: &DashboardState, area: Rect, _scroll: usize) {
        let inner = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        // Agents list
        let items: Vec<ListItem> = state
            .agent_names
            .iter()
            .enumerate()
            .map(|(i, name)| {
                ListItem::new(Line::from(vec![
                    Span::styled(format!("  {:>2}. ", i + 1), Style::default().fg(BRAND_DIM)),
                    Span::styled(
                        format!("\u{25c9} {}", name),
                        Style::default().fg(Color::White),
                    ),
                    Span::styled(
                        format!("  {}", AgentStatus::Idle.label()),
                        Style::default().fg(BRAND_GREEN),
                    ),
                ]))
            })
            .collect();

        let agents_list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BRAND_DIM))
                .title(Span::styled(
                    format!(" Registered Agents ({}) ", state.agent_names.len()),
                    Style::default().fg(BRAND_CYAN).bold(),
                ))
                .style(Style::default().bg(SURFACE)),
        );
        frame.render_widget(agents_list, inner[0]);

        // Capabilities list
        let caps_items: Vec<ListItem> = state
            .capabilities
            .iter()
            .map(|c| {
                ListItem::new(Line::from(vec![
                    Span::styled("  \u{25b8} ", Style::default().fg(BRAND_DIM)),
                    Span::styled(c.tag(), Style::default().fg(BRAND_CYAN)),
                ]))
            })
            .collect();

        let caps_list = List::new(caps_items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BRAND_DIM))
                .title(Span::styled(
                    format!(" Capabilities ({}) ", state.capabilities.len()),
                    Style::default().fg(BRAND_CYAN).bold(),
                ))
                .style(Style::default().bg(SURFACE)),
        );
        frame.render_widget(caps_list, inner[1]);
    }

    // -----------------------------------------------------------------------
    // Tasks tab
    // -----------------------------------------------------------------------

    fn render_tasks(frame: &mut Frame, state: &DashboardState, area: Rect, scroll: usize) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Summary bar
                Constraint::Length(4), // Throughput sparkline
                Constraint::Min(0),    // Task table
            ])
            .split(area);

        // --- Summary bar ---
        Self::render_task_summary(frame, state, chunks[0]);

        // --- Throughput sparkline ---
        Self::render_throughput(frame, state, chunks[1]);

        // --- Task table ---
        if state.task_log.is_empty() {
            let msg = Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  No tasks dispatched yet.",
                    Style::default().fg(BRAND_DIM).italic(),
                )),
            ])
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(BRAND_DIM))
                    .title(Span::styled(
                        " Task Log ",
                        Style::default().fg(BRAND_CYAN).bold(),
                    ))
                    .style(Style::default().bg(SURFACE)),
            );
            frame.render_widget(msg, chunks[2]);
            return;
        }

        // Separate in-flight from completed
        let in_flight: Vec<&TaskLogEntry> = state
            .task_log
            .iter()
            .filter(|t| t.status == "InFlight" || t.status == "Pending")
            .collect();

        let completed: Vec<&TaskLogEntry> = state
            .task_log
            .iter()
            .filter(|t| t.status != "InFlight" && t.status != "Pending")
            .rev()
            .collect();

        let mut all_rows: Vec<Row> = Vec::new();

        // In-flight tasks first
        for t in &in_flight {
            all_rows.push(task_row(t, true));
        }

        // Separator if we have both
        if !in_flight.is_empty() && !completed.is_empty() {
            all_rows.push(Row::new(vec![Cell::from(Span::styled(
                "\u{2500}\u{2500}\u{2500} completed \u{2500}\u{2500}\u{2500}",
                Style::default().fg(BRAND_DIM),
            ))]));
        }

        // Completed tasks
        for t in completed.iter().skip(scroll) {
            all_rows.push(task_row(t, false));
        }

        let table = Table::new(
            all_rows,
            [
                Constraint::Length(10),
                Constraint::Min(15),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Length(10),
            ],
        )
        .header(
            Row::new(vec![
                Cell::from(Span::styled(
                    "Task ID",
                    Style::default().fg(BRAND_CYAN).bold(),
                )),
                Cell::from(Span::styled(
                    "Capability",
                    Style::default().fg(BRAND_CYAN).bold(),
                )),
                Cell::from(Span::styled(
                    "Status",
                    Style::default().fg(BRAND_CYAN).bold(),
                )),
                Cell::from(Span::styled(
                    "Latency",
                    Style::default().fg(BRAND_CYAN).bold(),
                )),
                Cell::from(Span::styled("Peer", Style::default().fg(BRAND_CYAN).bold())),
            ])
            .bottom_margin(1),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BRAND_DIM))
                .title(Span::styled(
                    format!(" Task Log ({}) ", state.task_log.len()),
                    Style::default().fg(BRAND_CYAN).bold(),
                ))
                .style(Style::default().bg(SURFACE)),
        );

        frame.render_widget(table, chunks[2]);

        // Scrollbar
        let content_len = in_flight.len() + completed.len();
        if content_len > chunks[2].height.saturating_sub(4) as usize {
            let mut scrollbar_state = ScrollbarState::new(content_len).position(scroll);
            frame.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .style(Style::default().fg(BRAND_DIM)),
                chunks[2],
                &mut scrollbar_state,
            );
        }
    }

    fn render_task_summary(frame: &mut Frame, state: &DashboardState, area: Rect) {
        let total = state.tasks_total;
        let failed = state.tasks_failed;
        let succeeded = total.saturating_sub(failed);

        let timeout_count = state
            .task_log
            .iter()
            .filter(|t| t.status == "Timeout")
            .count() as u64;
        let error_count = failed.saturating_sub(timeout_count);

        let success_pct = if total > 0 {
            format!("{:.1}%", succeeded as f64 / total as f64 * 100.0)
        } else {
            "--".to_string()
        };

        let in_flight = state
            .task_log
            .iter()
            .filter(|t| t.status == "InFlight" || t.status == "Pending")
            .count();

        let summary = Paragraph::new(Line::from(vec![
            Span::styled(" Total: ", Style::default().fg(BRAND_DIM)),
            Span::styled(
                format!("{}", total),
                Style::default().fg(Color::White).bold(),
            ),
            Span::styled("  \u{2502}  ", Style::default().fg(BRAND_DIM)),
            Span::styled("\u{25cf} ", Style::default().fg(BRAND_GREEN)),
            Span::styled(
                format!("{} ok", succeeded),
                Style::default().fg(BRAND_GREEN),
            ),
            Span::styled("  ", Style::default()),
            Span::styled("\u{25cf} ", Style::default().fg(BRAND_RED)),
            Span::styled(
                format!("{} err", error_count),
                Style::default().fg(BRAND_RED),
            ),
            Span::styled("  ", Style::default()),
            Span::styled("\u{25cf} ", Style::default().fg(BRAND_YELLOW)),
            Span::styled(
                format!("{} timeout", timeout_count),
                Style::default().fg(BRAND_YELLOW),
            ),
            Span::styled("  \u{2502}  ", Style::default().fg(BRAND_DIM)),
            Span::styled(
                format!("{} in-flight", in_flight),
                Style::default().fg(ACCENT_BLUE).bold(),
            ),
            Span::styled("  \u{2502}  ", Style::default().fg(BRAND_DIM)),
            Span::styled("success: ", Style::default().fg(BRAND_DIM)),
            Span::styled(
                success_pct,
                Style::default()
                    .fg(if failed == 0 {
                        BRAND_GREEN
                    } else {
                        BRAND_YELLOW
                    })
                    .bold(),
            ),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BRAND_DIM))
                .title(Span::styled(
                    " Summary ",
                    Style::default().fg(BRAND_CYAN).bold(),
                ))
                .style(Style::default().bg(SURFACE)),
        );
        frame.render_widget(summary, area);
    }

    fn render_throughput(frame: &mut Frame, state: &DashboardState, area: Rect) {
        let data: Vec<u64> = state.throughput_history.iter().copied().collect();

        if data.is_empty() {
            let msg = Paragraph::new(Span::styled(
                " Throughput: waiting for data...",
                Style::default().fg(BRAND_DIM).italic(),
            ))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(BRAND_DIM))
                    .title(Span::styled(
                        " Throughput ",
                        Style::default().fg(BRAND_CYAN).bold(),
                    ))
                    .style(Style::default().bg(SURFACE)),
            );
            frame.render_widget(msg, area);
            return;
        }

        let max_val = data.iter().copied().max().unwrap_or(1).max(1);

        let sparkline = Sparkline::default()
            .data(&data)
            .max(max_val)
            .style(Style::default().fg(BRAND_GREEN))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(BRAND_DIM))
                    .title(Span::styled(
                        format!(" Throughput (last {}s, peak {}/s) ", data.len(), max_val),
                        Style::default().fg(BRAND_CYAN).bold(),
                    ))
                    .style(Style::default().bg(SURFACE)),
            );
        frame.render_widget(sparkline, area);
    }

    // -----------------------------------------------------------------------
    // State tab (CRDT viewer)
    // -----------------------------------------------------------------------

    fn render_state(frame: &mut Frame, state: &DashboardState, area: Rect, scroll: usize) {
        let has_data = !state.crdt_counters.is_empty()
            || !state.crdt_registers.is_empty()
            || !state.crdt_sets.is_empty()
            || !state.blackboard_entries.is_empty();

        if !has_data {
            let msg = Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  CRDT Shared State Viewer",
                    Style::default().fg(Color::White).bold(),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "  No CRDT state synced yet. State will appear as peers replicate.",
                    Style::default().fg(BRAND_DIM),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "  Supported types: GCounter, LWW-Register, OR-Set",
                    Style::default().fg(BRAND_DIM),
                )),
            ])
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(BRAND_DIM))
                    .title(Span::styled(
                        " Shared State ",
                        Style::default().fg(BRAND_CYAN).bold(),
                    ))
                    .style(Style::default().bg(SURFACE)),
            )
            .wrap(Wrap { trim: false });
            frame.render_widget(msg, area);
            return;
        }

        let mut lines: Vec<Line> = Vec::new();

        // GCounters
        if !state.crdt_counters.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  \u{25b8} GCounters",
                Style::default().fg(BRAND_CYAN).bold(),
            )));
            lines.push(Line::from(Span::styled(
                "  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
                Style::default().fg(Color::Rgb(50, 50, 56)),
            )));
            for (key, val) in &state.crdt_counters {
                lines.push(Line::from(vec![
                    Span::styled("    ", Style::default()),
                    Span::styled(key, Style::default().fg(Color::White)),
                    Span::styled(" = ", Style::default().fg(BRAND_DIM)),
                    Span::styled(format!("{}", val), Style::default().fg(BRAND_GREEN).bold()),
                ]));
            }
        }

        // LWW Registers
        if !state.crdt_registers.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  \u{25b8} LWW-Registers",
                Style::default().fg(BRAND_CYAN).bold(),
            )));
            lines.push(Line::from(Span::styled(
                "  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
                Style::default().fg(Color::Rgb(50, 50, 56)),
            )));
            for (key, val) in &state.crdt_registers {
                lines.push(Line::from(vec![
                    Span::styled("    ", Style::default()),
                    Span::styled(key, Style::default().fg(Color::White)),
                    Span::styled(" \u{2190} ", Style::default().fg(BRAND_DIM)),
                    Span::styled(format!("\"{}\"", val), Style::default().fg(BRAND_YELLOW)),
                ]));
            }
        }

        // OR-Sets
        if !state.crdt_sets.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  \u{25b8} OR-Sets",
                Style::default().fg(BRAND_CYAN).bold(),
            )));
            lines.push(Line::from(Span::styled(
                "  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
                Style::default().fg(Color::Rgb(50, 50, 56)),
            )));
            for (key, members) in &state.crdt_sets {
                lines.push(Line::from(vec![
                    Span::styled("    ", Style::default()),
                    Span::styled(key, Style::default().fg(Color::White)),
                    Span::styled(
                        format!(" ({} members)", members.len()),
                        Style::default().fg(BRAND_DIM),
                    ),
                ]));
                for member in members.iter().take(10) {
                    lines.push(Line::from(vec![
                        Span::styled("      \u{2022} ", Style::default().fg(BRAND_DIM)),
                        Span::styled(member, Style::default().fg(ACCENT_BLUE)),
                    ]));
                }
                if members.len() > 10 {
                    lines.push(Line::from(Span::styled(
                        format!("      ... and {} more", members.len() - 10),
                        Style::default().fg(BRAND_DIM),
                    )));
                }
            }
        }

        // Blackboard entries (bb: prefix from orchestrate)
        if !state.blackboard_entries.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  \u{25b8} Blackboard",
                Style::default().fg(BRAND_CYAN).bold(),
            )));
            lines.push(Line::from(Span::styled(
                "  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
                Style::default().fg(Color::Rgb(50, 50, 56)),
            )));
            for (key, preview, ts_ms) in &state.blackboard_entries {
                let ts_secs = ts_ms / 1000;
                lines.push(Line::from(vec![
                    Span::styled("    ", Style::default()),
                    Span::styled(key, Style::default().fg(Color::White)),
                    Span::styled(" \u{2190} ", Style::default().fg(BRAND_DIM)),
                    Span::styled(preview, Style::default().fg(BRAND_YELLOW)),
                    Span::styled(format!("  ({}s)", ts_secs), Style::default().fg(BRAND_DIM)),
                ]));
            }
        }

        let total_lines = lines.len();
        let visible = area.height.saturating_sub(2) as usize;
        let scroll = scroll.min(total_lines.saturating_sub(visible));
        let lines: Vec<Line> = lines.into_iter().skip(scroll).collect();

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BRAND_DIM))
            .title(Span::styled(
                format!(
                    " Shared State ({} counters, {} registers, {} sets, {} bb) ",
                    state.crdt_counters.len(),
                    state.crdt_registers.len(),
                    state.crdt_sets.len(),
                    state.blackboard_entries.len(),
                ),
                Style::default().fg(BRAND_CYAN).bold(),
            ))
            .style(Style::default().bg(SURFACE));

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        frame.render_widget(paragraph, area);

        // Scrollbar
        if total_lines > visible {
            let mut scrollbar_state = ScrollbarState::new(total_lines).position(scroll);
            frame.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .style(Style::default().fg(BRAND_DIM)),
                area,
                &mut scrollbar_state,
            );
        }
    }

    // -----------------------------------------------------------------------
    // Settings tab
    // -----------------------------------------------------------------------

    fn render_settings(frame: &mut Frame, state: &DashboardState, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(10), // Node info card
                Constraint::Length(9),  // LLM config card
                Constraint::Min(0),     // MCP / hints
            ])
            .split(area);

        // --- Node Info Card ---
        let peer_short = if state.peer_id.len() >= 16 {
            &state.peer_id[..16]
        } else {
            &state.peer_id
        };

        let node_lines = vec![
            Line::from(vec![
                Span::styled("  Peer ID:    ", Style::default().fg(BRAND_DIM)),
                Span::styled(peer_short, Style::default().fg(Color::White).bold()),
                Span::styled(
                    if state.peer_id.len() > 16 { "..." } else { "" },
                    Style::default().fg(BRAND_DIM),
                ),
            ]),
            Line::from(vec![
                Span::styled("  Listen:     ", Style::default().fg(BRAND_DIM)),
                Span::styled(&state.listen_addr, Style::default().fg(Color::White)),
            ]),
            Line::from(vec![
                Span::styled("  Uptime:     ", Style::default().fg(BRAND_DIM)),
                Span::styled(
                    format_uptime(state.uptime_secs),
                    Style::default().fg(BRAND_GREEN),
                ),
            ]),
            Line::from(vec![
                Span::styled("  Peers:      ", Style::default().fg(BRAND_DIM)),
                Span::styled(
                    format!("{}", state.peers.len()),
                    Style::default().fg(BRAND_GREEN).bold(),
                ),
            ]),
            Line::from(vec![
                Span::styled("  Agents:     ", Style::default().fg(BRAND_DIM)),
                Span::styled(
                    format!("{}", state.agent_names.len()),
                    Style::default().fg(ACCENT_BLUE),
                ),
                Span::styled(
                    format!("  ({})", state.agent_names.join(", ")),
                    Style::default().fg(BRAND_DIM),
                ),
            ]),
            Line::from(vec![
                Span::styled("  Config:     ", Style::default().fg(BRAND_DIM)),
                Span::styled(
                    if state.config_path.is_empty() {
                        "~/.config/axon/config.toml"
                    } else {
                        &state.config_path
                    },
                    Style::default().fg(BRAND_DIM),
                ),
            ]),
        ];

        let node_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BRAND_DIM))
            .title(Span::styled(
                " Node Info ",
                Style::default().fg(BRAND_CYAN).bold(),
            ))
            .style(Style::default().bg(SURFACE))
            .padding(Padding::vertical(1));

        frame.render_widget(Paragraph::new(node_lines).block(node_block), chunks[0]);

        // --- LLM Config Card ---
        let provider_display = if state.provider_name.is_empty() {
            "not configured"
        } else {
            &state.provider_name
        };

        let model_display = if state.model_name.is_empty() {
            "default"
        } else {
            &state.model_name
        };

        let llm_lines = vec![
            Line::from(vec![
                Span::styled("  Provider:   ", Style::default().fg(BRAND_DIM)),
                Span::styled(provider_display, Style::default().fg(BRAND_CYAN).bold()),
            ]),
            Line::from(vec![
                Span::styled("  Model:      ", Style::default().fg(BRAND_DIM)),
                Span::styled(model_display, Style::default().fg(ACCENT_BLUE).bold()),
            ]),
            Line::from(vec![
                Span::styled("  API Key:    ", Style::default().fg(BRAND_DIM)),
                Span::styled("configured", Style::default().fg(BRAND_GREEN)),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "  Run `axon auth <provider>` or `axon setup` to reconfigure",
                Style::default().fg(BRAND_DIM),
            )),
        ];

        let llm_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BRAND_DIM))
            .title(Span::styled(
                " LLM Configuration ",
                Style::default().fg(BRAND_CYAN).bold(),
            ))
            .style(Style::default().bg(SURFACE))
            .padding(Padding::vertical(1));

        frame.render_widget(Paragraph::new(llm_lines).block(llm_block), chunks[1]);

        // --- MCP & Commands Card ---
        let mcp_lines = vec![
            Line::from(vec![
                Span::styled("  MCP Servers:  ", Style::default().fg(BRAND_DIM)),
                Span::styled(
                    format!("{}", state.mcp_server_count),
                    Style::default().fg(Color::White).bold(),
                ),
                Span::styled("   Tools: ", Style::default().fg(BRAND_DIM)),
                Span::styled(
                    format!("{}", state.mcp_tool_count),
                    Style::default().fg(ACCENT_BLUE),
                ),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "  Quick Reference:",
                Style::default().fg(Color::White).bold(),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("    axon setup       ", Style::default().fg(BRAND_CYAN)),
                Span::styled("Re-run setup wizard", Style::default().fg(BRAND_DIM)),
            ]),
            Line::from(vec![
                Span::styled("    axon auth <p>    ", Style::default().fg(BRAND_CYAN)),
                Span::styled("Change provider/API key", Style::default().fg(BRAND_DIM)),
            ]),
            Line::from(vec![
                Span::styled("    axon models      ", Style::default().fg(BRAND_CYAN)),
                Span::styled("Browse available models", Style::default().fg(BRAND_DIM)),
            ]),
            Line::from(vec![
                Span::styled("    axon serve-mcp   ", Style::default().fg(BRAND_CYAN)),
                Span::styled("MCP gateway for AI tools", Style::default().fg(BRAND_DIM)),
            ]),
            Line::from(vec![
                Span::styled("    axon trust show  ", Style::default().fg(BRAND_CYAN)),
                Span::styled("View peer trust scores", Style::default().fg(BRAND_DIM)),
            ]),
            Line::from(vec![
                Span::styled("    axon init        ", Style::default().fg(BRAND_CYAN)),
                Span::styled("Generate example config", Style::default().fg(BRAND_DIM)),
            ]),
        ];

        let mcp_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BRAND_DIM))
            .title(Span::styled(
                " MCP & Commands ",
                Style::default().fg(BRAND_CYAN).bold(),
            ))
            .style(Style::default().bg(SURFACE))
            .padding(Padding::vertical(1));

        frame.render_widget(
            Paragraph::new(mcp_lines)
                .block(mcp_block)
                .wrap(Wrap { trim: false }),
            chunks[2],
        );
    }

    // -----------------------------------------------------------------------
    // Workflows tab
    // -----------------------------------------------------------------------

    fn render_workflows(frame: &mut Frame, state: &DashboardState, area: Rect, scroll: usize) {
        let all_workflows: Vec<&WorkflowSnapshot> = state
            .active_workflows
            .iter()
            .chain(state.completed_workflows.iter())
            .collect();

        if all_workflows.is_empty() && state.blackboard_entries.is_empty() {
            let msg = Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  Workflow Orchestration",
                    Style::default().fg(Color::White).bold(),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "  No workflows running. Use pipeline(), fan_out(), delegate(), or swarm_dispatch().",
                    Style::default().fg(BRAND_DIM),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "  Patterns: Pipeline \u{2192} FanOut \u{21c6} Delegate \u{27a4} Swarm \u{2731}",
                    Style::default().fg(BRAND_DIM),
                )),
            ])
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(BRAND_DIM))
                    .title(Span::styled(
                        " Workflows ",
                        Style::default().fg(BRAND_CYAN).bold(),
                    ))
                    .style(Style::default().bg(SURFACE)),
            )
            .wrap(Wrap { trim: false });
            frame.render_widget(msg, area);
            return;
        }

        // Split: top 60% workflow table, bottom 40% blackboard
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(area);

        // --- Workflow table ---
        let header = Row::new(vec![
            Cell::from("ID").style(Style::default().fg(BRAND_CYAN).bold()),
            Cell::from("Pattern").style(Style::default().fg(BRAND_CYAN).bold()),
            Cell::from("Steps").style(Style::default().fg(BRAND_CYAN).bold()),
            Cell::from("Status").style(Style::default().fg(BRAND_CYAN).bold()),
            Cell::from("Duration").style(Style::default().fg(BRAND_CYAN).bold()),
            Cell::from("Started").style(Style::default().fg(BRAND_CYAN).bold()),
        ])
        .height(1)
        .bottom_margin(0);

        let visible_rows = chunks[0].height.saturating_sub(4) as usize; // borders + header
        let scroll = scroll.min(all_workflows.len().saturating_sub(visible_rows));

        let rows: Vec<Row> = all_workflows
            .iter()
            .skip(scroll)
            .take(visible_rows)
            .map(|wf| {
                let id_short = if wf.id.len() > 12 {
                    format!("{}…", &wf.id[..12])
                } else {
                    wf.id.clone()
                };
                let steps_str = format!("{}/{}", wf.steps_completed, wf.steps_total);
                let (status_style, status_str) = match wf.status.as_str() {
                    "Running" => (Style::default().fg(BRAND_GREEN).bold(), "\u{25b6} Running"),
                    "Completed" => (Style::default().fg(ACCENT_BLUE), "\u{2714} Done"),
                    "Failed" => (Style::default().fg(BRAND_RED).bold(), "\u{2718} Failed"),
                    other => (Style::default().fg(BRAND_DIM), other),
                };
                let duration_str = if wf.duration_ms > 0 {
                    format!("{}ms", wf.duration_ms)
                } else {
                    "--".to_string()
                };
                Row::new(vec![
                    Cell::from(id_short).style(Style::default().fg(BRAND_DIM)),
                    Cell::from(wf.pattern.clone()).style(Style::default().fg(Color::White)),
                    Cell::from(steps_str).style(Style::default().fg(BRAND_CYAN)),
                    Cell::from(status_str).style(status_style),
                    Cell::from(duration_str).style(Style::default().fg(BRAND_YELLOW)),
                    Cell::from(wf.started_at.clone()).style(Style::default().fg(BRAND_DIM)),
                ])
            })
            .collect();

        let wf_table = Table::new(
            rows,
            [
                Constraint::Length(14),
                Constraint::Length(10),
                Constraint::Length(8),
                Constraint::Length(12),
                Constraint::Length(10),
                Constraint::Min(0),
            ],
        )
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BRAND_DIM))
                .title(Span::styled(
                    format!(
                        " Workflows ({} active, {} completed) ",
                        state.active_workflows.len(),
                        state.completed_workflows.len()
                    ),
                    Style::default().fg(BRAND_CYAN).bold(),
                ))
                .style(Style::default().bg(SURFACE)),
        )
        .column_spacing(1);

        frame.render_widget(wf_table, chunks[0]);

        // --- Blackboard panel ---
        let bb_lines: Vec<Line> = if state.blackboard_entries.is_empty() {
            vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  No blackboard entries yet.",
                    Style::default().fg(BRAND_DIM),
                )),
            ]
        } else {
            state
                .blackboard_entries
                .iter()
                .map(|(key, preview, ts_ms)| {
                    let age_secs = ts_ms / 1000;
                    Line::from(vec![
                        Span::styled("  ", Style::default()),
                        Span::styled(key, Style::default().fg(Color::White)),
                        Span::styled(" \u{2190} ", Style::default().fg(BRAND_DIM)),
                        Span::styled(preview, Style::default().fg(BRAND_YELLOW)),
                        Span::styled(format!("  ({}s)", age_secs), Style::default().fg(BRAND_DIM)),
                    ])
                })
                .collect()
        };

        let bb_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BRAND_DIM))
            .title(Span::styled(
                format!(" Blackboard ({} entries) ", state.blackboard_entries.len()),
                Style::default().fg(BRAND_CYAN).bold(),
            ))
            .style(Style::default().bg(SURFACE));

        frame.render_widget(
            Paragraph::new(bb_lines)
                .block(bb_block)
                .wrap(Wrap { trim: false }),
            chunks[1],
        );
    }

    // -----------------------------------------------------------------------
    // Logs tab
    // -----------------------------------------------------------------------

    fn render_logs(
        frame: &mut Frame,
        state: &DashboardState,
        area: Rect,
        scroll: usize,
        log_filter: LogFilter,
    ) {
        let filtered: Vec<&String> = state
            .logs
            .iter()
            .filter(|l| log_filter.matches(l))
            .collect();

        let total = filtered.len();
        let visible_height = area.height.saturating_sub(2) as usize;
        let scroll = scroll.min(total.saturating_sub(visible_height));

        let items: Vec<ListItem> = filtered
            .iter()
            .rev()
            .skip(scroll)
            .take(visible_height)
            .map(|l| {
                let (style, prefix) = log_line_style(l);
                ListItem::new(Line::from(vec![
                    Span::styled(prefix, style.add_modifier(Modifier::BOLD)),
                    Span::styled(format!(" {}", l), style),
                ]))
            })
            .collect();

        let filter_hint = if log_filter != LogFilter::All {
            format!(" [filter: {}]", log_filter.label())
        } else {
            String::new()
        };

        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BRAND_DIM))
                .title(Span::styled(
                    format!(" Logs ({}){} ", total, filter_hint),
                    Style::default().fg(BRAND_CYAN).bold(),
                ))
                .style(Style::default().bg(SURFACE)),
        );
        frame.render_widget(list, area);

        // Scrollbar
        if total > visible_height {
            let mut scrollbar_state = ScrollbarState::new(total).position(scroll);
            frame.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .style(Style::default().fg(BRAND_DIM)),
                area,
                &mut scrollbar_state,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

fn elapsed_secs(timestamp: u64) -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    now.saturating_sub(timestamp)
}

fn format_uptime(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{}h{}m{}s", h, m, s)
    } else if m > 0 {
        format!("{}m{}s", m, s)
    } else {
        format!("{}s", s)
    }
}

fn format_elapsed(secs: u64) -> String {
    if secs < 60 {
        format!("{}s ago", secs)
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else {
        format!("{}h ago", secs / 3600)
    }
}

fn peer_id_hex(peer_id: &[u8]) -> String {
    peer_id.iter().map(|b| format!("{:02x}", b)).collect()
}

fn trust_color(trust: f64) -> Color {
    if trust >= 0.7 {
        BRAND_GREEN
    } else if trust >= 0.4 {
        BRAND_YELLOW
    } else {
        BRAND_RED
    }
}

fn task_row<'a>(t: &'a TaskLogEntry, in_flight: bool) -> Row<'a> {
    let (status_style, status_icon) = match t.status.as_str() {
        "Success" => (Style::default().fg(BRAND_GREEN), "\u{2713}"),
        "Error" => (Style::default().fg(BRAND_RED), "\u{2717}"),
        "Timeout" => (Style::default().fg(BRAND_YELLOW), "\u{25f7}"),
        "InFlight" | "Pending" => (
            Style::default()
                .fg(ACCENT_BLUE)
                .add_modifier(Modifier::BOLD),
            "\u{25cb}",
        ),
        _ => (Style::default().fg(BRAND_DIM), "\u{2022}"),
    };

    let id_short = if t.id.len() >= 8 { &t.id[..8] } else { &t.id };

    let id_style = if in_flight {
        Style::default().fg(ACCENT_BLUE).bold()
    } else {
        Style::default().fg(BRAND_DIM)
    };

    Row::new(vec![
        Cell::from(Span::styled(id_short.to_string(), id_style)),
        Cell::from(Span::styled(
            t.capability.clone(),
            Style::default().fg(Color::White),
        )),
        Cell::from(Span::styled(
            format!("{} {}", status_icon, t.status),
            status_style,
        )),
        Cell::from(Span::styled(
            format!("{}ms", t.duration_ms),
            Style::default().fg(BRAND_DIM),
        )),
        Cell::from(Span::styled(t.peer.clone(), Style::default().fg(BRAND_DIM))),
    ])
}

fn log_line_style(line: &str) -> (Style, &'static str) {
    let upper = line.to_ascii_uppercase();
    if upper.contains("ERROR") {
        (Style::default().fg(BRAND_RED), "\u{2717}")
    } else if upper.contains("WARN") {
        (Style::default().fg(BRAND_YELLOW), "\u{26a0}")
    } else if upper.contains("INFO") {
        (Style::default().fg(BRAND_GREEN), "\u{2022}")
    } else if upper.contains("DEBUG") || upper.contains("TRACE") {
        (Style::default().fg(BRAND_DIM), "\u{00b7}")
    } else {
        (Style::default().fg(Color::Rgb(160, 160, 170)), "\u{2022}")
    }
}
