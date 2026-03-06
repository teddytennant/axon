use axon_core::{Capability, PeerInfo};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, List, ListItem, Paragraph, Row, Table, Tabs, Wrap},
};
use std::collections::VecDeque;
use std::io::stdout;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Mesh,
    Agents,
    Tasks,
    State,
    Logs,
}

impl Tab {
    pub fn all() -> Vec<Tab> {
        vec![Tab::Mesh, Tab::Agents, Tab::Tasks, Tab::State, Tab::Logs]
    }

    pub fn title(&self) -> &str {
        match self {
            Tab::Mesh => "Mesh",
            Tab::Agents => "Agents",
            Tab::Tasks => "Tasks",
            Tab::State => "State",
            Tab::Logs => "Logs",
        }
    }

    pub fn index(&self) -> usize {
        match self {
            Tab::Mesh => 0,
            Tab::Agents => 1,
            Tab::Tasks => 2,
            Tab::State => 3,
            Tab::Logs => 4,
        }
    }

    pub fn from_index(i: usize) -> Self {
        match i {
            0 => Tab::Mesh,
            1 => Tab::Agents,
            2 => Tab::Tasks,
            3 => Tab::State,
            4 => Tab::Logs,
            _ => Tab::Mesh,
        }
    }
}

/// Shared state between the TUI and the mesh node.
pub struct DashboardState {
    pub peer_id: String,
    pub listen_addr: String,
    pub peers: Vec<PeerInfo>,
    pub agent_names: Vec<String>,
    pub capabilities: Vec<Capability>,
    pub task_log: Vec<TaskLogEntry>,
    pub logs: VecDeque<String>,
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
        }
    }

    pub fn add_log(&mut self, msg: String) {
        self.logs.push_back(msg);
        if self.logs.len() > 1000 {
            self.logs.pop_front();
        }
    }
}

pub struct Dashboard {
    state: Arc<RwLock<DashboardState>>,
    active_tab: Tab,
    scroll_offset: usize,
}

impl Dashboard {
    pub fn new(state: Arc<RwLock<DashboardState>>) -> Self {
        Self {
            state,
            active_tab: Tab::Mesh,
            scroll_offset: 0,
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

            terminal.draw(|frame| {
                Self::render(frame, &state, active_tab, scroll);
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
                let idx = (self.active_tab.index() + 1) % 5;
                self.active_tab = Tab::from_index(idx);
                self.scroll_offset = 0;
            }
            KeyCode::BackTab => {
                let idx = (self.active_tab.index() + 4) % 5;
                self.active_tab = Tab::from_index(idx);
                self.scroll_offset = 0;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.scroll_offset = self.scroll_offset.saturating_add(1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
            }
            KeyCode::Char('1') => self.active_tab = Tab::Mesh,
            KeyCode::Char('2') => self.active_tab = Tab::Agents,
            KeyCode::Char('3') => self.active_tab = Tab::Tasks,
            KeyCode::Char('4') => self.active_tab = Tab::State,
            KeyCode::Char('5') => self.active_tab = Tab::Logs,
            _ => {}
        }
        false
    }

    fn render(frame: &mut Frame, state: &DashboardState, active_tab: Tab, scroll: usize) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Header
                Constraint::Length(3), // Tabs
                Constraint::Min(0),   // Content
                Constraint::Length(1), // Status bar
            ])
            .split(frame.area());

        // Header
        let header = Paragraph::new(format!(
            " axon mesh  |  Peer: {}  |  Listen: {}  |  Peers: {}",
            &state.peer_id[..8.min(state.peer_id.len())],
            state.listen_addr,
            state.peers.len(),
        ))
        .block(Block::default().borders(Borders::ALL).title(" Axon "))
        .style(Style::default().fg(Color::Cyan));
        frame.render_widget(header, chunks[0]);

