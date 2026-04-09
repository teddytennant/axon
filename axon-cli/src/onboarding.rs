use crate::config::{self, LlmSection, NodeConfig, NodeSection};
use crate::providers::{self, ModelInfo, ProviderKind};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, Padding, Paragraph, Wrap},
};
use std::io::stdout;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Theme (matches dashboard)
// ---------------------------------------------------------------------------

const BRAND_CYAN: Color = Color::Rgb(0, 200, 200);
const BRAND_GREEN: Color = Color::Rgb(80, 220, 120);
const BRAND_YELLOW: Color = Color::Rgb(240, 200, 60);
const BRAND_RED: Color = Color::Rgb(240, 80, 80);
const BRAND_DIM: Color = Color::Rgb(100, 100, 110);
const BRAND_BG: Color = Color::Reset;
const ACCENT_BLUE: Color = Color::Rgb(80, 140, 240);
const SURFACE: Color = Color::Reset;

// ---------------------------------------------------------------------------
// Wizard steps
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Welcome,
    Provider,
    ApiKey,
    Models,
    Done,
}

impl Step {
    fn index(self) -> usize {
        match self {
            Step::Welcome => 0,
            Step::Provider => 1,
            Step::ApiKey => 2,
            Step::Models => 3,
            Step::Done => 4,
        }
    }

    fn total() -> usize {
        5
    }
}

// ---------------------------------------------------------------------------
// Provider option
// ---------------------------------------------------------------------------

struct ProviderOption {
    kind: ProviderKind,
    label: &'static str,
    desc: &'static str,
    needs_key: bool,
}

const PROVIDERS: &[ProviderOption] = &[
    ProviderOption {
        kind: ProviderKind::Ollama,
        label: "Ollama",
        desc: "Local models, no API key needed",
        needs_key: false,
    },
    ProviderOption {
        kind: ProviderKind::OpenRouter,
        label: "OpenRouter",
        desc: "200+ models — Claude, GPT, Gemini, Mistral, DeepSeek...",
        needs_key: true,
    },
    ProviderOption {
        kind: ProviderKind::Xai,
        label: "xAI (Grok)",
        desc: "Grok models from xAI",
        needs_key: true,
    },
    ProviderOption {
        kind: ProviderKind::Custom,
        label: "Custom Endpoint",
        desc: "Any OpenAI-compatible API",
        needs_key: true,
    },
];

// ---------------------------------------------------------------------------
// Wizard state
// ---------------------------------------------------------------------------

struct WizardState {
    step: Step,
    // Provider selection
    provider_cursor: usize,
    selected_provider: Option<usize>,
    // API key input
    api_key_input: String,
    api_key_cursor: usize,
    api_key_show: bool,
    #[allow(dead_code)]
    api_key_validating: bool,
    api_key_error: Option<String>,
    // Model selection
    models: Vec<ModelInfo>,
    models_loading: bool,
    models_error: Option<String>,
    model_cursor: usize,
    model_filter: String,
    model_filtering: bool,
    // Custom endpoint
    custom_endpoint: String,
    editing_endpoint: bool,
    // Result
    saved_path: Option<String>,
}

impl WizardState {
    fn new() -> Self {
        Self {
            step: Step::Welcome,
            provider_cursor: 0,
            selected_provider: None,
            api_key_input: String::new(),
            api_key_cursor: 0,
            api_key_show: false,
            api_key_validating: false,
            api_key_error: None,
            models: Vec::new(),
            models_loading: false,
            models_error: None,
            model_cursor: 0,
            model_filter: String::new(),
            model_filtering: false,
            custom_endpoint: String::new(),
            editing_endpoint: false,
            saved_path: None,
        }
    }

    fn current_provider(&self) -> &ProviderOption {
        &PROVIDERS[self.selected_provider.unwrap_or(0)]
    }

    fn filtered_models(&self) -> Vec<&ModelInfo> {
        if self.model_filter.is_empty() {
            self.models.iter().collect()
        } else {
            let q = self.model_filter.to_lowercase();
            self.models
                .iter()
                .filter(|m| {
                    m.id.to_lowercase().contains(&q)
                        || m.name.to_lowercase().contains(&q)
                        || m.description.to_lowercase().contains(&q)
                })
                .collect()
        }
    }

