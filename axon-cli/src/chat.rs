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
// Theme
// ---------------------------------------------------------------------------

const ACCENT: Color = Color::Rgb(110, 110, 118);
const DIM: Color = Color::Rgb(70, 70, 78);
const FAINT: Color = Color::Rgb(45, 45, 50);
const TEXT: Color = Color::Rgb(170, 170, 178);
const TEXT_DIM: Color = Color::Rgb(145, 145, 152);
const LABEL: Color = Color::Rgb(130, 130, 138);
const POPUP_BG: Color = Color::Rgb(18, 18, 22);
const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

// ---------------------------------------------------------------------------
// Command registry — single source of truth
// ---------------------------------------------------------------------------

struct CmdDef {
    name: &'static str,
    aliases: &'static [&'static str],
    args: &'static str,
    desc: &'static str,
    category: &'static str,
}

const COMMANDS: &[CmdDef] = &[
    // Agent orchestration
    CmdDef {
        name: "run",
        aliases: &["r"],
        args: "<cap> [data]",
        desc: "run an agent task, show result",
        category: "agents",
    },
    CmdDef {
        name: "spawn",
        aliases: &["s"],
        args: "<cap> [data]",
        desc: "launch background agent task",
        category: "agents",
    },
    CmdDef {
        name: "jobs",
        aliases: &["j"],
        args: "",
        desc: "list running and completed jobs",
        category: "agents",
    },
    CmdDef {
        name: "kill",
        aliases: &[],
        args: "<id>",
        desc: "cancel a background job",
        category: "agents",
    },
    CmdDef {
        name: "agents",
        aliases: &["a"],
        args: "",
        desc: "list available agents",
        category: "agents",
    },
    CmdDef {
        name: "auto",
        aliases: &[],
        args: "",
        desc: "toggle auto-agent (LLM launches agents)",
        category: "agents",
    },
    CmdDef {
        name: "task",
        aliases: &["t"],
        args: "<cap> [data]",
        desc: "alias for /run",
        category: "agents",
    },
    // Mesh
    CmdDef {
        name: "peers",
        aliases: &[],
        args: "",
        desc: "show connected mesh peers",
        category: "mesh",
    },
    CmdDef {
        name: "tools",
        aliases: &[],
        args: "",
        desc: "list available MCP tools",
        category: "mesh",
    },
    CmdDef {
        name: "status",
        aliases: &[],
        args: "",
        desc: "show node and mesh status",
        category: "mesh",
    },
    CmdDef {
        name: "trust",
        aliases: &[],
        args: "[peer]",
        desc: "show peer trust scores",
        category: "mesh",
    },
    CmdDef {
        name: "route",
        aliases: &[],
        args: "<peer> <msg>",
        desc: "send prompt to a specific peer's LLM",
        category: "mesh",
    },
    // Model / Provider
    CmdDef {
        name: "model",
        aliases: &["m"],
        args: "[id]",
        desc: "pick or switch model",
        category: "llm",
    },
    CmdDef {
        name: "provider",
        aliases: &["p"],
        args: "<name>",
        desc: "switch provider",
        category: "llm",
    },
    CmdDef {
        name: "system",
        aliases: &[],
        args: "<prompt>",
        desc: "set system prompt",
        category: "llm",
    },
    // Session
    CmdDef {
        name: "clear",
        aliases: &["c"],
        args: "",
        desc: "clear conversation",
        category: "session",
    },
    CmdDef {
        name: "help",
        aliases: &["h", "?"],
        args: "",
        desc: "show help",
        category: "session",
    },
    CmdDef {
        name: "quit",
        aliases: &["q", "exit"],
        args: "",
        desc: "exit",
        category: "session",
    },
];

fn match_command(input: &str) -> Option<(&'static CmdDef, String)> {
    let input = input.trim();
    if !input.starts_with('/') {
        return None;
    }
    let mut parts = input[1..].splitn(2, ' ');
    let cmd = parts.next().unwrap_or("").to_lowercase();
    let arg = parts.next().unwrap_or("").trim().to_string();
    COMMANDS
        .iter()
        .find(|c| c.name == cmd || c.aliases.contains(&cmd.as_str()))
        .map(|c| (c, arg))
}

fn filtered_suggestions(input: &str) -> Vec<&'static CmdDef> {
    if !input.starts_with('/') {
        return Vec::new();
    }
    let partial = &input[1..].to_lowercase();
    if partial.is_empty() {
        return COMMANDS.iter().collect();
    }
    if partial.contains(' ') {
        return Vec::new();
    }
    COMMANDS
        .iter()
        .filter(|c| c.name.starts_with(partial) || c.aliases.iter().any(|a| a.starts_with(partial)))
        .collect()
}

// ---------------------------------------------------------------------------
// Messages
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
                .filter(|(_, m)| {
                    m.id.to_lowercase().contains(&q) || m.name.to_lowercase().contains(&q)
                })
                .collect()
        }
    }
}

// ---------------------------------------------------------------------------
// Background jobs
// ---------------------------------------------------------------------------

