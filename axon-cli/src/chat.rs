use crate::config;
use crate::providers::{
    self, build_provider, CompletionRequest, LlmProvider, ModelInfo, ProviderError, ProviderKind,
};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};
use std::io::stdout;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

// ---------------------------------------------------------------------------
// Theme — no opaque backgrounds, inherit terminal transparency
// ---------------------------------------------------------------------------

const CYAN: Color = Color::Rgb(0, 200, 200);
const GREEN: Color = Color::Rgb(80, 220, 120);
const DIM: Color = Color::Rgb(90, 90, 100);
const FAINT: Color = Color::Rgb(55, 55, 65);
const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Role { User, Assistant, System }

#[derive(Debug, Clone)]
struct ChatMessage {
    role: Role,
    content: String,
    duration_ms: Option<u64>,
    tokens: Option<(u32, u32)>,
}

// ---------------------------------------------------------------------------
// Model picker
// ---------------------------------------------------------------------------

struct ModelPicker {
    models: Vec<ModelInfo>,
    cursor: usize,
    filter: String,
    loading: bool,
    error: Option<String>,
}

impl ModelPicker {
    fn filtered(&self) -> Vec<(usize, &ModelInfo)> {
        if self.filter.is_empty() {
            self.models.iter().enumerate().collect()
        } else {
            let q = self.filter.to_lowercase();
            self.models
                .iter()
                .enumerate()
                .filter(|(_, m)| m.id.to_lowercase().contains(&q) || m.name.to_lowercase().contains(&q))
                .collect()
        }
    }
}

// ---------------------------------------------------------------------------
// Slash commands
// ---------------------------------------------------------------------------

enum SlashResult {
    Info(String),
    SetModel(String),
    SetProvider(ProviderKind),
    Clear,
    Help,
    Quit,
    Unknown(String),
    OpenModelPicker,
}

fn parse_slash(input: &str) -> Option<SlashResult> {
    let input = input.trim();
    if !input.starts_with('/') { return None; }
    let mut parts = input[1..].splitn(2, ' ');
    let cmd = parts.next().unwrap_or("").to_lowercase();
    let arg = parts.next().unwrap_or("").trim().to_string();

    Some(match cmd.as_str() {
        "help" | "h" | "?" => SlashResult::Help,
        "quit" | "exit" | "q" => SlashResult::Quit,
        "clear" | "c" => SlashResult::Clear,
        "model" | "m" | "models" => {
            if arg.is_empty() {
                SlashResult::OpenModelPicker
            } else {
                SlashResult::SetModel(arg)
            }
        }
        "provider" | "p" => {
            if arg.is_empty() {
                SlashResult::Info("Usage: /provider <ollama|openrouter|xai|custom>".into())
            } else {
                match arg.parse::<ProviderKind>() {
                    Ok(k) => SlashResult::SetProvider(k),
                    Err(e) => SlashResult::Info(e),
                }
            }
        }
        "system" => {
            if arg.is_empty() {
                SlashResult::Info("Usage: /system <prompt>".into())
            } else {
                SlashResult::Info(format!("System prompt set: {}", arg))
            }
        }
        _ => SlashResult::Unknown(cmd),
    })
}

// ---------------------------------------------------------------------------
// Chat state
// ---------------------------------------------------------------------------

struct ChatState {
    messages: Vec<ChatMessage>,
    input: String,
    input_cursor: usize,
    scroll: usize,
    auto_scroll: bool,

    provider_kind: ProviderKind,
    model: String,
    endpoint: String,
    api_key: String,
    provider: Box<dyn LlmProvider>,
    system_prompt: String,

    pending: Option<oneshot::Receiver<Result<providers::CompletionResponse, ProviderError>>>,
    pending_start: Option<Instant>,
    spinner_tick: usize,

    total_prompt_tokens: u64,
    total_completion_tokens: u64,

    show_help: bool,
    model_picker: Option<ModelPicker>,

    cmd_history: Vec<String>,
    cmd_history_idx: Option<usize>,
}