    fn build_config(&self) -> NodeConfig {
        let provider = self.current_provider();
        let model_id = {
            let filtered = self.filtered_models();
            filtered
                .get(self.model_cursor)
                .map(|m| m.id.clone())
                .unwrap_or_else(|| providers::default_model(&provider.kind).to_string())
        };

        let endpoint = if provider.kind == ProviderKind::Custom {
            self.custom_endpoint.clone()
        } else {
            providers::default_endpoint(&provider.kind).to_string()
        };

        NodeConfig {
            node: NodeSection::default(),
            llm: LlmSection {
                provider: provider.kind.to_string(),
                endpoint,
                api_key: self.api_key_input.clone(),
                model: model_id,
            },
            mcp: Default::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Run the onboarding wizard. Returns Ok(true) if config was saved.
pub async fn run_onboarding() -> anyhow::Result<bool> {
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    let mut state = WizardState::new();

    let result = run_wizard_loop(&mut terminal, &mut state).await;

    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;

    result
}

/// Run the onboarding for a specific provider (for `axon auth <provider>`).
pub async fn run_auth(provider: &ProviderKind) -> anyhow::Result<bool> {
    let idx = PROVIDERS.iter().position(|p| p.kind == *provider).unwrap_or(0);

    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    let mut state = WizardState::new();
    state.selected_provider = Some(idx);

    // Load existing config if any
    if config::config_exists() {
        let existing = config::load_config();
        if !existing.llm.api_key.is_empty() {
            state.api_key_input = existing.llm.api_key;
            state.api_key_cursor = state.api_key_input.len();
        }
    }

    // Skip to API key step
    state.step = if PROVIDERS[idx].needs_key {
        Step::ApiKey
    } else {
        Step::Models
    };

    let result = run_wizard_loop(&mut terminal, &mut state).await;

    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    result
}

// ---------------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------------

async fn run_wizard_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    state: &mut WizardState,
) -> anyhow::Result<bool> {
    loop {
        terminal.draw(|frame| render(frame, state))?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match handle_key(key, state) {
                    Action::None => {}
                    Action::Quit => return Ok(false),
                    Action::Next => advance(state).await,
                    Action::Back => go_back(state),
                    Action::_FetchModels => {
                        state.models_loading = true;
                        state.models_error = None;
                        state.models.clear();
                        fetch_models_for_state(state).await;
                    }
                    Action::Save => {
                        let cfg = state.build_config();
                        match config::save_config(&cfg) {
                            Ok(path) => {
                                state.saved_path = Some(path.display().to_string());
                                state.step = Step::Done;
                            }
                            Err(e) => {
                                state.models_error = Some(format!("Save failed: {}", e));
                            }
                        }
                    }
                    Action::Finish => return Ok(state.saved_path.is_some()),
                }
            }
        }
    }
}

enum Action {
    None,
    Quit,
    Next,
    Back,
    _FetchModels,
    Save,
    Finish,
}