struct BackgroundJob {
    id: usize,
    capability: String,
    payload: String,
    started: Instant,
    rx: Option<oneshot::Receiver<Result<String, String>>>,
    result: Option<Result<String, String>>,
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

    // Autocomplete
    ac_cursor: usize,

    // Background jobs
    jobs: Vec<BackgroundJob>,
    next_job_id: usize,
    auto_agent: bool,

    cmd_history: Vec<String>,
    cmd_history_idx: Option<usize>,
}

impl ChatState {
    fn build_prompt(&self, user_msg: &str) -> String {
        let mut prompt = String::new();

        if self.auto_agent {
            prompt.push_str(
                "System: You are an AI orchestrator in the Axon mesh. You have access to agents you can launch.\n\
                 Available agents:\n\
                 - echo.ping <message> — echoes back the message\n\
                 - system.info — returns hostname, OS, and architecture\n\
                 - llm.chat <prompt> — sends a prompt to another LLM instance\n\n\
                 To launch an agent, include [[run:<capability>:<payload>]] in your response.\n\
                 Examples:\n\
                 - [[run:system.info:]] to get system information\n\
                 - [[run:echo.ping:hello world]] to echo a message\n\
                 - [[run:llm.chat:summarize quantum computing in one paragraph]] to delegate to another LLM\n\n\
                 You can launch multiple agents in one response. Results will be shown to the user.\n\
                 Only launch agents when it would genuinely help answer the question.\n\n"
            );
        }

        if !self.system_prompt.is_empty() {
            prompt.push_str(&format!("System: {}\n\n", self.system_prompt));
        }
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
        for msg in recent {
            let role = match msg.role {
                Role::User => "User",
                Role::Assistant => "Assistant",
                Role::System => "System",
            };
            prompt.push_str(&format!("{}: {}\n\n", role, msg.content));
        }
        prompt.push_str(&format!("User: {}\n\nAssistant:", user_msg));
        prompt
    }

    fn rebuild_provider(&mut self) -> Result<(), ProviderError> {
        self.provider = build_provider(
            &self.provider_kind,
            &self.endpoint,
            &self.api_key,
            &self.model,
        )?;
        Ok(())
    }

    fn sys_msg(&mut self, content: String) {
        self.messages.push(ChatMessage {
            role: Role::System,
            content,
            duration_ms: None,
            tokens: None,
        });
        self.auto_scroll = true;
    }

    fn suggestions(&self) -> Vec<&'static CmdDef> {
        filtered_suggestions(&self.input)
    }

    fn has_suggestions(&self) -> bool {
        !self.suggestions().is_empty()
            && self.input.starts_with('/')
            && !self.input[1..].contains(' ')
    }
}

// ---------------------------------------------------------------------------
// Public entry
// ---------------------------------------------------------------------------

