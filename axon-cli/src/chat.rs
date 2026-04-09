use crate::config;
use crate::providers::{
    self, build_provider, CompletionRequest, LlmProvider, ProviderError, ProviderKind,
};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Padding, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};
use std::io::stdout;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

// ---------------------------------------------------------------------------
// Theme
// ---------------------------------------------------------------------------

const CYAN: Color = Color::Rgb(0, 200, 200);
const GREEN: Color = Color::Rgb(80, 220, 120);
const DIM: Color = Color::Rgb(100, 100, 110);
const BG: Color = Color::Rgb(15, 15, 20);
const SURFACE: Color = Color::Rgb(25, 25, 35);
const SEPARATOR: Color = Color::Rgb(40, 40, 50);
const INPUT_BG: Color = Color::Rgb(20, 20, 30);

const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

// ---------------------------------------------------------------------------
// Message types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Role {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone)]
struct ChatMessage {
    role: Role,
    content: String,
    duration_ms: Option<u64>,
    tokens: Option<(u32, u32)>, // (prompt, completion)
}

// ---------------------------------------------------------------------------
// Slash commands
// ---------------------------------------------------------------------------

enum SlashResult {
    /// Show info message in chat
    Info(String),
    /// Switch model
    SetModel(String),
    /// Switch provider + rebuild
    SetProvider(ProviderKind),
    /// Clear conversation
    Clear,
    /// Show help
    Help,
    /// Exit
    Quit,
    /// Unknown command
    Unknown(String),
    /// List models (async)
    ListModels,
}