impl ChatState {
    fn build_prompt(&self, user_msg: &str) -> String {
        let mut prompt = String::new();
        if !self.system_prompt.is_empty() {
            prompt.push_str(&format!("System: {}\n\n", self.system_prompt));
        }
        let history: Vec<&ChatMessage> = self.messages.iter()
            .filter(|m| matches!(m.role, Role::User | Role::Assistant))
            .collect();
        let recent = if history.len() > 20 { &history[history.len() - 20..] } else { &history };
        for msg in recent {
            let role = match msg.role { Role::User => "User", Role::Assistant => "Assistant", Role::System => "System" };
            prompt.push_str(&format!("{}: {}\n\n", role, msg.content));
        }
        prompt.push_str(&format!("User: {}\n\nAssistant:", user_msg));
        prompt
    }

    fn rebuild_provider(&mut self) -> Result<(), ProviderError> {
        self.provider = build_provider(&self.provider_kind, &self.endpoint, &self.api_key, &self.model)?;
        Ok(())
    }

    fn sys_msg(&mut self, content: String) {
        self.messages.push(ChatMessage { role: Role::System, content, duration_ms: None, tokens: None });
        self.auto_scroll = true;
    }
}

// ---------------------------------------------------------------------------
// Public entry
// ---------------------------------------------------------------------------

pub async fn run_chat() -> anyhow::Result<()> {
    let cfg = config::load_config();
    let kind: ProviderKind = cfg.llm.provider.parse().unwrap_or(ProviderKind::Ollama);
    let api_key = if cfg.llm.api_key.is_empty() { providers::resolve_api_key("", &kind) } else { cfg.llm.api_key.clone() };
    let endpoint = if cfg.llm.endpoint.is_empty() { providers::default_endpoint(&kind).to_string() } else { cfg.llm.endpoint.clone() };
    let model = if cfg.llm.model.is_empty() { providers::default_model(&kind).to_string() } else { cfg.llm.model.clone() };
    let provider = build_provider(&kind, &endpoint, &api_key, &model)?;

    let mut state = ChatState {
        messages: Vec::new(),
        input: String::new(),
        input_cursor: 0,
        scroll: 0,
        auto_scroll: true,
        provider_kind: kind,
        model,
        endpoint,
        api_key,
        provider,
        system_prompt: String::new(),
        pending: None,
        pending_start: None,
        spinner_tick: 0,
        total_prompt_tokens: 0,
        total_completion_tokens: 0,
        show_help: false,
        model_picker: None,
        cmd_history: Vec::new(),
        cmd_history_idx: None,
    };

    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    let result = event_loop(&mut terminal, &mut state).await;
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    result?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Event loop
// ---------------------------------------------------------------------------

async fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    state: &mut ChatState,
) -> anyhow::Result<()> {
    loop {
        // Check LLM response
        if let Some(mut rx) = state.pending.take() {
            match rx.try_recv() {
                Ok(result) => {
                    let elapsed = state.pending_start.map(|s| s.elapsed().as_millis() as u64).unwrap_or(0);
                    state.pending_start = None;
                    match result {
                        Ok(resp) => {
                            let tokens = resp.usage.as_ref().map(|u| (u.prompt_tokens, u.completion_tokens));
                            if let Some((p, c)) = tokens {
                                state.total_prompt_tokens += p as u64;
                                state.total_completion_tokens += c as u64;
                            }
                            state.messages.push(ChatMessage {
                                role: Role::Assistant,
                                content: resp.text.trim().to_string(),
                                duration_ms: Some(elapsed),
                                tokens,
                            });
                        }
                        Err(e) => state.sys_msg(format!("Error: {}", e)),
                    }
                    state.auto_scroll = true;
                }
                Err(oneshot::error::TryRecvError::Empty) => {
                    state.pending = Some(rx);
                    state.spinner_tick = state.spinner_tick.wrapping_add(1);
                }
                Err(oneshot::error::TryRecvError::Closed) => {
                    state.pending_start = None;
                    state.sys_msg("Request cancelled.".into());
                }
            }
        }

        // Check model picker async load
        if let Some(picker) = &state.model_picker {
            if picker.loading {
                // Kick off fetch if not started yet (loading flag set, models empty)
                if picker.models.is_empty() && picker.error.is_none() {
                    let kind = state.provider_kind.clone();
                    let endpoint = state.endpoint.clone();
                    let api_key = state.api_key.clone();
                    match providers::fetch_models(&kind, &endpoint, &api_key).await {
                        Ok(models) => {
                            if let Some(p) = &mut state.model_picker {
                                p.models = models;
                                p.loading = false;
                            }
                        }
                        Err(e) => {
                            if let Some(p) = &mut state.model_picker {
                                p.error = Some(format!("{}", e));
                                p.loading = false;
                            }
                        }
                    }
                }
            }
        }

        terminal.draw(|frame| render(frame, state))?;

        let poll_ms = if state.pending.is_some() { 80 } else { 150 };
        if event::poll(Duration::from_millis(poll_ms))? {
            if let Event::Key(key) = event::read()? {
                if handle_key(key, state).await { break; }
            }
        }
    }
    Ok(())
}