fn handle_key(key: KeyEvent, state: &mut WizardState) -> Action {
    // Global quit
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Action::Quit;
    }

    match state.step {
        Step::Welcome => match key.code {
            KeyCode::Enter | KeyCode::Char(' ') => Action::Next,
            KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
            _ => Action::None,
        },
        Step::Provider => match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                state.provider_cursor = state.provider_cursor.saturating_sub(1);
                Action::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                state.provider_cursor =
                    (state.provider_cursor + 1).min(PROVIDERS.len() - 1);
                Action::None
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                state.selected_provider = Some(state.provider_cursor);
                Action::Next
            }
            KeyCode::Esc | KeyCode::Char('q') => Action::Quit,
            _ => Action::None,
        },
        Step::ApiKey => {
            if state.editing_endpoint {
                // Editing custom endpoint
                match key.code {
                    KeyCode::Char(c) => {
                        state.custom_endpoint.push(c);
                        Action::None
                    }
                    KeyCode::Backspace => {
                        state.custom_endpoint.pop();
                        Action::None
                    }
                    KeyCode::Enter | KeyCode::Tab => {
                        state.editing_endpoint = false;
                        Action::None
                    }
                    KeyCode::Esc => Action::Back,
                    _ => Action::None,
                }
            } else {
                match key.code {
                    KeyCode::Char(c) => {
                        state
                            .api_key_input
                            .insert(state.api_key_cursor, c);
                        state.api_key_cursor += 1;
                        state.api_key_error = None;
                        Action::None
                    }
                    KeyCode::Backspace => {
                        if state.api_key_cursor > 0 {
                            state.api_key_cursor -= 1;
                            state.api_key_input.remove(state.api_key_cursor);
                            state.api_key_error = None;
                        }
                        Action::None
                    }
                    KeyCode::Left => {
                        state.api_key_cursor = state.api_key_cursor.saturating_sub(1);
                        Action::None
                    }
                    KeyCode::Right => {
                        state.api_key_cursor =
                            (state.api_key_cursor + 1).min(state.api_key_input.len());
                        Action::None
                    }
                    KeyCode::Enter => {
                        if state.api_key_input.is_empty()
                            && state.current_provider().needs_key
                        {
                            state.api_key_error = Some("API key is required".into());
                            Action::None
                        } else {
                            Action::Next
                        }
                    }
                    KeyCode::Tab => {
                        state.api_key_show = !state.api_key_show;
                        Action::None
                    }
                    KeyCode::Esc => Action::Back,
                    _ => Action::None,
                }
            }
        }
        Step::Models => {
            if state.model_filtering {
                match key.code {
                    KeyCode::Char(c) => {
                        state.model_filter.push(c);
                        state.model_cursor = 0;
                        Action::None
                    }
                    KeyCode::Backspace => {
                        state.model_filter.pop();
                        state.model_cursor = 0;
                        Action::None
                    }
                    KeyCode::Esc | KeyCode::Enter => {
                        state.model_filtering = false;
                        Action::None
                    }
                    _ => Action::None,
                }
            } else {
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        state.model_cursor = state.model_cursor.saturating_sub(1);
                        Action::None
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        let max = state.filtered_models().len().saturating_sub(1);
                        state.model_cursor = (state.model_cursor + 1).min(max);
                        Action::None
                    }
                    KeyCode::Char('/') => {
                        state.model_filtering = true;
                        Action::None
                    }
                    KeyCode::Enter => Action::Save,
                    KeyCode::Esc => Action::Back,
                    _ => Action::None,
                }
            }
        }
        Step::Done => match key.code {
            KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Char('q') | KeyCode::Esc => {
                Action::Finish
            }
            _ => Action::None,
        },
    }
}

async fn advance(state: &mut WizardState) {
    match state.step {
        Step::Welcome => {
            state.step = Step::Provider;
        }
        Step::Provider => {
            let provider = state.current_provider();
            if provider.needs_key {
                // For custom, start editing endpoint
                if provider.kind == ProviderKind::Custom {
                    state.editing_endpoint = true;
                }
                state.step = Step::ApiKey;
            } else {
                // Skip API key for Ollama, go straight to models
                state.step = Step::Models;
                state.models_loading = true;
                fetch_models_for_state(state).await;
            }
        }
        Step::ApiKey => {
            state.step = Step::Models;
            state.models_loading = true;
            state.models_error = None;
            fetch_models_for_state(state).await;
        }
        Step::Models => {} // handled by Save action
        Step::Done => {}
    }
}

fn go_back(state: &mut WizardState) {
    match state.step {
        Step::Welcome => {}
        Step::Provider => state.step = Step::Welcome,
        Step::ApiKey => state.step = Step::Provider,
        Step::Models => {
            if state.current_provider().needs_key {
                state.step = Step::ApiKey;
            } else {
                state.step = Step::Provider;
            }
        }
        Step::Done => state.step = Step::Models,
    }
}