fn parse_slash(input: &str) -> Option<SlashResult> {
    let input = input.trim();
    if !input.starts_with('/') {
        return None;
    }

    let mut parts = input[1..].splitn(2, ' ');
    let cmd = parts.next().unwrap_or("").to_lowercase();
    let arg = parts.next().unwrap_or("").trim().to_string();

    Some(match cmd.as_str() {
        "help" | "h" | "?" => SlashResult::Help,
        "quit" | "exit" | "q" => SlashResult::Quit,
        "clear" | "c" => SlashResult::Clear,
        "model" | "m" => {
            if arg.is_empty() {
                SlashResult::Info("Usage: /model <model-id>".into())
            } else {
                SlashResult::SetModel(arg)
            }
        }
        "provider" | "p" => {
            if arg.is_empty() {
                SlashResult::Info(
                    "Usage: /provider <ollama|openrouter|xai|custom>".into(),
                )
            } else {
                match arg.parse::<ProviderKind>() {
                    Ok(k) => SlashResult::SetProvider(k),
                    Err(e) => SlashResult::Info(e),
                }
            }
        }
        "models" => SlashResult::ListModels,
        "config" => SlashResult::Info("(use /provider and /model to change config)".into()),
        "system" => {
            if arg.is_empty() {
                SlashResult::Info("Usage: /system <prompt>  — sets a system prompt prepended to all messages".into())
            } else {
                SlashResult::Info(format!("System prompt set to: {}", arg))
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

    // Provider state
    provider_kind: ProviderKind,
    model: String,
    endpoint: String,
    api_key: String,
    provider: Box<dyn LlmProvider>,
    system_prompt: String,

    // Async LLM
    pending: Option<oneshot::Receiver<Result<providers::CompletionResponse, ProviderError>>>,
    pending_start: Option<Instant>,
    spinner_tick: usize,

    // Stats
    total_prompt_tokens: u64,
    total_completion_tokens: u64,
    message_count: u64,

    // Help overlay
    show_help: bool,

    // Command history
    cmd_history: Vec<String>,
    cmd_history_idx: Option<usize>,
}

impl ChatState {
    fn build_prompt(&self, user_msg: &str) -> String {
        let mut prompt = String::new();

        if !self.system_prompt.is_empty() {
            prompt.push_str(&format!("System: {}\n\n", self.system_prompt));
        }

        // Include conversation history (last 20 messages for context)
        let history: Vec<&ChatMessage> = self
            .messages
            .iter()
            .filter(|m| matches!(m.role, Role::User | Role::Assistant))
            .collect();

        let recent = if history.len() > 20 {
            &history[history.len() - 20..]
        } else {
            &history
        };

        if !recent.is_empty() {
            for msg in recent {
                let role = match msg.role {
                    Role::User => "User",
                    Role::Assistant => "Assistant",
                    Role::System => "System",
                };
                prompt.push_str(&format!("{}: {}\n\n", role, msg.content));
            }
        }

        prompt.push_str(&format!("User: {}\n\nAssistant:", user_msg));
        prompt
    }

    fn rebuild_provider(&mut self) -> Result<(), providers::ProviderError> {
        self.provider = build_provider(
            &self.provider_kind,
            &self.endpoint,
            &self.api_key,
            &self.model,
        )?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Public entry
// ---------------------------------------------------------------------------

pub async fn run_chat() -> anyhow::Result<()> {
    let file_config = config::load_config();
    let kind: ProviderKind = file_config
        .llm
        .provider
        .parse()
        .unwrap_or(ProviderKind::Ollama);

    let api_key = if file_config.llm.api_key.is_empty() {
        providers::resolve_api_key("", &kind)
    } else {
        file_config.llm.api_key.clone()
    };
    let endpoint = if file_config.llm.endpoint.is_empty() {
        providers::default_endpoint(&kind).to_string()
    } else {
        file_config.llm.endpoint.clone()
    };
    let model = if file_config.llm.model.is_empty() {
        providers::default_model(&kind).to_string()
    } else {
        file_config.llm.model.clone()
    };

    let provider = build_provider(&kind, &endpoint, &api_key, &model)?;

    let mut state = ChatState {
        messages: vec![ChatMessage {
            role: Role::System,
            content: "Type a message to chat. /help for commands.".into(),
            duration_ms: None,
            tokens: None,
        }],
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
        message_count: 0,
        show_help: false,
        cmd_history: Vec::new(),
        cmd_history_idx: None,
    };

    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    let result = event_loop(&mut terminal, &mut state).await;

    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;

    if let Err(e) = result {
        eprintln!("Chat error: {}", e);
    }
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
        // Check for LLM response
        if let Some(mut rx) = state.pending.take() {
            match rx.try_recv() {
                Ok(result) => {
                    let elapsed = state
                        .pending_start
                        .map(|s| s.elapsed().as_millis() as u64)
                        .unwrap_or(0);
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
                        Err(e) => {
                            state.messages.push(ChatMessage {
                                role: Role::System,
                                content: format!("Error: {}", e),
                                duration_ms: Some(elapsed),
                                tokens: None,
                            });
                        }
                    }
                    state.auto_scroll = true;
                }
                Err(oneshot::error::TryRecvError::Empty) => {
                    // Still pending
                    state.pending = Some(rx);
                    state.spinner_tick = state.spinner_tick.wrapping_add(1);
                }
                Err(oneshot::error::TryRecvError::Closed) => {
                    state.pending_start = None;
                    state.messages.push(ChatMessage {
                        role: Role::System,
                        content: "Request cancelled.".into(),
                        duration_ms: None,
                        tokens: None,
                    });
                }
            }
        }

        // Render
        terminal.draw(|frame| render(frame, state))?;

        // Poll input (fast tick for spinner)
        let poll_ms = if state.pending.is_some() { 80 } else { 200 };
        if event::poll(Duration::from_millis(poll_ms))? {
            if let Event::Key(key) = event::read()? {
                if handle_key(key, state).await {
                    break;
                }
            }
        }
    }
    Ok(())
}

/// Returns true if should exit.
async fn handle_key(key: KeyEvent, state: &mut ChatState) -> bool {
    // Help overlay dismissal
    if state.show_help {
        state.show_help = false;
        return false;
    }

    // Ctrl-C always quits
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return true;
    }

    // Esc: if pending, cancel; otherwise quit
    if key.code == KeyCode::Esc {
        if state.pending.is_some() {
            state.pending = None;
            state.pending_start = None;
            state.messages.push(ChatMessage {
                role: Role::System,
                content: "Cancelled.".into(),
                duration_ms: None,
                tokens: None,
            });
            return false;
        }
        return true;
    }

    // Don't accept input while waiting for response (except cancel)
    if state.pending.is_some() {
        return false;
    }

    match key.code {
        KeyCode::Enter => {
            let input = state.input.trim().to_string();
            if input.is_empty() {
                return false;
            }

            state.input.clear();
            state.input_cursor = 0;
            state.cmd_history_idx = None;

            // Save to history
            if !input.starts_with('/') || input.starts_with("/system") {
                state.cmd_history.push(input.clone());
            }

            // Check for slash command
            if let Some(result) = parse_slash(&input) {
                return handle_slash(result, state, &input).await;
            }

            // Regular message — send to LLM
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
            if state.input_cursor > 0 {
                state.input_cursor -= 1;
                state.input.remove(state.input_cursor);
            }
            false
        }
        KeyCode::Delete => {
            if state.input_cursor < state.input.len() {
                state.input.remove(state.input_cursor);
            }
            false
        }
        KeyCode::Left => {
            state.input_cursor = state.input_cursor.saturating_sub(1);
            false
        }
        KeyCode::Right => {
            state.input_cursor = (state.input_cursor + 1).min(state.input.len());
            false
        }
        KeyCode::Home => {
            state.input_cursor = 0;
            false
        }
        KeyCode::End => {
            state.input_cursor = state.input.len();
            false
        }
        KeyCode::Up => {
            // Command history navigation
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
                    let new_idx = idx + 1;
                    state.cmd_history_idx = Some(new_idx);
                    state.input = state.cmd_history[new_idx].clone();
                    state.input_cursor = state.input.len();
                } else {
                    state.cmd_history_idx = None;
                    state.input.clear();
                    state.input_cursor = 0;
                }
            }
            false
        }
        KeyCode::PageUp => {
            state.scroll = state.scroll.saturating_sub(10);
            state.auto_scroll = false;
            false
        }
        KeyCode::PageDown => {
            state.scroll = state.scroll.saturating_add(10);
            state.auto_scroll = true;
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
            state.messages.push(ChatMessage {
                role: Role::System,
                content: "Conversation cleared.".into(),
                duration_ms: None,
                tokens: None,
            });
            state.scroll = 0;
        }
        SlashResult::Help => {
            state.show_help = true;
        }
        SlashResult::Info(msg) => {
            state.messages.push(ChatMessage {
                role: Role::System,
                content: msg,
                duration_ms: None,
                tokens: None,
            });
            state.auto_scroll = true;
        }
        SlashResult::SetModel(m) => {
            state.model = m.clone();
            if let Err(e) = state.rebuild_provider() {
                state.messages.push(ChatMessage {
                    role: Role::System,
                    content: format!("Failed to switch model: {}", e),
                    duration_ms: None,
                    tokens: None,
                });
            } else {
                state.messages.push(ChatMessage {
                    role: Role::System,
                    content: format!("Model switched to {}", m),
                    duration_ms: None,
                    tokens: None,
                });
            }
            state.auto_scroll = true;
        }
        SlashResult::SetProvider(k) => {
            let old = state.provider_kind.to_string();
            state.provider_kind = k.clone();
            state.endpoint = providers::default_endpoint(&k).to_string();
            state.model = providers::default_model(&k).to_string();
            // Keep api_key — user may have set it
            if let Err(e) = state.rebuild_provider() {
                state.messages.push(ChatMessage {
                    role: Role::System,
                    content: format!("Failed to switch provider: {}", e),
                    duration_ms: None,
                    tokens: None,
                });
            } else {
                state.messages.push(ChatMessage {
                    role: Role::System,
                    content: format!(
                        "Switched from {} to {} (model: {})",
                        old, state.provider_kind, state.model
                    ),
                    duration_ms: None,
                    tokens: None,
                });
            }
            state.auto_scroll = true;
        }
        SlashResult::ListModels => {
            state.messages.push(ChatMessage {
                role: Role::System,
                content: "Fetching models...".into(),
                duration_ms: None,
                tokens: None,
            });
            state.auto_scroll = true;
            // Fetch models
            match providers::fetch_models(&state.provider_kind, &state.endpoint, &state.api_key)
                .await
            {
                Ok(models) => {
                    let list: String = models
                        .iter()
                        .take(30)
                        .map(|m| format!("  {} — {}", m.id, m.name))
                        .collect::<Vec<_>>()
                        .join("\n");
                    let total = models.len();
                    // Replace the "Fetching..." message
                    if let Some(last) = state.messages.last_mut() {
                        last.content = format!(
                            "Available models ({}):\n{}\n{}",
                            total,
                            list,
                            if total > 30 {
                                format!("  ... and {} more. Use /model <id> to switch.", total - 30)
                            } else {
                                "  Use /model <id> to switch.".into()
                            }
                        );
                    }
                }
                Err(e) => {
                    if let Some(last) = state.messages.last_mut() {
                        last.content = format!("Failed to fetch models: {}", e);
                    }
                }
            }
        }
        SlashResult::Unknown(cmd) => {
            state.messages.push(ChatMessage {
                role: Role::System,
                content: format!("Unknown command: /{}. Type /help for available commands.", cmd),
                duration_ms: None,
                tokens: None,
            });
            state.auto_scroll = true;
        }
    }

    // Handle /system specially (set the system prompt)
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
    state.message_count += 1;
    state.auto_scroll = true;

    // Build prompt with context
    let prompt = state.build_prompt(user_msg);

    // Spawn async LLM call
    let (tx, rx) = oneshot::channel();

    // We need to clone what we need for the spawned task
    let provider_kind = state.provider_kind.clone();
    let endpoint = state.endpoint.clone();
    let api_key = state.api_key.clone();
    let model = state.model.clone();

    tokio::spawn(async move {
        // Build a fresh provider for the async task
        let result = match build_provider(&provider_kind, &endpoint, &api_key, &model) {
            Ok(p) => {
                p.complete(CompletionRequest {
                    prompt,
                    max_tokens: None,
                    temperature: None,
                })
                .await
            }
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
    frame.render_widget(Block::default().style(Style::default().bg(BG)), area);

    if state.show_help {
        render_help_overlay(frame, area);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // header
            Constraint::Min(4),   // messages
            Constraint::Length(3), // input
            Constraint::Length(1), // status bar
        ])
        .split(area);

    render_header(frame, state, chunks[0]);
    render_messages(frame, state, chunks[1]);
    render_input(frame, state, chunks[2]);
    render_status_bar(frame, state, chunks[3]);
}

fn render_header(frame: &mut Frame, state: &ChatState, area: Rect) {
    let model_short = if state.model.len() > 35 {
        format!("{}…", &state.model[..34])
    } else {
        state.model.clone()
    };

    let total_tokens = state.total_prompt_tokens + state.total_completion_tokens;
    let token_display = if total_tokens > 1000 {
        format!("{:.1}K tokens", total_tokens as f64 / 1000.0)
    } else if total_tokens > 0 {
        format!("{} tokens", total_tokens)
    } else {
        String::new()
    };

    let right_side = if token_display.is_empty() {
        format!("{} · {}", state.provider_kind, model_short)
    } else {
        format!(
            "{} · {}  ·  {}",
            state.provider_kind, model_short, token_display
        )
    };

    // Calculate padding
    let left = " ▲ AXON";
    let total_width = area.width as usize;
    let padding = total_width
        .saturating_sub(left.len())
        .saturating_sub(right_side.len())
        .saturating_sub(2);

    let header = Line::from(vec![
        Span::styled(left, Style::default().fg(CYAN).bold()),
        Span::styled(" ".repeat(padding), Style::default()),
        Span::styled(right_side, Style::default().fg(DIM)),
        Span::styled(" ", Style::default()),
    ]);

    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(SEPARATOR))
        .style(Style::default().bg(SURFACE));

    frame.render_widget(Paragraph::new(header).block(block), area);
}

fn render_messages(frame: &mut Frame, state: &mut ChatState, area: Rect) {
    let inner_width = area.width.saturating_sub(2) as usize; // account for borders
    let visible_height = area.height.saturating_sub(2) as usize;

    // Build all lines for all messages
    let mut all_lines: Vec<Line> = Vec::new();

    for msg in &state.messages {
        match msg.role {
            Role::User => {
                all_lines.push(Line::from(""));
                // Role label with optional metadata
                all_lines.push(Line::from(Span::styled(
                    "  ● You",
                    Style::default().fg(CYAN).bold(),
                )));

                // Message content — wrap manually
                for line in msg.content.lines() {
                    for wrapped in wrap_text(line, inner_width.saturating_sub(4)) {
                        all_lines.push(Line::from(Span::styled(
                            format!("    {}", wrapped),
                            Style::default().fg(Color::White),
                        )));
                    }
                }
            }
            Role::Assistant => {
                all_lines.push(Line::from(""));

                // Role label with timing
                let mut label_spans = vec![Span::styled(
                    "  ◆ Axon",
                    Style::default().fg(GREEN).bold(),
                )];
                if let Some(ms) = msg.duration_ms {
                    let time_str = if ms >= 1000 {
                        format!(" {:.1}s", ms as f64 / 1000.0)
                    } else {
                        format!(" {}ms", ms)
                    };
                    label_spans.push(Span::styled(
                        time_str,
                        Style::default().fg(DIM),
                    ));
                }
                if let Some((_, c)) = msg.tokens {
                    label_spans.push(Span::styled(
                        format!(" · {} tok", c),
                        Style::default().fg(DIM),
                    ));
                }
                all_lines.push(Line::from(label_spans));

                // Message content
                for line in msg.content.lines() {
                    for wrapped in wrap_text(line, inner_width.saturating_sub(4)) {
                        all_lines.push(Line::from(Span::styled(
                            format!("    {}", wrapped),
                            Style::default().fg(Color::Rgb(210, 210, 220)),
                        )));
                    }
                }
            }
            Role::System => {
                all_lines.push(Line::from(""));

                // Multi-line system messages (e.g., model list)
                for (i, line) in msg.content.lines().enumerate() {
                    for wrapped in wrap_text(line, inner_width.saturating_sub(4)) {
                        if i == 0 && !msg.content.starts_with("Available") {
                            all_lines.push(Line::from(Span::styled(
                                format!("  ─ {}", wrapped),
                                Style::default().fg(DIM).italic(),
                            )));
                        } else {
                            all_lines.push(Line::from(Span::styled(
                                format!("    {}", wrapped),
                                Style::default().fg(DIM),
                            )));
                        }
                    }
                }
            }
        }
    }

    // Thinking indicator
    if state.pending.is_some() {
        all_lines.push(Line::from(""));
        let frame_idx = (state.spinner_tick / 2) % SPINNER.len();
        let elapsed = state
            .pending_start
            .map(|s| s.elapsed().as_secs())
            .unwrap_or(0);
        let elapsed_str = if elapsed > 0 {
            format!(" {}s", elapsed)
        } else {
            String::new()
        };
        all_lines.push(Line::from(vec![
            Span::styled(
                format!("  {} ", SPINNER[frame_idx]),
                Style::default().fg(CYAN),
            ),
            Span::styled("Thinking...", Style::default().fg(DIM).italic()),
            Span::styled(elapsed_str, Style::default().fg(DIM)),
        ]));
    }

    all_lines.push(Line::from("")); // bottom padding

    // Auto-scroll: pin to bottom
    let total_lines = all_lines.len();
    if state.auto_scroll {
        state.scroll = total_lines.saturating_sub(visible_height);
    } else {
        state.scroll = state.scroll.min(total_lines.saturating_sub(visible_height));
    }

    let display_lines: Vec<Line> = all_lines
        .into_iter()
        .skip(state.scroll)
        .take(visible_height)
        .collect();

    let block = Block::default()
        .borders(Borders::NONE)
        .style(Style::default().bg(BG));

    frame.render_widget(Paragraph::new(display_lines).block(block), area);

    // Scrollbar
    if total_lines > visible_height {
        let mut scrollbar_state = ScrollbarState::new(total_lines).position(state.scroll);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .style(Style::default().fg(Color::Rgb(50, 50, 60))),
            area,
            &mut scrollbar_state,
        );
    }
}

fn render_input(frame: &mut Frame, state: &ChatState, area: Rect) {
    let is_waiting = state.pending.is_some();

    let (prompt_icon, prompt_color) = if is_waiting {
        ("  ◌ ", DIM)
    } else {
        ("  ❯ ", CYAN)
    };

    let display_text = if is_waiting {
        "waiting for response...".to_string()
    } else if state.input.is_empty() {
        "Type a message...".to_string()
    } else {
        state.input.clone()
    };

    let text_color = if is_waiting || state.input.is_empty() {
        DIM
    } else {
        Color::White
    };

    let input_line = Line::from(vec![
        Span::styled(prompt_icon, Style::default().fg(prompt_color).bold()),
        Span::styled(display_text, Style::default().fg(text_color)),
        if !is_waiting && !state.input.is_empty() || state.input.is_empty() {
            Span::styled("", Style::default())
        } else {
            Span::styled("", Style::default())
        },
    ]);

    let block = Block::default()
        .borders(Borders::TOP | Borders::BOTTOM)
        .border_style(Style::default().fg(SEPARATOR))
        .style(Style::default().bg(INPUT_BG));

    frame.render_widget(Paragraph::new(input_line).block(block), area);

    // Show cursor
    if !is_waiting {
        let cursor_x = area.x + 4 + state.input_cursor as u16;
        let cursor_y = area.y + 1;
        if cursor_x < area.x + area.width - 1 {
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }
}

fn render_status_bar(frame: &mut Frame, state: &ChatState, area: Rect) {
    let msg_count = state
        .messages
        .iter()
        .filter(|m| matches!(m.role, Role::User))
        .count();

    let spans = vec![
        Span::styled(" Enter", Style::default().fg(CYAN).bold()),
        Span::styled(" send", Style::default().fg(DIM)),
        Span::styled(" │ ", Style::default().fg(SEPARATOR)),
        Span::styled("/help", Style::default().fg(CYAN).bold()),
        Span::styled(" commands", Style::default().fg(DIM)),
        Span::styled(" │ ", Style::default().fg(SEPARATOR)),
        Span::styled("/model", Style::default().fg(CYAN).bold()),
        Span::styled(" switch", Style::default().fg(DIM)),
        Span::styled(" │ ", Style::default().fg(SEPARATOR)),
        Span::styled("/clear", Style::default().fg(CYAN).bold()),
        Span::styled(" reset", Style::default().fg(DIM)),
        Span::styled(" │ ", Style::default().fg(SEPARATOR)),
        Span::styled("Esc", Style::default().fg(CYAN).bold()),
        Span::styled(" quit", Style::default().fg(DIM)),
        Span::styled(" │ ", Style::default().fg(SEPARATOR)),
        Span::styled(
            format!("{} msgs", msg_count),
            Style::default().fg(DIM),
        ),
    ];

    let bar = Paragraph::new(Line::from(spans)).style(Style::default().bg(SURFACE).fg(DIM));
    frame.render_widget(bar, area);
}

fn render_help_overlay(frame: &mut Frame, area: Rect) {
    // Dim background
    frame.render_widget(
        Block::default().style(Style::default().bg(Color::Rgb(10, 10, 15))),
        area,
    );

    let width = 60u16.min(area.width.saturating_sub(4));
    let height = 22u16.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let popup = Rect::new(x, y, width, height);

    let commands = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  Slash Commands",
            Style::default().fg(Color::White).bold(),
        )),
        Line::from(""),
        cmd_line("/help, /h, /?", "Show this help"),
        cmd_line("/model <id>", "Switch to a different model"),
        cmd_line("/models", "List available models"),
        cmd_line("/provider <name>", "Switch provider (ollama/openrouter/xai/custom)"),
        cmd_line("/system <prompt>", "Set system prompt"),
        cmd_line("/clear, /c", "Clear conversation"),
        cmd_line("/quit, /exit, /q", "Exit chat"),
        Line::from(""),
        Line::from(Span::styled(
            "  Keyboard",
            Style::default().fg(Color::White).bold(),
        )),
        Line::from(""),
        cmd_line("Enter", "Send message"),
        cmd_line("Up / Down", "Navigate command history"),
        cmd_line("PageUp / PageDown", "Scroll conversation"),
        cmd_line("Esc", "Cancel request or quit"),
        cmd_line("Ctrl-C", "Force quit"),
        Line::from(""),
        Line::from(Span::styled(
            "  Press any key to close",
            Style::default().fg(DIM).italic(),
        )),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CYAN))
        .title(Span::styled(
            " ▲ Help ",
            Style::default().fg(CYAN).bold(),
        ))
        .style(Style::default().bg(SURFACE))
        .padding(Padding::horizontal(1));

    frame.render_widget(Paragraph::new(commands).block(block), popup);
}

fn cmd_line<'a>(cmd: &'a str, desc: &'a str) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("    {:<22}", cmd), Style::default().fg(CYAN)),
        Span::styled(desc, Style::default().fg(DIM)),
    ])
}

// ---------------------------------------------------------------------------
// Text wrapping
// ---------------------------------------------------------------------------

fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }
    if text.len() <= max_width {
        return vec![text.to_string()];
    }

    let mut lines = Vec::new();
    let mut remaining = text;

    while remaining.len() > max_width {
        // Find a good break point (space near the limit)
        let break_at = remaining[..max_width]
            .rfind(' ')
            .unwrap_or(max_width);

        let (line, rest) = remaining.split_at(break_at);
        lines.push(line.to_string());
        remaining = rest.trim_start();
    }

    if !remaining.is_empty() {
        lines.push(remaining.to_string());
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}