async fn handle_key(key: KeyEvent, state: &mut ChatState) -> bool {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return true;
    }

    // Model picker mode
    if state.model_picker.is_some() {
        return handle_picker_key(key, state);
    }

    // Help overlay
    if state.show_help {
        state.show_help = false;
        return false;
    }

    if key.code == KeyCode::Esc {
        if state.pending.is_some() {
            state.pending = None;
            state.pending_start = None;
            state.sys_msg("Cancelled.".into());
            return false;
        }
        return true;
    }

    if state.pending.is_some() { return false; }

    match key.code {
        KeyCode::Enter => {
            let input = state.input.trim().to_string();
            if input.is_empty() { return false; }
            state.input.clear();
            state.input_cursor = 0;
            state.cmd_history_idx = None;
            if !input.starts_with('/') { state.cmd_history.push(input.clone()); }

            if let Some(result) = parse_slash(&input) {
                return handle_slash(result, state, &input).await;
            }
            send_message(state, &input);
            false
        }
        KeyCode::Char(c) => {
            state.input.insert(state.input_cursor, c);
            state.input_cursor += 1;
            state.cmd_history_idx = None;
            false
        }
        KeyCode::Backspace => {
            if state.input_cursor > 0 { state.input_cursor -= 1; state.input.remove(state.input_cursor); }
            false
        }
        KeyCode::Delete => {
            if state.input_cursor < state.input.len() { state.input.remove(state.input_cursor); }
            false
        }
        KeyCode::Left => { state.input_cursor = state.input_cursor.saturating_sub(1); false }
        KeyCode::Right => { state.input_cursor = (state.input_cursor + 1).min(state.input.len()); false }
        KeyCode::Home => { state.input_cursor = 0; false }
        KeyCode::End => { state.input_cursor = state.input.len(); false }
        KeyCode::Up => {
            if !state.cmd_history.is_empty() {
                let idx = match state.cmd_history_idx {
                    Some(i) => i.saturating_sub(1),
                    None => state.cmd_history.len() - 1,
                };
                state.cmd_history_idx = Some(idx);
                state.input = state.cmd_history[idx].clone();
                state.input_cursor = state.input.len();
            }
            false
        }
        KeyCode::Down => {
            if let Some(idx) = state.cmd_history_idx {
                if idx + 1 < state.cmd_history.len() {
                    state.cmd_history_idx = Some(idx + 1);
                    state.input = state.cmd_history[idx + 1].clone();
                    state.input_cursor = state.input.len();
                } else {
                    state.cmd_history_idx = None;
                    state.input.clear();
                    state.input_cursor = 0;
                }
            }
            false
        }
        KeyCode::PageUp => { state.scroll = state.scroll.saturating_sub(10); state.auto_scroll = false; false }
        KeyCode::PageDown => { state.scroll = state.scroll.saturating_add(10); state.auto_scroll = true; false }
        KeyCode::Tab => {
            // Tab completion for slash commands
            if state.input.starts_with('/') {
                let cmds = ["/help", "/model", "/models", "/provider", "/system", "/clear", "/quit"];
                if let Some(match_) = cmds.iter().find(|c| c.starts_with(&state.input) && **c != state.input) {
                    state.input = match_.to_string();
                    state.input_cursor = state.input.len();
                }
            }
            false
        }
        _ => false,
    }
}