async fn fetch_models_for_state(state: &mut WizardState) {
    let provider = &PROVIDERS[state.selected_provider.unwrap_or(0)];
    let endpoint = if provider.kind == ProviderKind::Custom {
        &state.custom_endpoint
    } else {
        providers::default_endpoint(&provider.kind)
    };

    match providers::fetch_models(&provider.kind, endpoint, &state.api_key_input).await {
        Ok(models) => {
            state.models = models;
            state.models_loading = false;
            state.model_cursor = 0;
        }
        Err(e) => {
            state.models_error = Some(format!("{}", e));
            state.models_loading = false;
            // Provide some fallback models
            if provider.kind == ProviderKind::OpenRouter {
                state.models = popular_openrouter_models();
            }
        }
    }
}

fn popular_openrouter_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "anthropic/claude-sonnet-4-6".into(),
            name: "Claude Sonnet 4.6".into(),
            description: "Anthropic's latest balanced model".into(),
            context_length: Some(200000),
        },
        ModelInfo {
            id: "anthropic/claude-opus-4-6".into(),
            name: "Claude Opus 4.6".into(),
            description: "Anthropic's most capable model".into(),
            context_length: Some(1000000),
        },
        ModelInfo {
            id: "openai/gpt-4.1".into(),
            name: "GPT-4.1".into(),
            description: "OpenAI's latest model".into(),
            context_length: Some(1047576),
        },
        ModelInfo {
            id: "google/gemini-2.5-pro-preview".into(),
            name: "Gemini 2.5 Pro".into(),
            description: "Google's advanced reasoning model".into(),
            context_length: Some(1000000),
        },
        ModelInfo {
            id: "x-ai/grok-4.20-beta".into(),
            name: "Grok 4.20".into(),
            description: "xAI's latest flagship".into(),
            context_length: Some(131072),
        },
        ModelInfo {
            id: "deepseek/deepseek-chat".into(),
            name: "DeepSeek Chat".into(),
            description: "DeepSeek V3".into(),
            context_length: Some(65536),
        },
        ModelInfo {
            id: "meta-llama/llama-4-maverick".into(),
            name: "Llama 4 Maverick".into(),
            description: "Meta's open model".into(),
            context_length: Some(1048576),
        },
        ModelInfo {
            id: "mistralai/mistral-large-latest".into(),
            name: "Mistral Large".into(),
            description: "Mistral's flagship".into(),
            context_length: Some(128000),
        },
    ]
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn render(frame: &mut Frame, state: &WizardState) {
    let area = frame.area();

    // Background
    frame.render_widget(Block::default().style(Style::default().bg(BRAND_BG)), area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // top margin
            Constraint::Length(3),  // logo (compact)
            Constraint::Length(1),  // progress bar
            Constraint::Min(6),    // content
            Constraint::Length(1),  // status bar
        ])
        .horizontal_margin(4)
        .split(area);

    render_logo(frame, chunks[1]);
    render_progress(frame, state, chunks[2]);

    match state.step {
        Step::Welcome => render_welcome(frame, chunks[3]),
        Step::Provider => render_provider_select(frame, state, chunks[3]),
        Step::ApiKey => render_api_key(frame, state, chunks[3]),
        Step::Models => render_model_select(frame, state, chunks[3]),
        Step::Done => render_done(frame, state, chunks[3]),
    }

    render_help_bar(frame, state, chunks[4]);
}

fn render_logo(frame: &mut Frame, area: Rect) {
    let logo = vec![
        Line::from(vec![
            Span::styled("  ▲ ", Style::default().fg(BRAND_CYAN).bold()),
            Span::styled("A X O N", Style::default().fg(BRAND_CYAN).bold()),
            Span::styled("  ·  Decentralized AI Agent Mesh", Style::default().fg(BRAND_DIM)),
        ]),
    ];
    frame.render_widget(Paragraph::new(logo), area);
}