        // Tabs
        let tab_titles: Vec<Line> = Tab::all()
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let style = if *t == active_tab {
                    Style::default().fg(Color::Yellow).bold()
                } else {
                    Style::default().fg(Color::Gray)
                };
                Line::from(format!(" {} {} ", i + 1, t.title())).style(style)
            })
            .collect();
        let tabs = Tabs::new(tab_titles)
            .select(active_tab.index())
            .highlight_style(Style::default().fg(Color::Yellow))
            .block(Block::default().borders(Borders::ALL));
        frame.render_widget(tabs, chunks[1]);

        // Content
        match active_tab {
            Tab::Mesh => Self::render_mesh(frame, state, chunks[2]),
            Tab::Agents => Self::render_agents(frame, state, chunks[2]),
            Tab::Tasks => Self::render_tasks(frame, state, chunks[2], scroll),
            Tab::State => Self::render_state(frame, state, chunks[2]),
            Tab::Logs => Self::render_logs(frame, state, chunks[2], scroll),
        }

        // Status bar
        let status = Paragraph::new(" q: quit | Tab: switch | j/k: scroll | 1-5: jump to tab")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(status, chunks[3]);
    }

    fn render_mesh(frame: &mut Frame, state: &DashboardState, area: Rect) {
        if state.peers.is_empty() {
            let msg = Paragraph::new("No peers discovered yet.\nWaiting for mesh connections...")
                .block(Block::default().borders(Borders::ALL).title(" Mesh Peers "))
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(msg, area);
            return;
        }

        let rows: Vec<Row> = state
            .peers
            .iter()
            .map(|p| {
                let id_short = if p.peer_id.len() >= 4 {
                    p.peer_id.iter().take(4).map(|b| format!("{:02x}", b)).collect::<String>()
                } else {
                    "????".to_string()
                };
                let caps = p
                    .capabilities
                    .iter()
                    .map(|c: &Capability| c.tag())
                    .collect::<Vec<_>>()
                    .join(", ");
                Row::new(vec![
                    Cell::from(id_short).style(Style::default().fg(Color::Green)),
                    Cell::from(p.addr.clone()),
                    Cell::from(caps),
                    Cell::from(format!("{}s ago", elapsed_secs(p.last_seen))),
                ])
            })
            .collect();

        let table = Table::new(
            rows,
            [
                Constraint::Length(10),
                Constraint::Length(22),
                Constraint::Min(20),
                Constraint::Length(12),
            ],
        )
        .header(
            Row::new(vec!["Peer ID", "Address", "Capabilities", "Last Seen"])
                .style(Style::default().fg(Color::Yellow).bold()),
        )
        .block(Block::default().borders(Borders::ALL).title(" Mesh Peers "));

        frame.render_widget(table, area);
    }

    fn render_agents(frame: &mut Frame, state: &DashboardState, area: Rect) {
        let items: Vec<ListItem> = state
            .agent_names
            .iter()
            .map(|name| ListItem::new(format!("  {}", name)))
            .collect();

        let caps_text: Vec<ListItem> = state
            .capabilities
            .iter()
            .map(|c| {
                ListItem::new(format!("  {} ", c.tag()))
                    .style(Style::default().fg(Color::Cyan))
            })
            .collect();

        let inner = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        let agents_list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(" Registered Agents "));
        frame.render_widget(agents_list, inner[0]);

        let caps_list = List::new(caps_text)
            .block(Block::default().borders(Borders::ALL).title(" Capabilities "));
        frame.render_widget(caps_list, inner[1]);
    }

    fn render_tasks(frame: &mut Frame, state: &DashboardState, area: Rect, scroll: usize) {
        if state.task_log.is_empty() {
            let msg = Paragraph::new("No tasks dispatched yet.")
                .block(Block::default().borders(Borders::ALL).title(" Task Log "))
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(msg, area);
            return;
        }

        let rows: Vec<Row> = state
            .task_log
            .iter()
            .rev()
            .skip(scroll)
            .map(|t| {
                let status_style = match t.status.as_str() {
                    "Success" => Style::default().fg(Color::Green),
                    "Error" => Style::default().fg(Color::Red),
                    "Timeout" => Style::default().fg(Color::Yellow),
                    _ => Style::default().fg(Color::Gray),
                };
                Row::new(vec![
                    Cell::from(t.id[..8.min(t.id.len())].to_string()),
                    Cell::from(t.capability.clone()),
                    Cell::from(t.status.clone()).style(status_style),
                    Cell::from(format!("{}ms", t.duration_ms)),
                    Cell::from(t.peer.clone()),
                ])
            })
            .collect();

        let table = Table::new(
            rows,
            [
                Constraint::Length(10),
                Constraint::Min(15),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Length(10),
            ],
        )
        .header(
            Row::new(vec!["Task ID", "Capability", "Status", "Latency", "Peer"])
                .style(Style::default().fg(Color::Yellow).bold()),
        )
        .block(Block::default().borders(Borders::ALL).title(" Task Log "));

        frame.render_widget(table, area);
    }

    fn render_state(frame: &mut Frame, _state: &DashboardState, area: Rect) {
        let msg = Paragraph::new("CRDT shared state viewer\n\nState will appear here as peers sync.")
            .block(Block::default().borders(Borders::ALL).title(" Shared State "))
            .wrap(Wrap { trim: false });
        frame.render_widget(msg, area);
    }

    fn render_logs(frame: &mut Frame, state: &DashboardState, area: Rect, scroll: usize) {
        let items: Vec<ListItem> = state
            .logs
            .iter()
            .rev()
            .skip(scroll)
            .take(area.height as usize)
            .map(|l| ListItem::new(l.as_str()))
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(" Logs "));
        frame.render_widget(list, area);
    }
}

fn elapsed_secs(timestamp: u64) -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    now.saturating_sub(timestamp)
}