fn handle_picker_key(key: KeyEvent, state: &mut ChatState) -> bool {
    let picker = match &mut state.model_picker {
        Some(p) => p,
        None => return false,
    };

    match key.code {
        KeyCode::Esc => { state.model_picker = None; false }
        KeyCode::Up | KeyCode::Char('k') => {
            picker.cursor = picker.cursor.saturating_sub(1);
            false
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let max = picker.filtered().len().saturating_sub(1);
            picker.cursor = (picker.cursor + 1).min(max);
            false
        }
        KeyCode::Enter => {
            let filtered = picker.filtered();
            if let Some((_, model)) = filtered.get(picker.cursor) {
                let model_id = model.id.clone();
                state.model = model_id.clone();
                state.model_picker = None;
                if let Err(e) = state.rebuild_provider() {
                    state.sys_msg(format!("Failed: {}", e));
                } else {
                    state.sys_msg(format!("Model → {}", model_id));
                }
            } else {
                state.model_picker = None;
            }
            false
        }
        KeyCode::Char(c) => {
            picker.filter.push(c);
            picker.cursor = 0;
            false
        }
        KeyCode::Backspace => {
            picker.filter.pop();
            picker.cursor = 0;
            false
        }
        _ => false,
    }
}

async fn handle_slash(result: SlashResult, state: &mut ChatState, raw: &str) -> bool {
    match result {
        SlashResult::Quit => return true,
        SlashResult::Clear => {
            state.messages.clear();
            state.scroll = 0;
        }
        SlashResult::Help => { state.show_help = true; }
        SlashResult::Info(msg) => state.sys_msg(msg),
        SlashResult::SetModel(m) => {
            state.model = m.clone();
            if let Err(e) = state.rebuild_provider() {
                state.sys_msg(format!("Failed: {}", e));
            } else {
                state.sys_msg(format!("Model → {}", m));
            }
        }
        SlashResult::SetProvider(k) => {
            state.provider_kind = k.clone();
            state.endpoint = providers::default_endpoint(&k).to_string();
            state.model = providers::default_model(&k).to_string();
            if let Err(e) = state.rebuild_provider() {
                state.sys_msg(format!("Failed: {}", e));
            } else {
                state.sys_msg(format!("Provider → {} ({})", state.provider_kind, state.model));
            }
        }
        SlashResult::OpenModelPicker => {
            state.model_picker = Some(ModelPicker {
                models: Vec::new(),
                cursor: 0,
                filter: String::new(),
                loading: true,
                error: None,
            });
        }
        SlashResult::Unknown(cmd) => {
            state.sys_msg(format!("Unknown: /{}  — try /help", cmd));
        }
    }
    if raw.starts_with("/system ") {
        state.system_prompt = raw[8..].trim().to_string();
    }
    false
}