fn render_progress(frame: &mut Frame, state: &WizardState, area: Rect) {
    let current = state.step.index();
    let total = Step::total();

    let labels = ["Welcome", "Provider", "API Key", "Model", "Done"];
    let mut spans = Vec::new();
    spans.push(Span::styled("    ", Style::default()));
    for (i, label) in labels.iter().enumerate() {
        let (color, marker) = if i < current {
            (BRAND_GREEN, "●")
        } else if i == current {
            (BRAND_CYAN, "◉")
        } else {
            (BRAND_DIM, "○")
        };
        spans.push(Span::styled(
            format!("{} {} ", marker, label),
            Style::default().fg(color),
        ));
        if i < total - 1 {
            let line_color = if i < current { BRAND_GREEN } else { BRAND_DIM };
            spans.push(Span::styled("─── ", Style::default().fg(line_color)));
        }
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_welcome(frame: &mut Frame, area: Rect) {
    let content = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  Welcome to Axon!",
            Style::default().fg(Color::White).bold(),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Decentralized mesh network for AI agents — discover peers,",
            Style::default().fg(BRAND_DIM),
        )),
        Line::from(Span::styled(
            "  negotiate tasks, build trust, and collaborate.",
            Style::default().fg(BRAND_DIM),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  This wizard will configure your node:",
            Style::default().fg(Color::White),
        )),
        Line::from(vec![
            Span::styled("    1. ", Style::default().fg(BRAND_CYAN).bold()),
            Span::styled("Choose an LLM provider  ", Style::default().fg(Color::White)),
            Span::styled("2. ", Style::default().fg(BRAND_CYAN).bold()),
            Span::styled("Enter API key  ", Style::default().fg(Color::White)),
            Span::styled("3. ", Style::default().fg(BRAND_CYAN).bold()),
            Span::styled("Pick a model", Style::default().fg(Color::White)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  Press Enter to get started.",
            Style::default().fg(BRAND_CYAN).bold(),
        )),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BRAND_DIM))
        .style(Style::default().bg(SURFACE))
        .padding(Padding::horizontal(1));

    frame.render_widget(Paragraph::new(content).block(block), area);
}