pub async fn run_chat() -> anyhow::Result<()> {
    let cfg = config::load_config();
    let kind: ProviderKind = cfg.llm.provider.parse().unwrap_or(ProviderKind::Ollama);
    let api_key = if cfg.llm.api_key.is_empty() {
        providers::resolve_api_key("", &kind)
    } else {
        cfg.llm.api_key.clone()
    };
    let endpoint = if cfg.llm.endpoint.is_empty() {
        providers::default_endpoint(&kind).to_string()
    } else {
        cfg.llm.endpoint.clone()
    };
    let model = if cfg.llm.model.is_empty() {
        providers::default_model(&kind).to_string()
    } else {
        cfg.llm.model.clone()
    };
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
        ac_cursor: 0,
        jobs: Vec::new(),
        next_job_id: 1,
        auto_agent: false,
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
                    let elapsed = state
                        .pending_start
                        .map(|s| s.elapsed().as_millis() as u64)
                        .unwrap_or(0);
                    state.pending_start = None;
                    match result {
                        Ok(resp) => {
                            let tokens = resp
                                .usage
                                .as_ref()
                                .map(|u| (u.prompt_tokens, u.completion_tokens));
                            if let Some((p, c)) = tokens {
                                state.total_prompt_tokens += p as u64;
                                state.total_completion_tokens += c as u64;
                            }
                            let text = resp.text.trim().to_string();

                            // Auto-agent: parse [[run:cap:payload]] and spawn background jobs
                            if state.auto_agent {
                                let calls = extract_agent_calls(&text);
                                for (cap, payload) in &calls {
                                    spawn_job(state, cap, payload);
                                }
                            }

                            state.messages.push(ChatMessage {
                                role: Role::Assistant,
                                content: text,
                                duration_ms: Some(elapsed),
                                tokens,
                            });
                        }
                        Err(e) => state.sys_msg(format!("error: {}", e)),
                    }
                    state.auto_scroll = true;
                }
                Err(oneshot::error::TryRecvError::Empty) => {
                    state.pending = Some(rx);
                    state.spinner_tick = state.spinner_tick.wrapping_add(1);
                }
                Err(oneshot::error::TryRecvError::Closed) => {
                    state.pending_start = None;
                    state.sys_msg("cancelled".into());
                }
            }
        }

        // Model picker async load
        if let Some(picker) = &state.model_picker {
            if picker.loading && picker.models.is_empty() && picker.error.is_none() {
                let kind = state.provider_kind.clone();
                let ep = state.endpoint.clone();
                let key = state.api_key.clone();
                match providers::fetch_models(&kind, &ep, &key).await {
                    Ok(m) => {
                        if let Some(p) = &mut state.model_picker {
                            p.models = m;
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

        // Poll background jobs
        poll_jobs(state);

        terminal.draw(|frame| render(frame, state))?;

        let has_active = state.pending.is_some() || state.jobs.iter().any(|j| j.rx.is_some());
        let poll_ms = if has_active { 80 } else { 150 };
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

async fn handle_key(key: KeyEvent, state: &mut ChatState) -> bool {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return true;
    }

    // Model picker takes priority
    if state.model_picker.is_some() {
        return handle_picker_key(key, state);
    }
    // Help overlay
    if state.show_help {
        state.show_help = false;
        return false;
    }

    if key.code == KeyCode::Esc {
        if state.has_suggestions() {
            // Dismiss autocomplete by clearing input
            state.input.clear();
            state.input_cursor = 0;
            return false;
        }
        if state.pending.is_some() {
            state.pending = None;
            state.pending_start = None;
            state.sys_msg("cancelled".into());
            return false;
        }
        return true;
    }

    if state.pending.is_some() {
        return false;
    }

    // Autocomplete interactions
    if state.has_suggestions() {
        match key.code {
            KeyCode::Tab | KeyCode::Right => {
                // Accept highlighted suggestion
                let sug = state.suggestions();
                let idx = state.ac_cursor.min(sug.len().saturating_sub(1));
                if let Some(cmd) = sug.get(idx) {
                    if cmd.args.is_empty() {
                        state.input = format!("/{}", cmd.name);
                    } else {
                        state.input = format!("/{} ", cmd.name);
                    }
                    state.input_cursor = state.input.len();
                    state.ac_cursor = 0;
                }
                return false;
            }
            KeyCode::Down => {
                let max = state.suggestions().len().saturating_sub(1);
                state.ac_cursor = (state.ac_cursor + 1).min(max);
                return false;
            }
            KeyCode::Up => {
                state.ac_cursor = state.ac_cursor.saturating_sub(1);
                return false;
            }
            KeyCode::Enter => {
                // Accept suggestion if cursor is on one, then execute if no args needed
                let sug = state.suggestions();
                let idx = state.ac_cursor.min(sug.len().saturating_sub(1));
                if let Some(cmd) = sug.get(idx) {
                    if cmd.args.is_empty() {
                        // Execute directly
                        state.input = format!("/{}", cmd.name);
                        state.input_cursor = state.input.len();
                        state.ac_cursor = 0;
                        // Fall through to normal Enter handling below
                    } else {
                        // Fill in and let user type args
                        state.input = format!("/{} ", cmd.name);
                        state.input_cursor = state.input.len();
                        state.ac_cursor = 0;
                        return false;
                    }
                }
                // Fall through
            }
            _ => {
                // Reset autocomplete cursor when typing
                state.ac_cursor = 0;
                // Fall through to normal key handling
            }
        }
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
            state.ac_cursor = 0;
            if !input.starts_with('/') {
                state.cmd_history.push(input.clone());
            }

            if let Some((cmd, arg)) = match_command(&input) {
                return handle_command(cmd.name, &arg, state, &input).await;
            }
            send_message(state, &input);
            false
        }
        KeyCode::Char(c) => {
            state.input.insert(state.input_cursor, c);
            state.input_cursor += 1;
            state.cmd_history_idx = None;
            state.ac_cursor = 0;
            false
        }
        KeyCode::Backspace => {
            if state.input_cursor > 0 {
                state.input_cursor -= 1;
                state.input.remove(state.input_cursor);
                state.ac_cursor = 0;
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
        KeyCode::Tab => false,
        _ => false,
    }
}

fn handle_picker_key(key: KeyEvent, state: &mut ChatState) -> bool {
    let picker = match &mut state.model_picker {
        Some(p) => p,
        None => return false,
    };
    match key.code {
        KeyCode::Esc => {
            state.model_picker = None;
            false
        }
        KeyCode::Up => {
            picker.cursor = picker.cursor.saturating_sub(1);
            false
        }
        KeyCode::Down => {
            let max = picker.filtered().len().saturating_sub(1);
            picker.cursor = (picker.cursor + 1).min(max);
            false
        }
        KeyCode::Enter => {
            let filtered = picker.filtered();
            if let Some((_, model)) = filtered.get(picker.cursor) {
                let id = model.id.clone();
                state.model = id.clone();
                state.model_picker = None;
                if let Err(e) = state.rebuild_provider() {
                    state.sys_msg(format!("failed: {}", e));
                } else {
                    state.sys_msg(format!("model → {}", id));
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

// ---------------------------------------------------------------------------
// Command dispatch
// ---------------------------------------------------------------------------

async fn handle_command(name: &str, arg: &str, state: &mut ChatState, raw: &str) -> bool {
    match name {
        "quit" => return true,
        "clear" => {
            state.messages.clear();
            state.scroll = 0;
        }
        "help" => {
            state.show_help = true;
        }

        "model" => {
            if arg.is_empty() {
                state.model_picker = Some(ModelPicker {
                    models: Vec::new(),
                    cursor: 0,
                    filter: String::new(),
                    loading: true,
                    error: None,
                });
            } else {
                state.model = arg.to_string();
                if let Err(e) = state.rebuild_provider() {
                    state.sys_msg(format!("failed: {}", e));
                } else {
                    state.sys_msg(format!("model → {}", arg));
                }
            }
        }
        "provider" => {
            if arg.is_empty() {
                state.sys_msg(format!(
                    "current: {} — options: ollama, openrouter, xai, custom",
                    state.provider_kind
                ));
            } else {
                match arg.parse::<ProviderKind>() {
                    Ok(k) => {
                        state.provider_kind = k.clone();
                        state.endpoint = providers::default_endpoint(&k).to_string();
                        state.model = providers::default_model(&k).to_string();
                        if let Err(e) = state.rebuild_provider() {
                            state.sys_msg(format!("failed: {}", e));
                        } else {
                            state.sys_msg(format!(
                                "provider → {} ({})",
                                state.provider_kind, state.model
                            ));
                        }
                    }
                    Err(e) => state.sys_msg(e),
                }
            }
        }
        "system" => {
            if arg.is_empty() {
                if state.system_prompt.is_empty() {
                    state.sys_msg("no system prompt set".into());
                } else {
                    state.sys_msg(format!("system: {}", state.system_prompt));
                }
            } else {
                state.system_prompt = arg.to_string();
                state.sys_msg(format!("system prompt set ({} chars)", arg.len()));
            }
        }

        // --- Agent orchestration ---
        "run" => {
            if arg.is_empty() {
                state.sys_msg("usage: /run <capability> [payload]\n  /run system.info\n  /run echo.ping hello\n  /run llm.chat explain QUIC".into());
            } else {
                let (cap, payload) = parse_cap_payload(arg);
                let result = execute_agent(&cap, &payload, state).await;
                match result {
                    Ok(output) => state.sys_msg(format!("{} → {}", cap, output)),
                    Err(e) => state.sys_msg(format!("{} failed: {}", cap, e)),
                }
            }
        }
        "spawn" => {
            if arg.is_empty() {
                state.sys_msg("usage: /spawn <capability> [payload]\nlaunches agent in background, results shown when done".into());
            } else {
                let (cap, payload) = parse_cap_payload(arg);
                spawn_job(state, &cap, &payload);
            }
        }
        "jobs" => {
            if state.jobs.is_empty() {
                state.sys_msg("no jobs".into());
            } else {
                let mut lines = Vec::new();
                for job in &state.jobs {
                    let elapsed = job.started.elapsed().as_secs();
                    let status = if job.rx.is_some() {
                        format!("running {}s", elapsed)
                    } else {
                        match &job.result {
                            Some(Ok(_)) => "done".into(),
                            Some(Err(e)) => format!("failed: {}", e),
                            None => "cancelled".into(),
                        }
                    };
                    lines.push(format!(
                        "  #{:<3} {:<20} {}",
                        job.id, job.capability, status
                    ));
                }
                state.sys_msg(lines.join("\n"));
            }
        }
        "kill" => {
            if let Ok(id) = arg.trim_start_matches('#').parse::<usize>() {
                if let Some(job) = state.jobs.iter_mut().find(|j| j.id == id) {
                    job.rx = None;
                    job.result = Some(Err("killed".into()));
                    state.sys_msg(format!("job #{} killed", id));
                } else {
                    state.sys_msg(format!("no job #{}", id));
                }
            } else {
                state.sys_msg("usage: /kill <job-id>".into());
            }
        }
        "agents" => {
            let cfg = config::load_config();
            let auto_label = if state.auto_agent { "on" } else { "off" };
            let mut lines = vec![
                format!(
                    "agents:                                  auto-agent: {}",
                    auto_label
                ),
                format!("  echo.ping        echo back a message            /run echo.ping hello"),
                format!("  system.info      hostname, os, arch             /run system.info"),
                format!("  llm.chat         prompt the LLM                 /run llm.chat <prompt>"),
            ];
            let mcp_count = cfg.mcp.servers.len();
            if mcp_count > 0 {
                lines.push(format!(
                    "  mcp.*            {} MCP server(s)               (via axon start)",
                    mcp_count
                ));
            }
            lines.push(String::new());
            lines.push("/run  — execute and wait     /spawn — launch in background".into());
            lines.push("/auto — let the LLM launch agents autonomously".into());
            state.sys_msg(lines.join("\n"));
        }
        "auto" => {
            state.auto_agent = !state.auto_agent;
            if state.auto_agent {
                state.sys_msg(
                    "auto-agent on — the LLM can now launch agents via [[run:cap:payload]]".into(),
                );
            } else {
                state.sys_msg("auto-agent off".into());
            }
        }
        // Keep /task as alias for /run
        "task" => {
            if arg.is_empty() {
                state.sys_msg("use /run or /spawn — see /agents for available capabilities".into());
            } else {
                let (cap, payload) = parse_cap_payload(arg);
                let result = execute_agent(&cap, &payload, state).await;
                match result {
                    Ok(output) => state.sys_msg(format!("{} → {}", cap, output)),
                    Err(e) => state.sys_msg(format!("{} failed: {}", cap, e)),
                }
            }
        }
        "tools" => {
            let cfg = config::load_config();
            if cfg.mcp.servers.is_empty() {
                state.sys_msg(
                    "no MCP servers configured\nadd [[mcp.servers]] to ~/.config/axon/config.toml"
                        .into(),
                );
            } else {
                let mut lines = vec![format!(
                    "{} MCP server(s) configured:",
                    cfg.mcp.servers.len()
                )];
                for s in &cfg.mcp.servers {
                    lines.push(format!("  {} — {} {}", s.name, s.command, s.args.join(" ")));
                }
                lines.push(String::new());
                lines.push("tools are available when the node is running (axon start)".to_string());
                state.sys_msg(lines.join("\n"));
            }
        }
        "status" => {
            let cfg = config::load_config();
            let has_key = !cfg.llm.api_key.is_empty()
                || !providers::resolve_api_key("", &state.provider_kind).is_empty();
            let id_path = axon_core::Identity::default_path();
            let has_id = id_path.exists();
            let lines = vec![
                format!("provider:  {}", state.provider_kind),
                format!("model:     {}", state.model),
                format!("endpoint:  {}", state.endpoint),
                format!(
                    "api key:   {}",
                    if has_key { "configured" } else { "missing" }
                ),
                format!(
                    "identity:  {}",
                    if has_id { "exists" } else { "not created" }
                ),
                format!("mcp:       {} server(s)", cfg.mcp.servers.len()),
                format!(
                    "tokens:    {} prompt + {} completion",
                    state.total_prompt_tokens, state.total_completion_tokens
                ),
            ];
            state.sys_msg(lines.join("\n"));
        }
        "trust" => {
            state.sys_msg("trust scores require a running node with peer history\nstart with: axon start\nview with: axon trust show".into());
        }
        "route" => {
            if arg.is_empty() {
                state.sys_msg("usage: /route <peer-addr> <message>\nroutes a prompt to a specific peer's LLM agent over QUIC".into());
            } else {
                state.sys_msg(
                    "routing requires a running node\nstart with: axon start, then use /route"
                        .into(),
                );
            }
        }

        _ => state.sys_msg(format!("unknown: /{}", name)),
    }

    // Handle /system with args
    if name == "system" && raw.starts_with("/system ") {
        state.system_prompt = raw[8..].trim().to_string();
    }
    false
}

// ---------------------------------------------------------------------------
// Agent execution
// ---------------------------------------------------------------------------

fn parse_cap_payload(arg: &str) -> (String, String) {
    let mut parts = arg.splitn(2, ' ');
    let cap = parts.next().unwrap_or("").to_string();
    let payload = parts.next().unwrap_or("").to_string();
    (cap, payload)
}

async fn execute_agent(cap: &str, payload: &str, state: &ChatState) -> Result<String, String> {
    let parts: Vec<&str> = cap.splitn(2, '.').collect();
    if parts.len() != 2 {
        return Err(format!("capability must be namespace.name, got: {}", cap));
    }
    let (ns, name) = (parts[0], parts[1]);

    match (ns, name) {
        ("echo", "ping") => Ok(if payload.is_empty() {
            "pong".into()
        } else {
            payload.to_string()
        }),
        ("system", "info") => {
            let hostname = std::env::var("HOSTNAME")
                .or_else(|_| std::fs::read_to_string("/etc/hostname").map(|s| s.trim().to_string()))
                .unwrap_or_else(|_| "unknown".into());
            Ok(format!(
                "host: {}  os: {}  arch: {}",
                hostname,
                std::env::consts::OS,
                std::env::consts::ARCH
            ))
        }
        ("llm", "chat") => {
            if payload.is_empty() {
                return Err("llm.chat requires a payload".into());
            }
            let provider = build_provider(
                &state.provider_kind,
                &state.endpoint,
                &state.api_key,
                &state.model,
            )
            .map_err(|e| format!("{}", e))?;
            let resp = provider
                .complete(CompletionRequest {
                    prompt: payload.to_string(),
                    max_tokens: None,
                    temperature: None,
                })
                .await
                .map_err(|e| format!("{}", e))?;
            Ok(resp.text.trim().to_string())
        }
        _ => Err(format!("no agent for {}", cap)),
    }
}

fn spawn_job(state: &mut ChatState, cap: &str, payload: &str) {
    let id = state.next_job_id;
    state.next_job_id += 1;

    let (tx, rx) = oneshot::channel();
    let cap_owned = cap.to_string();
    let payload_owned = payload.to_string();
    let pk = state.provider_kind.clone();
    let ep = state.endpoint.clone();
    let key = state.api_key.clone();
    let mdl = state.model.clone();

    tokio::spawn(async move {
        let parts: Vec<&str> = cap_owned.splitn(2, '.').collect();
        let result = if parts.len() != 2 {
            Err(format!("invalid capability: {}", cap_owned))
        } else {
            match (parts[0], parts[1]) {
                ("echo", "ping") => Ok(if payload_owned.is_empty() {
                    "pong".into()
                } else {
                    payload_owned
                }),
                ("system", "info") => {
                    let h = std::env::var("HOSTNAME")
                        .or_else(|_| {
                            std::fs::read_to_string("/etc/hostname").map(|s| s.trim().to_string())
                        })
                        .unwrap_or_else(|_| "unknown".into());
                    Ok(format!(
                        "host: {}  os: {}  arch: {}",
                        h,
                        std::env::consts::OS,
                        std::env::consts::ARCH
                    ))
                }
                ("llm", "chat") => match build_provider(&pk, &ep, &key, &mdl) {
                    Ok(p) => p
                        .complete(CompletionRequest {
                            prompt: payload_owned,
                            max_tokens: None,
                            temperature: None,
                        })
                        .await
                        .map(|r| r.text.trim().to_string())
                        .map_err(|e| format!("{}", e)),
                    Err(e) => Err(format!("{}", e)),
                },
                _ => Err(format!("no agent for {}", cap_owned)),
            }
        };
        let _ = tx.send(result);
    });

    state.jobs.push(BackgroundJob {
        id,
        capability: cap.to_string(),
        payload: payload.to_string(),
        started: Instant::now(),
        rx: Some(rx),
        result: None,
    });

    state.sys_msg(format!("job #{} spawned: {}", id, cap));
}

fn poll_jobs(state: &mut ChatState) {
    for job in &mut state.jobs {
        if let Some(mut rx) = job.rx.take() {
            match rx.try_recv() {
                Ok(result) => {
                    let elapsed = job.started.elapsed();
                    let time_str = if elapsed.as_secs() > 0 {
                        format!("{:.1}s", elapsed.as_secs_f64())
                    } else {
                        format!("{}ms", elapsed.as_millis())
                    };
                    match &result {
                        Ok(output) => {
                            state.messages.push(ChatMessage {
                                role: Role::System,
                                content: format!(
                                    "job #{} ({}) done in {}\n  {}",
                                    job.id, job.capability, time_str, output
                                ),
                                duration_ms: Some(elapsed.as_millis() as u64),
                                tokens: None,
                            });
                        }
                        Err(e) => {
                            state.messages.push(ChatMessage {
                                role: Role::System,
                                content: format!(
                                    "job #{} ({}) failed in {}: {}",
                                    job.id, job.capability, time_str, e
                                ),
                                duration_ms: Some(elapsed.as_millis() as u64),
                                tokens: None,
                            });
                        }
                    }
                    job.result = Some(result);
                    state.auto_scroll = true;
                }
                Err(oneshot::error::TryRecvError::Empty) => {
                    job.rx = Some(rx); // still running
                }
                Err(oneshot::error::TryRecvError::Closed) => {
                    job.result = Some(Err("task dropped".into()));
                }
            }
        }
    }
}

/// Parse LLM response for [[run:capability:payload]] markers and execute them.
fn extract_agent_calls(text: &str) -> Vec<(String, String)> {
    let mut calls = Vec::new();
    let mut remaining = text;
    while let Some(start) = remaining.find("[[run:") {
        let after = &remaining[start + 6..];
        if let Some(end) = after.find("]]") {
            let inner = &after[..end];
            // Split on first : to get cap:payload
            let mut parts = inner.splitn(2, ':');
            let cap = parts.next().unwrap_or("").trim().to_string();
            let payload = parts.next().unwrap_or("").trim().to_string();
            if !cap.is_empty() {
                calls.push((cap, payload));
            }
            remaining = &after[end + 2..];
        } else {
            break;
        }
    }
    calls
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
    let pk = state.provider_kind.clone();
    let ep = state.endpoint.clone();
    let key = state.api_key.clone();
    let mdl = state.model.clone();

    tokio::spawn(async move {
        let result = match build_provider(&pk, &ep, &key, &mdl) {
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

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);

    render_header(frame, state, chunks[0]);
    render_messages(frame, state, chunks[1]);
    render_input(frame, state, chunks[2]);
    render_status_bar(frame, state, chunks[3]);

    // Autocomplete popup (above input)
    if state.has_suggestions() && state.model_picker.is_none() && !state.show_help {
        render_autocomplete(frame, state, chunks[2]);
    }

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
        let t = if total_tokens > 1000 {
            format!("{:.1}k", total_tokens as f64 / 1000.0)
        } else {
            format!("{}", total_tokens)
        };
        format!("{} {} · {} tok ", state.provider_kind, model_short, t)
    } else {
        format!("{} {} ", state.provider_kind, model_short)
    };
    let left = " ▲ axon ";
    let w = area.width as usize;
    let pad = w.saturating_sub(left.len()).saturating_sub(right.len());

    let line = Line::from(vec![
        Span::styled(left, Style::default().fg(ACCENT).bold()),
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
                lines.push(Line::from(Span::styled(
                    "  you",
                    Style::default().fg(ACCENT).bold(),
                )));
                for l in msg.content.lines() {
                    for wr in wrap(l, w.saturating_sub(6)) {
                        lines.push(Line::from(Span::styled(
                            format!("  {}", wr),
                            Style::default().fg(TEXT),
                        )));
                    }
                }
            }
            Role::Assistant => {
                lines.push(Line::from(""));
                let mut label = vec![Span::styled("  axon", Style::default().fg(LABEL).bold())];
                if let Some(ms) = msg.duration_ms {
                    let t = if ms >= 1000 {
                        format!("{:.1}s", ms as f64 / 1000.0)
                    } else {
                        format!("{}ms", ms)
                    };
                    label.push(Span::styled(format!("  {}", t), Style::default().fg(FAINT)));
                }
                if let Some((_, c)) = msg.tokens {
                    label.push(Span::styled(
                        format!(" · {}tok", c),
                        Style::default().fg(FAINT),
                    ));
                }
                lines.push(Line::from(label));
                for l in msg.content.lines() {
                    for wr in wrap(l, w.saturating_sub(6)) {
                        lines.push(Line::from(Span::styled(
                            format!("  {}", wr),
                            Style::default().fg(TEXT_DIM),
                        )));
                    }
                }
            }
            Role::System => {
                lines.push(Line::from(""));
                for l in msg.content.lines() {
                    for wr in wrap(l, w.saturating_sub(6)) {
                        lines.push(Line::from(Span::styled(
                            format!("  {}", wr),
                            Style::default().fg(DIM).italic(),
                        )));
                    }
                }
            }
        }
    }

    if state.pending.is_some() {
        lines.push(Line::from(""));
        let f = (state.spinner_tick / 2) % SPINNER.len();
        let elapsed = state
            .pending_start
            .map(|s| s.elapsed().as_secs())
            .unwrap_or(0);
        let t = if elapsed > 0 {
            format!("  {}s", elapsed)
        } else {
            String::new()
        };
        lines.push(Line::from(vec![
            Span::styled(format!("  {} ", SPINNER[f]), Style::default().fg(ACCENT)),
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
            area,
            &mut sb,
        );
    }
}

fn render_input(frame: &mut Frame, state: &ChatState, area: Rect) {
    let waiting = state.pending.is_some();
    let (icon, ic) = if waiting {
        ("◌ ", FAINT)
    } else {
        ("❯ ", ACCENT)
    };
    let text = if waiting {
        "waiting...".to_string()
    } else if state.input.is_empty() {
        "message or /command".to_string()
    } else {
        state.input.clone()
    };
    let tc = if waiting || state.input.is_empty() {
        FAINT
    } else {
        TEXT
    };

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

fn render_autocomplete(frame: &mut Frame, state: &ChatState, input_area: Rect) {
    let suggestions = filtered_suggestions(&state.input);
    if suggestions.is_empty() {
        return;
    }

    let count = suggestions.len().min(10) as u16;
    let w = 50u16.min(input_area.width.saturating_sub(2));
    let h = count + 2; // borders
    let x = input_area.x + 1;
    let y = input_area.y.saturating_sub(h);
    let popup = Rect::new(x, y, w, h);

    frame.render_widget(Block::default().style(Style::default().bg(POPUP_BG)), popup);

    let mut lines: Vec<Line> = Vec::new();
    for (i, cmd) in suggestions.iter().enumerate().take(10) {
        let sel = i == state.ac_cursor;
        let name_style = if sel {
            Style::default().fg(TEXT).bold()
        } else {
            Style::default().fg(ACCENT)
        };
        let desc_style = if sel {
            Style::default().fg(DIM)
        } else {
            Style::default().fg(FAINT)
        };

        let args_str = if cmd.args.is_empty() {
            String::new()
        } else {
            format!(" {}", cmd.args)
        };

        lines.push(Line::from(vec![
            Span::styled(
                if sel { " ▸ " } else { "   " },
                Style::default().fg(if sel { ACCENT } else { FAINT }),
            ),
            Span::styled(format!("/{}", cmd.name), name_style),
            Span::styled(args_str, Style::default().fg(FAINT)),
            Span::styled(format!("  {}", cmd.desc), desc_style),
        ]));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(FAINT))
        .style(Style::default().bg(POPUP_BG));
    frame.render_widget(Paragraph::new(lines).block(block), popup);
}

fn render_status_bar(frame: &mut Frame, state: &ChatState, area: Rect) {
    let n = state
        .messages
        .iter()
        .filter(|m| matches!(m.role, Role::User))
        .count();
    let running = state.jobs.iter().filter(|j| j.rx.is_some()).count();
    let mut spans = vec![
        Span::styled(" enter", Style::default().fg(DIM)),
        Span::styled(" send  ", Style::default().fg(FAINT)),
        Span::styled("tab", Style::default().fg(DIM)),
        Span::styled(" complete  ", Style::default().fg(FAINT)),
        Span::styled("esc", Style::default().fg(DIM)),
        Span::styled(" quit  ", Style::default().fg(FAINT)),
    ];
    if running > 0 {
        spans.push(Span::styled(
            format!("{}job ", running),
            Style::default().fg(ACCENT),
        ));
    }
    if state.auto_agent {
        spans.push(Span::styled("auto ", Style::default().fg(ACCENT)));
    }
    spans.push(Span::styled(
        format!("{}msg", n),
        Style::default().fg(FAINT),
    ));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_help(frame: &mut Frame, area: Rect) {
    let w = 60u16.min(area.width.saturating_sub(4));
    let h = 26u16.min(area.height.saturating_sub(2));
    let popup = centered(area, w, h);

    frame.render_widget(Block::default().style(Style::default().bg(POPUP_BG)), popup);

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  orchestration",
            Style::default().fg(TEXT).bold(),
        )),
        Line::from(""),
    ];
    for cmd in COMMANDS.iter().filter(|c| c.category == "mesh") {
        lines.push(help_line(cmd));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  model / provider",
        Style::default().fg(TEXT).bold(),
    )));
    lines.push(Line::from(""));
    for cmd in COMMANDS.iter().filter(|c| c.category == "llm") {
        lines.push(help_line(cmd));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  session",
        Style::default().fg(TEXT).bold(),
    )));
    lines.push(Line::from(""));
    for cmd in COMMANDS.iter().filter(|c| c.category == "session") {
        lines.push(help_line(cmd));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  press any key",
        Style::default().fg(FAINT).italic(),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(FAINT))
        .style(Style::default().bg(POPUP_BG));
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

    frame.render_widget(Block::default().style(Style::default().bg(POPUP_BG)), popup);

    if picker.loading {
        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "  loading models...",
                Style::default().fg(DIM).italic(),
            )),
        ];
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(FAINT))
            .title(Span::styled(" model ", Style::default().fg(ACCENT)))
            .style(Style::default().bg(POPUP_BG));
        frame.render_widget(Paragraph::new(lines).block(block), popup);
        return;
    }

    let filtered = picker.filtered();
    let inner_h = popup.height.saturating_sub(4) as usize;
    let scroll = if picker.cursor >= inner_h {
        picker.cursor - inner_h + 1
    } else {
        0
    };

    let mut lines: Vec<Line> = Vec::new();
    if !picker.filter.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("  /", Style::default().fg(FAINT)),
            Span::styled(&picker.filter, Style::default().fg(ACCENT)),
        ]));
    } else {
        lines.push(Line::from(Span::styled(
            "  type to filter",
            Style::default().fg(FAINT).italic(),
        )));
    }
    lines.push(Line::from(""));

    if let Some(err) = &picker.error {
        lines.push(Line::from(Span::styled(
            format!("  {}", err),
            Style::default().fg(Color::Rgb(150, 70, 70)),
        )));
    }

    for (i, (_, model)) in filtered.iter().enumerate().skip(scroll).take(inner_h) {
        let sel = i == picker.cursor;
        let is_current = model.id == state.model;
        let mut spans = vec![
            Span::styled(
                if sel { " ▸ " } else { "   " },
                Style::default().fg(if sel { ACCENT } else { FAINT }),
            ),
            Span::styled(
                &model.id,
                if sel {
                    Style::default().fg(ACCENT).bold()
                } else {
                    Style::default().fg(TEXT)
                },
            ),
        ];
        if is_current {
            spans.push(Span::styled(" ●", Style::default().fg(LABEL)));
        }
        if let Some(ctx) = model.context_length {
            let c = if ctx >= 1_000_000 {
                format!("{}M", ctx / 1_000_000)
            } else {
                format!("{}K", ctx / 1_000)
            };
            spans.push(Span::styled(format!("  {}", c), Style::default().fg(FAINT)));
        }
        lines.push(Line::from(spans));
    }

    let title = format!(" model ({}) ", filtered.len());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(FAINT))
        .title(Span::styled(title, Style::default().fg(ACCENT)))
        .title_bottom(Line::from(Span::styled(
            " ↑↓ enter esc ",
            Style::default().fg(FAINT),
        )))
        .style(Style::default().bg(POPUP_BG));
    frame.render_widget(Paragraph::new(lines).block(block), popup);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn help_line(cmd: &CmdDef) -> Line<'static> {
    let args = if cmd.args.is_empty() {
        String::new()
    } else {
        format!(" {}", cmd.args)
    };
    Line::from(vec![
        Span::styled(
            format!("  /{:<12}{:<12}", cmd.name, args),
            Style::default().fg(ACCENT),
        ),
        Span::styled(cmd.desc.to_string(), Style::default().fg(DIM)),
    ])
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    Rect::new(
        (area.width.saturating_sub(w)) / 2,
        (area.height.saturating_sub(h)) / 2,
        w.min(area.width),
        h.min(area.height),
    )
}

fn wrap(text: &str, max: usize) -> Vec<String> {
    if max == 0 || text.len() <= max {
        return vec![text.to_string()];
    }
    let mut out = Vec::new();
    let mut rem = text;
    while rem.len() > max {
        let at = rem[..max].rfind(' ').unwrap_or(max);
        let (l, r) = rem.split_at(at);
        out.push(l.to_string());
        rem = r.trim_start();
    }
    if !rem.is_empty() {
        out.push(rem.to_string());
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}