fn send_message(state: &mut ChatState, user_msg: &str) {
    state.messages.push(ChatMessage {
        role: Role::User,
        content: user_msg.to_string(),
        duration_ms: None,
        tokens: None,
    });
    state.auto_scroll = true;

    let prompt = state.build_prompt(user_msg);
    let (tx, rx) = oneshot::channel();
    let provider_kind = state.provider_kind.clone();
    let endpoint = state.endpoint.clone();
    let api_key = state.api_key.clone();
    let model = state.model.clone();

    tokio::spawn(async move {
        let result = match build_provider(&provider_kind, &endpoint, &api_key, &model) {
            Ok(p) => p.complete(CompletionRequest { prompt, max_tokens: None, temperature: None }).await,
            Err(e) => Err(e),
        };
        let _ = tx.send(result);
    });

    state.pending = Some(rx);
    state.pending_start = Some(Instant::now());
    state.spinner_tick = 0;
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn render(frame: &mut Frame, state: &mut ChatState) {
    let area = frame.area();
    // No background fill — terminal shows through

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header
            Constraint::Min(3),   // messages
            Constraint::Length(1), // input
            Constraint::Length(1), // status
        ])
        .split(area);

    render_header(frame, state, chunks[0]);
    render_messages(frame, state, chunks[1]);
    render_input(frame, state, chunks[2]);
    render_status_bar(frame, state, chunks[3]);

    // Overlays
    if state.show_help {
        render_help(frame, area);
    }
    if state.model_picker.is_some() {
        render_model_picker(frame, state, area);
    }
}

fn render_header(frame: &mut Frame, state: &ChatState, area: Rect) {
    let model_short = if state.model.len() > 30 {
        format!("{}…", &state.model[..29])
    } else {
        state.model.clone()
    };

    let total_tokens = state.total_prompt_tokens + state.total_completion_tokens;
    let right = if total_tokens > 0 {
        let t = if total_tokens > 1000 { format!("{:.1}k", total_tokens as f64 / 1000.0) } else { format!("{}", total_tokens) };
        format!("{} {} · {} tok ", state.provider_kind, model_short, t)
    } else {
        format!("{} {} ", state.provider_kind, model_short)
    };

    let left = " ▲ axon ";
    let w = area.width as usize;
    let pad = w.saturating_sub(left.len()).saturating_sub(right.len());

    let line = Line::from(vec![
        Span::styled(left, Style::default().fg(CYAN).bold()),
        Span::styled("─".repeat(pad), Style::default().fg(FAINT)),
        Span::styled(right, Style::default().fg(DIM)),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn render_messages(frame: &mut Frame, state: &mut ChatState, area: Rect) {
    let w = area.width as usize;
    let visible = area.height as usize;
    let mut lines: Vec<Line> = Vec::new();

    for msg in &state.messages {
        match msg.role {
            Role::User => {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled("  you", Style::default().fg(CYAN).bold())));
                for l in msg.content.lines() {
                    for wrapped in wrap(l, w.saturating_sub(6)) {
                        lines.push(Line::from(Span::styled(format!("  {}", wrapped), Style::default().fg(Color::White))));
                    }
                }
            }
            Role::Assistant => {
                lines.push(Line::from(""));
                let mut label = vec![Span::styled("  axon", Style::default().fg(GREEN).bold())];
                if let Some(ms) = msg.duration_ms {
                    let t = if ms >= 1000 { format!("{:.1}s", ms as f64 / 1000.0) } else { format!("{}ms", ms) };
                    label.push(Span::styled(format!("  {}", t), Style::default().fg(FAINT)));
                }
                if let Some((_, c)) = msg.tokens {
                    label.push(Span::styled(format!(" · {}tok", c), Style::default().fg(FAINT)));
                }
                lines.push(Line::from(label));
                for l in msg.content.lines() {
                    for wrapped in wrap(l, w.saturating_sub(6)) {
                        lines.push(Line::from(Span::styled(format!("  {}", wrapped), Style::default().fg(Color::Rgb(200, 200, 210)))));
                    }
                }
            }
            Role::System => {
                lines.push(Line::from(""));
                for l in msg.content.lines() {
                    for wrapped in wrap(l, w.saturating_sub(6)) {
                        lines.push(Line::from(Span::styled(format!("  {}", wrapped), Style::default().fg(DIM).italic())));
                    }
                }
            }
        }
    }

    // Spinner
    if state.pending.is_some() {
        lines.push(Line::from(""));
        let f = (state.spinner_tick / 2) % SPINNER.len();
        let elapsed = state.pending_start.map(|s| s.elapsed().as_secs()).unwrap_or(0);
        let t = if elapsed > 0 { format!("  {}s", elapsed) } else { String::new() };
        lines.push(Line::from(vec![
            Span::styled(format!("  {} ", SPINNER[f]), Style::default().fg(CYAN)),
            Span::styled("thinking", Style::default().fg(DIM).italic()),
            Span::styled(t, Style::default().fg(FAINT)),
        ]));
    }

    lines.push(Line::from(""));

    let total = lines.len();
    if state.auto_scroll {
        state.scroll = total.saturating_sub(visible);
    } else {
        state.scroll = state.scroll.min(total.saturating_sub(visible));
    }

    let display: Vec<Line> = lines.into_iter().skip(state.scroll).take(visible).collect();
    frame.render_widget(Paragraph::new(display), area);

    if total > visible {
        let mut sb = ScrollbarState::new(total).position(state.scroll);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight).style(Style::default().fg(FAINT)),
            area, &mut sb,
        );
    }
}