fn render_provider_select(frame: &mut Frame, state: &WizardState, area: Rect) {
    let mut lines: Vec<Line> = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  Choose your LLM provider:",
            Style::default().fg(Color::White).bold(),
        )),
        Line::from(""),
    ];

    for (i, provider) in PROVIDERS.iter().enumerate() {
        let is_selected = i == state.provider_cursor;
        let marker = if is_selected { "▸" } else { " " };
        let marker_color = if is_selected { BRAND_CYAN } else { BRAND_DIM };

        lines.push(Line::from(vec![
            Span::styled(format!("  {} ", marker), Style::default().fg(marker_color)),
            Span::styled(
                format!("{:<18}", provider.label),
                if is_selected {
                    Style::default().fg(BRAND_CYAN).bold()
                } else {
                    Style::default().fg(Color::White)
                },
            ),
            Span::styled(
                provider.desc,
                Style::default().fg(if is_selected {
                    BRAND_DIM
                } else {
                    Color::Rgb(70, 70, 80)
                }),
            ),
        ]));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BRAND_DIM))
        .title(Span::styled(
            " Provider ",
            Style::default().fg(BRAND_CYAN).bold(),
        ))
        .style(Style::default().bg(SURFACE))
        .padding(Padding::horizontal(1));

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_api_key(frame: &mut Frame, state: &WizardState, area: Rect) {
    let provider = state.current_provider();

    let key_hint = match provider.kind {
        ProviderKind::OpenRouter => "Get your key at: https://openrouter.ai/keys",
        ProviderKind::Xai => "Get your key at: https://console.x.ai",
        ProviderKind::Custom => "Enter the API key for your endpoint",
        ProviderKind::Ollama => "No key needed!",
    };

    let displayed_key = if state.api_key_show || state.api_key_input.is_empty() {
        state.api_key_input.clone()
    } else {
        let len = state.api_key_input.len();
        if len <= 8 {
            "•".repeat(len)
        } else {
            format!(
                "{}{}",
                &state.api_key_input[..4],
                "•".repeat(len - 4)
            )
        }
    };

    let cursor_char = if state.step == Step::ApiKey && !state.editing_endpoint {
        "█"
    } else {
        ""
    };

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("  {} API Key", provider.label),
            Style::default().fg(Color::White).bold(),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!("  {}", key_hint),
            Style::default().fg(BRAND_DIM),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  ▸ ", Style::default().fg(BRAND_CYAN)),
            Span::styled(
                &displayed_key,
                Style::default().fg(BRAND_GREEN),
            ),
            Span::styled(cursor_char, Style::default().fg(BRAND_CYAN)),
        ]),
        Line::from(""),
    ];

    if let Some(err) = &state.api_key_error {
        lines.push(Line::from(Span::styled(
            format!("  ✗ {}", err),
            Style::default().fg(BRAND_RED),
        )));
        lines.push(Line::from(""));
    }

    lines.push(Line::from(Span::styled(
        format!(
            "  [Tab] {} key",
            if state.api_key_show { "Hide" } else { "Show" }
        ),
        Style::default().fg(BRAND_DIM),
    )));

    // Custom endpoint field
    if provider.kind == ProviderKind::Custom {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Endpoint URL:",
            Style::default().fg(Color::White).bold(),
        )));
        let ep_cursor = if state.editing_endpoint { "█" } else { "" };
        lines.push(Line::from(vec![
            Span::styled("  ▸ ", Style::default().fg(if state.editing_endpoint { BRAND_CYAN } else { BRAND_DIM })),
            Span::styled(
                if state.custom_endpoint.is_empty() {
                    "http://localhost:8080/v1"
                } else {
                    &state.custom_endpoint
                },
                Style::default().fg(if state.custom_endpoint.is_empty() && !state.editing_endpoint {
                    BRAND_DIM
                } else {
                    BRAND_GREEN
                }),
            ),
            Span::styled(ep_cursor, Style::default().fg(BRAND_CYAN)),
        ]));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BRAND_DIM))
        .title(Span::styled(
            " Authentication ",
            Style::default().fg(BRAND_CYAN).bold(),
        ))
        .style(Style::default().bg(SURFACE))
        .padding(Padding::horizontal(2));

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_model_select(frame: &mut Frame, state: &WizardState, area: Rect) {
    if state.models_loading {
        let lines = vec![
            Line::from(""),
            Line::from(""),
            Line::from(Span::styled(
                "  ◌ Loading models...",
                Style::default().fg(BRAND_CYAN),
            )),
        ];
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BRAND_DIM))
            .title(Span::styled(
                " Select Model ",
                Style::default().fg(BRAND_CYAN).bold(),
            ))
            .style(Style::default().bg(SURFACE));
        frame.render_widget(Paragraph::new(lines).block(block), area);
        return;
    }

    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header + filter
            Constraint::Min(0),   // model list
        ])
        .split(area);

    // Header with filter
    let filter_display = if state.model_filtering {
        format!("/{}", state.model_filter)
    } else if !state.model_filter.is_empty() {
        format!("filter: {} ", state.model_filter)
    } else {
        String::new()
    };

    let mut header_spans = vec![
        Span::styled(
            format!("  Choose a model ({})", state.current_provider().label),
            Style::default().fg(Color::White).bold(),
        ),
    ];
    if !filter_display.is_empty() {
        header_spans.push(Span::styled("  ", Style::default()));
        header_spans.push(Span::styled(
            filter_display,
            Style::default().fg(BRAND_YELLOW),
        ));
    }

    let mut header_lines = vec![
        Line::from(""),
        Line::from(header_spans),
    ];

    if let Some(err) = &state.models_error {
        header_lines.push(Line::from(Span::styled(
            format!("  ⚠ {} (showing popular models)", err),
            Style::default().fg(BRAND_YELLOW),
        )));
    }

    let header_block = Block::default()
        .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
        .border_style(Style::default().fg(BRAND_DIM))
        .title(Span::styled(
            " Select Model ",
            Style::default().fg(BRAND_CYAN).bold(),
        ))
        .style(Style::default().bg(SURFACE));

    frame.render_widget(
        Paragraph::new(header_lines).block(header_block),
        inner[0],
    );

    // Model list
    let filtered = state.filtered_models();
    let visible_height = inner[1].height.saturating_sub(2) as usize;
    let scroll = if state.model_cursor >= visible_height {
        state.model_cursor - visible_height + 1
    } else {
        0
    };

    let items: Vec<ListItem> = filtered
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible_height)
        .map(|(i, model)| {
            let is_selected = i == state.model_cursor;
            let marker = if is_selected { "▸" } else { " " };
            let marker_color = if is_selected { BRAND_CYAN } else { BRAND_DIM };

            let ctx_str = model
                .context_length
                .map(|c| {
                    if c >= 1_000_000 {
                        format!(" {}M ctx", c / 1_000_000)
                    } else if c >= 1_000 {
                        format!(" {}K ctx", c / 1_000)
                    } else {
                        format!(" {} ctx", c)
                    }
                })
                .unwrap_or_default();

            ListItem::new(Line::from(vec![
                Span::styled(format!(" {} ", marker), Style::default().fg(marker_color)),
                Span::styled(
                    &model.id,
                    if is_selected {
                        Style::default().fg(BRAND_CYAN).bold()
                    } else {
                        Style::default().fg(Color::White)
                    },
                ),
                Span::styled(ctx_str, Style::default().fg(BRAND_DIM)),
                Span::styled(
                    if model.description.is_empty() {
                        String::new()
                    } else {
                        format!("  {}", model.description)
                    },
                    Style::default().fg(Color::Rgb(70, 70, 80)),
                ),
            ]))
        })
        .collect();

    let model_block = Block::default()
        .borders(Borders::BOTTOM | Borders::LEFT | Borders::RIGHT)
        .border_style(Style::default().fg(BRAND_DIM))
        .style(Style::default().bg(SURFACE));

    let count_label = if filtered.len() != state.models.len() {
        format!("{}/{}", filtered.len(), state.models.len())
    } else {
        format!("{}", state.models.len())
    };

    let model_block = model_block.title_bottom(Line::from(Span::styled(
        format!(" {} models ", count_label),
        Style::default().fg(BRAND_DIM),
    )));

    frame.render_widget(List::new(items).block(model_block), inner[1]);
}