fn render_input(frame: &mut Frame, state: &ChatState, area: Rect) {
    let waiting = state.pending.is_some();
    let (icon, ic) = if waiting { ("◌ ", FAINT) } else { ("❯ ", CYAN) };
    let text = if waiting {
        "waiting...".to_string()
    } else if state.input.is_empty() {
        "message (/help for commands)".to_string()
    } else {
        state.input.clone()
    };
    let tc = if waiting || state.input.is_empty() { FAINT } else { Color::White };

    let line = Line::from(vec![
        Span::styled(format!(" {}", icon), Style::default().fg(ic)),
        Span::styled(text, Style::default().fg(tc)),
    ]);
    frame.render_widget(Paragraph::new(line), area);

    if !waiting {
        let cx = area.x + 3 + state.input_cursor as u16;
        if cx < area.x + area.width {
            frame.set_cursor_position((cx, area.y));
        }
    }
}

fn render_status_bar(frame: &mut Frame, state: &ChatState, area: Rect) {
    let n = state.messages.iter().filter(|m| matches!(m.role, Role::User)).count();
    let spans = vec![
        Span::styled(" enter", Style::default().fg(DIM)),
        Span::styled(" send  ", Style::default().fg(FAINT)),
        Span::styled("/model", Style::default().fg(DIM)),
        Span::styled(" pick  ", Style::default().fg(FAINT)),
        Span::styled("/help", Style::default().fg(DIM)),
        Span::styled("  ", Style::default().fg(FAINT)),
        Span::styled("esc", Style::default().fg(DIM)),
        Span::styled(" quit  ", Style::default().fg(FAINT)),
        Span::styled(format!("{}msg", n), Style::default().fg(FAINT)),
    ];
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_help(frame: &mut Frame, area: Rect) {
    let w = 56u16.min(area.width.saturating_sub(4));
    let h = 19u16.min(area.height.saturating_sub(2));
    let popup = centered(area, w, h);

    // Clear the popup area
    frame.render_widget(Block::default().style(Style::default().bg(Color::Rgb(15, 15, 20))), popup);

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled("  commands", Style::default().fg(Color::White).bold())),
        Line::from(""),
        help_line("/model, /m", "pick a model (interactive)"),
        help_line("/model <id>", "switch model directly"),
        help_line("/provider <p>", "switch provider"),
        help_line("/system <text>", "set system prompt"),
        help_line("/clear", "clear conversation"),
        help_line("/quit", "exit"),
        Line::from(""),
        Line::from(Span::styled("  keys", Style::default().fg(Color::White).bold())),
        Line::from(""),
        help_line("enter", "send"),
        help_line("up/down", "history"),
        help_line("tab", "autocomplete /command"),
        help_line("esc", "cancel or quit"),
        Line::from(""),
        Line::from(Span::styled("  press any key", Style::default().fg(FAINT).italic())),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(FAINT))
        .style(Style::default().bg(Color::Rgb(15, 15, 20)));
    frame.render_widget(Paragraph::new(lines).block(block), popup);
}

fn render_model_picker(frame: &mut Frame, state: &ChatState, area: Rect) {
    let picker = match &state.model_picker {
        Some(p) => p,
        None => return,
    };

    let w = 70u16.min(area.width.saturating_sub(4));
    let h = (area.height - 4).min(24);
    let popup = centered(area, w, h);

    // Background for popup
    frame.render_widget(Block::default().style(Style::default().bg(Color::Rgb(15, 15, 20))), popup);

    if picker.loading {
        let lines = vec![
            Line::from(""),
            Line::from(Span::styled("  loading models...", Style::default().fg(DIM).italic())),
        ];
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(FAINT))
            .title(Span::styled(" model ", Style::default().fg(CYAN)))
            .style(Style::default().bg(Color::Rgb(15, 15, 20)));
        frame.render_widget(Paragraph::new(lines).block(block), popup);
        return;
    }

    let filtered = picker.filtered();
    let inner_h = popup.height.saturating_sub(4) as usize; // borders + header + filter line

    // Scroll the list so cursor is visible
    let scroll = if picker.cursor >= inner_h {
        picker.cursor - inner_h + 1
    } else {
        0
    };

    let mut lines: Vec<Line> = Vec::new();

    // Filter line
    if !picker.filter.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("  /", Style::default().fg(FAINT)),
            Span::styled(&picker.filter, Style::default().fg(CYAN)),
        ]));
    } else {
        lines.push(Line::from(Span::styled("  type to filter", Style::default().fg(FAINT).italic())));
    }
    lines.push(Line::from(""));

    if let Some(err) = &picker.error {
        lines.push(Line::from(Span::styled(format!("  {}", err), Style::default().fg(Color::Rgb(240, 80, 80)))));
    }

    for (i, (_, model)) in filtered.iter().enumerate().skip(scroll).take(inner_h) {
        let sel = i == picker.cursor;
        let marker = if sel { "▸" } else { " " };
        let is_current = model.id == state.model;

        let mut spans = vec![
            Span::styled(format!(" {} ", marker), Style::default().fg(if sel { CYAN } else { FAINT })),
            Span::styled(
                &model.id,
                if sel { Style::default().fg(CYAN).bold() } else { Style::default().fg(Color::White) },
            ),
        ];

        if is_current {
            spans.push(Span::styled(" ●", Style::default().fg(GREEN)));
        }

        if let Some(ctx) = model.context_length {
            let c = if ctx >= 1_000_000 { format!("{}M", ctx / 1_000_000) } else { format!("{}K", ctx / 1_000) };
            spans.push(Span::styled(format!("  {}", c), Style::default().fg(FAINT)));
        }

        lines.push(Line::from(spans));
    }

    let title = format!(" model ({}) ", filtered.len());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(FAINT))
        .title(Span::styled(title, Style::default().fg(CYAN)))
        .title_bottom(Line::from(Span::styled(" ↑↓ navigate  enter select  esc close ", Style::default().fg(FAINT))))
        .style(Style::default().bg(Color::Rgb(15, 15, 20)));
    frame.render_widget(Paragraph::new(lines).block(block), popup);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn help_line<'a>(cmd: &'a str, desc: &'a str) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("  {:<18}", cmd), Style::default().fg(CYAN)),
        Span::styled(desc, Style::default().fg(DIM)),
    ])
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w.min(area.width), h.min(area.height))
}

fn wrap(text: &str, max: usize) -> Vec<String> {
    if max == 0 || text.len() <= max { return vec![text.to_string()]; }
    let mut out = Vec::new();
    let mut rem = text;
    while rem.len() > max {
        let at = rem[..max].rfind(' ').unwrap_or(max);
        let (l, r) = rem.split_at(at);
        out.push(l.to_string());
        rem = r.trim_start();
    }
    if !rem.is_empty() { out.push(rem.to_string()); }
    if out.is_empty() { out.push(String::new()); }
    out
}