fn render_done(frame: &mut Frame, state: &WizardState, area: Rect) {
    let provider = state.current_provider();
    let filtered = state.filtered_models();
    let model_name = filtered
        .get(state.model_cursor)
        .map(|m| m.id.as_str())
        .unwrap_or("default");

    let path = state
        .saved_path
        .as_deref()
        .unwrap_or("~/.config/axon/config.toml");

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  ✓ Configuration saved!",
            Style::default().fg(BRAND_GREEN).bold(),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("    Provider:  ", Style::default().fg(BRAND_DIM)),
            Span::styled(provider.label, Style::default().fg(Color::White).bold()),
        ]),
        Line::from(vec![
            Span::styled("    Model:     ", Style::default().fg(BRAND_DIM)),
            Span::styled(model_name, Style::default().fg(ACCENT_BLUE).bold()),
        ]),
        Line::from(vec![
            Span::styled("    Config:    ", Style::default().fg(BRAND_DIM)),
            Span::styled(path, Style::default().fg(BRAND_DIM)),
        ]),
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            "  You're ready to go! Start your node with:",
            Style::default().fg(Color::White),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "    $ axon start",
            Style::default().fg(BRAND_CYAN).bold(),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Or explore more commands:",
            Style::default().fg(Color::White),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("    axon models    ", Style::default().fg(BRAND_CYAN)),
            Span::styled("Browse available models", Style::default().fg(BRAND_DIM)),
        ]),
        Line::from(vec![
            Span::styled("    axon auth      ", Style::default().fg(BRAND_CYAN)),
            Span::styled("Change provider or API key", Style::default().fg(BRAND_DIM)),
        ]),
        Line::from(vec![
            Span::styled("    axon serve-mcp ", Style::default().fg(BRAND_CYAN)),
            Span::styled("MCP server for AI tools", Style::default().fg(BRAND_DIM)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  Press Enter to exit.",
            Style::default().fg(BRAND_DIM),
        )),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BRAND_GREEN))
        .style(Style::default().bg(SURFACE))
        .padding(Padding::horizontal(2));

    frame.render_widget(
        Paragraph::new(lines).block(block).wrap(Wrap { trim: false }),
        area,
    );
}

fn render_help_bar(frame: &mut Frame, state: &WizardState, area: Rect) {
    let spans = match state.step {
        Step::Welcome => vec![
            Span::styled(" Enter", Style::default().fg(BRAND_CYAN).bold()),
            Span::styled(" start ", Style::default().fg(BRAND_DIM)),
            Span::styled("│", Style::default().fg(BRAND_DIM)),
            Span::styled(" q", Style::default().fg(BRAND_CYAN).bold()),
            Span::styled(" quit", Style::default().fg(BRAND_DIM)),
        ],
        Step::Provider => vec![
            Span::styled(" ↑/↓", Style::default().fg(BRAND_CYAN).bold()),
            Span::styled(" navigate ", Style::default().fg(BRAND_DIM)),
            Span::styled("│", Style::default().fg(BRAND_DIM)),
            Span::styled(" Enter", Style::default().fg(BRAND_CYAN).bold()),
            Span::styled(" select ", Style::default().fg(BRAND_DIM)),
            Span::styled("│", Style::default().fg(BRAND_DIM)),
            Span::styled(" Esc", Style::default().fg(BRAND_CYAN).bold()),
            Span::styled(" back", Style::default().fg(BRAND_DIM)),
        ],
        Step::ApiKey => vec![
            Span::styled(" Enter", Style::default().fg(BRAND_CYAN).bold()),
            Span::styled(" continue ", Style::default().fg(BRAND_DIM)),
            Span::styled("│", Style::default().fg(BRAND_DIM)),
            Span::styled(" Tab", Style::default().fg(BRAND_CYAN).bold()),
            Span::styled(" show/hide key ", Style::default().fg(BRAND_DIM)),
            Span::styled("│", Style::default().fg(BRAND_DIM)),
            Span::styled(" Esc", Style::default().fg(BRAND_CYAN).bold()),
            Span::styled(" back", Style::default().fg(BRAND_DIM)),
        ],
        Step::Models => {
            if state.model_filtering {
                vec![
                    Span::styled(" type", Style::default().fg(BRAND_CYAN).bold()),
                    Span::styled(" to filter ", Style::default().fg(BRAND_DIM)),
                    Span::styled("│", Style::default().fg(BRAND_DIM)),
                    Span::styled(" Enter/Esc", Style::default().fg(BRAND_CYAN).bold()),
                    Span::styled(" done filtering", Style::default().fg(BRAND_DIM)),
                ]
            } else {
                vec![
                    Span::styled(" ↑/↓", Style::default().fg(BRAND_CYAN).bold()),
                    Span::styled(" navigate ", Style::default().fg(BRAND_DIM)),
                    Span::styled("│", Style::default().fg(BRAND_DIM)),
                    Span::styled(" Enter", Style::default().fg(BRAND_CYAN).bold()),
                    Span::styled(" confirm ", Style::default().fg(BRAND_DIM)),
                    Span::styled("│", Style::default().fg(BRAND_DIM)),
                    Span::styled(" /", Style::default().fg(BRAND_CYAN).bold()),
                    Span::styled(" filter ", Style::default().fg(BRAND_DIM)),
                    Span::styled("│", Style::default().fg(BRAND_DIM)),
                    Span::styled(" Esc", Style::default().fg(BRAND_CYAN).bold()),
                    Span::styled(" back", Style::default().fg(BRAND_DIM)),
                ]
            }
        }
        Step::Done => vec![
            Span::styled(" Enter", Style::default().fg(BRAND_CYAN).bold()),
            Span::styled(" finish", Style::default().fg(BRAND_DIM)),
        ],
    };

    let bar =
        Paragraph::new(Line::from(spans)).style(Style::default().bg(SURFACE).fg(BRAND_DIM));
    frame.render_widget(bar, area);
}
