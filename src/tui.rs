//! Ratatui-based interactive profile editor.
//!
//! Two views:
//!   1. **Main** — three tabbed panels (Extensions / Skills / Prompts) with a
//!      detail sidebar. Space toggles selection. Enter opens configure view.
//!   2. **Configure** — form for an extension's config fields.

use crate::paths;
use crate::schema::{self, ConfigField, ConfigStatus, ExtensionManifest, FieldType};
use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::style::palette::tailwind;
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph};
use std::fs;

// ─── Data ───────────────────────────────────────────────────────────────

/// Represents one selectable item in a pool category.
#[derive(Debug, Clone)]
struct PoolItem {
    name: String,
    selected: bool,
    manifest: Option<ExtensionManifest>,
    config_status: ConfigStatus,
}

/// The full state for the TUI editor.
struct EditorState {
    profile_name: String,

    // Pool items by category
    extensions: Vec<PoolItem>,
    skills: Vec<PoolItem>,
    prompts: Vec<PoolItem>,
    mcp_servers: Vec<PoolItem>,
    models: Vec<PoolItem>,

    // Which panel is focused (0=extensions, 1=skills, 2=prompts, 3=mcp)
    active_panel: usize,
    // Cursor within the active panel
    cursor: usize,

    // Current view mode
    mode: ViewMode,

    // Configure form state (when in Configure mode)
    config_target: Option<String>,   // item name being configured
    config_fields: Vec<ConfigField>, // fields for the item being configured
    config_values: Vec<String>,      // current field values
    config_cursor: usize,            // which field is focused in configure form
    config_dirty: bool,              // whether config was modified

    // Whether anything changed since last save
    dirty: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum ViewMode {
    Main,
    Configure,
    Done,
}

// ─── Profile data loading ───────────────────────────────────────────────

/// Load editor state from a profile directory.
fn load_state(profile_name: &str, selected: &[String], subdir: &str) -> Vec<PoolItem> {
    let pool_dir = paths::pool_dir().join(subdir);
    let mut items = Vec::new();

    if let Ok(entries) = fs::read_dir(&pool_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy().to_string();

            // Skip files that aren't extension entry points
            if entry
                .file_type()
                .map_or(true, |t| !t.is_dir() && !t.is_file())
            {
                continue;
            }
            // Skip non-TS files
            if entry.file_type().is_ok_and(|t| t.is_file())
                && !name_str.to_ascii_lowercase().ends_with(".ts")
            {
                continue;
            }

            let manifest = ExtensionManifest::load(&entry.path());
            let config = load_item_config(profile_name, &name_str);
            let config_status = match &manifest {
                Some(m) => schema::evaluate_status(m, config.as_ref()),
                None => ConfigStatus::Ready,
            };

            items.push(PoolItem {
                name: name_str.clone(),
                selected: selected.contains(&name_str),
                manifest,
                config_status,
            });
        }
    }

    items.sort_by(|a, b| a.name.cmp(&b.name));
    items
}

/// Load MCP server items from pool/mcp/<name>/mcp.json
/// Each server is a directory with an mcp.json that may contain `config_fields`.
fn load_mcp_state(profile_name: &str, selected: &[String]) -> Vec<PoolItem> {
    let pool_dir = paths::pool_mcp_dir();
    let mut items = Vec::new();

    if let Ok(entries) = fs::read_dir(&pool_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy().to_string();

            // Only process directories
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }

            // Read the mcp.json from the pool entry to extract config_fields
            let mcp_path = entry.path().join("mcp.json");
            let (mcp_manifest, config_status) = if let Ok(content) = fs::read_to_string(&mcp_path) {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) {
                    let fields = value
                        .get("config_fields")
                        .and_then(|f| serde_json::from_value::<Vec<ConfigField>>(f.clone()).ok())
                        .unwrap_or_default();
                    let manifest = ExtensionManifest {
                        name: Some(name_str.clone()),
                        description: Some("MCP server".to_string()),
                        config_fields: fields,
                        checks: vec![],
                        tags: vec!["mcp".to_string()],
                    };
                    let config = load_item_config(profile_name, &name_str);
                    let status = schema::evaluate_status(&manifest, config.as_ref());
                    (Some(manifest), status)
                } else {
                    (None, ConfigStatus::Ready)
                }
            } else {
                (None, ConfigStatus::Ready)
            };

            items.push(PoolItem {
                name: name_str.clone(),
                selected: selected.contains(&name_str),
                manifest: mcp_manifest,
                config_status,
            });
        }
    }

    items.sort_by(|a, b| a.name.cmp(&b.name));
    items
}

/// Load model provider items from pool/models/<name>/model.json
/// Each entry is a directory with a `model.json` that may contain `config_fields`.
fn load_models_state(profile_name: &str, selected: &[String]) -> Vec<PoolItem> {
    let pool_dir = paths::pool_models_dir();
    let mut items = Vec::new();

    if let Ok(entries) = fs::read_dir(&pool_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy().to_string();

            // Only process directories
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }

            // Read the model.json from the pool entry to extract config_fields
            let model_path = entry.path().join("model.json");
            let (model_manifest, config_status) = if let Ok(content) =
                fs::read_to_string(&model_path)
            {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) {
                    let fields = value
                        .get("config_fields")
                        .and_then(|f| serde_json::from_value::<Vec<ConfigField>>(f.clone()).ok())
                        .unwrap_or_default();
                    let desc = value
                        .get("description")
                        .and_then(|d| d.as_str().map(String::from))
                        .unwrap_or_else(|| "Model provider".to_string());
                    let manifest = ExtensionManifest {
                        name: value
                            .get("name")
                            .and_then(|n| n.as_str().map(String::from))
                            .or_else(|| Some(name_str.clone())),
                        description: Some(desc),
                        config_fields: fields,
                        checks: vec![],
                        tags: vec!["model".to_string()],
                    };
                    let config = load_item_config(profile_name, &name_str);
                    let status = schema::evaluate_status(&manifest, config.as_ref());
                    (Some(manifest), status)
                } else {
                    (None, ConfigStatus::Ready)
                }
            } else {
                (None, ConfigStatus::Ready)
            };

            items.push(PoolItem {
                name: name_str.clone(),
                selected: selected.contains(&name_str),
                manifest: model_manifest,
                config_status,
            });
        }
    }

    items.sort_by(|a, b| a.name.cmp(&b.name));
    items
}

/// Read the profile's config file for a specific item, if it exists.
fn load_item_config(profile_name: &str, item_name: &str) -> Option<serde_json::Value> {
    let config_path = paths::profile_dir(profile_name)
        .join("config")
        .join(format!("{item_name}.json"));
    if config_path.exists() {
        fs::read_to_string(&config_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
    } else {
        None
    }
}

/// Read current config values for a specific item into the form state.
fn load_config_into_values(
    profile_name: &str,
    item_name: &str,
    fields: &[ConfigField],
) -> Vec<String> {
    let config = load_item_config(profile_name, item_name);
    let config_map = config.as_ref().and_then(|c| c.as_object());

    fields
        .iter()
        .map(|f| {
            // Try config file first
            if let Some(map) = config_map {
                if let Some(v) = map.get(&f.key) {
                    let s = match v {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    if !s.is_empty() {
                        return s;
                    }
                }
            }
            // Then env var
            if let Some(env_key) = &f.env_var {
                if let Ok(v) = std::env::var(env_key) {
                    if !v.is_empty() {
                        return v;
                    }
                }
            }
            // Then default
            f.default.clone().unwrap_or_default()
        })
        .collect()
}

/// Write config values to the profile's config file.
fn write_config_values(
    profile_name: &str,
    item_name: &str,
    fields: &[ConfigField],
    values: &[String],
) -> Result<()> {
    let config: serde_json::Map<String, serde_json::Value> = fields
        .iter()
        .zip(values.iter())
        .filter(|(f, v)| !v.is_empty() || f.default.is_some())
        .map(|(f, v)| {
            let value: serde_json::Value = match f.r#type {
                FieldType::Number => {
                    if let Ok(n) = v.parse::<f64>() {
                        serde_json::Value::Number(
                            serde_json::Number::from_f64(n).unwrap_or(0.into()),
                        )
                    } else {
                        serde_json::Value::String(v.clone())
                    }
                }
                FieldType::Boolean => serde_json::Value::Bool(
                    v.eq_ignore_ascii_case("true") || v == "1" || v == "yes",
                ),
                _ => serde_json::Value::String(v.clone()),
            };
            (f.key.clone(), value)
        })
        .collect();

    let config_dir = paths::profile_dir(profile_name).join("config");
    fs::create_dir_all(&config_dir).context("Failed to create config directory")?;

    let json = serde_json::to_string_pretty(&config).context("Failed to serialize config")?;
    fs::write(config_dir.join(format!("{item_name}.json")), &json)
        .with_context(|| format!("Failed to write config for '{item_name}'"))?;

    Ok(())
}

// ─── TUI Entry Point ────────────────────────────────────────────────────

pub fn run_editor(
    profile_name: &str,
    selected_exts: &[String],
    selected_skills: &[String],
    selected_prompts: &[String],
    selected_mcp: &[String],
    selected_models: &[String],
) -> Result<EditorResult> {
    enable_raw_mode()
        .context("Failed to enter raw terminal mode. The TUI editor requires an interactive terminal (TTY).")
        .context("Try running 'pim list' first, or use a real terminal.")?;
    let mut stderr = std::io::stderr();
    crossterm::execute!(stderr, EnterAlternateScreen)
        .context("Failed to enter alternate screen.")?;
    let backend = CrosstermBackend::new(stderr);
    let mut terminal = Terminal::new(backend)?;

    let mut state = EditorState {
        profile_name: profile_name.to_string(),
        extensions: load_state(profile_name, selected_exts, "extensions"),
        skills: load_state(profile_name, selected_skills, "skills"),
        prompts: load_state(profile_name, selected_prompts, "prompts"),
        mcp_servers: load_mcp_state(profile_name, selected_mcp),
        models: load_models_state(profile_name, selected_models),
        active_panel: 0,
        cursor: 0,
        mode: ViewMode::Main,
        config_target: None,
        config_fields: Vec::new(),
        config_values: Vec::new(),
        config_cursor: 0,
        config_dirty: false,
        dirty: false,
    };

    let result = loop {
        terminal.draw(|f| render_main(f, &state))?;

        if state.mode == ViewMode::Done {
            break build_result(&state);
        }

        let event = event::read()?;
        if handle_event(&mut state, &event)? {
            break build_result(&state);
        }
    };

    disable_raw_mode()?;
    crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(result)
}

/// Result returned by the editor.
#[derive(Debug, Default)]
pub struct EditorResult {
    pub selected_extensions: Vec<String>,
    pub selected_skills: Vec<String>,
    pub selected_prompts: Vec<String>,
    pub selected_mcp_servers: Vec<String>,
    pub selected_models: Vec<String>,
    pub changed: bool,
}

fn build_result(state: &EditorState) -> EditorResult {
    EditorResult {
        selected_extensions: selected_names(&state.extensions),
        selected_skills: selected_names(&state.skills),
        selected_prompts: selected_names(&state.prompts),
        selected_mcp_servers: selected_names(&state.mcp_servers),
        selected_models: selected_names(&state.models),
        changed: state.dirty,
    }
}

fn selected_names(items: &[PoolItem]) -> Vec<String> {
    items
        .iter()
        .filter(|i| i.selected)
        .map(|i| i.name.clone())
        .collect()
}

// ─── Event Handling ─────────────────────────────────────────────────────

/// Returns `true` if the caller should exit (saved or quit).
fn handle_event(state: &mut EditorState, event: &Event) -> Result<bool> {
    match state.mode {
        ViewMode::Main => Ok(handle_main_event(state, event)),
        ViewMode::Configure => handle_configure_event(state, event),
        ViewMode::Done => Ok(true),
    }
}

fn handle_main_event(state: &mut EditorState, event: &Event) -> bool {
    if let Event::Key(KeyEvent {
        code, modifiers, ..
    }) = event
    {
        match code {
            KeyCode::Char('q' | 's') if *modifiers == KeyModifiers::NONE => {
                state.mode = ViewMode::Done;
                return true;
            }
            KeyCode::Char(' ') => {
                toggle_current(state);
                state.dirty = true;
            }
            KeyCode::Enter => {
                open_configure(state);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                state.cursor = state.cursor.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let count = current_panel_len(state);
                if state.cursor + 1 < count {
                    state.cursor += 1;
                }
            }
            KeyCode::Left | KeyCode::Tab if *modifiers == KeyModifiers::SHIFT => {
                state.active_panel = (state.active_panel + 4) % 5;
                state.cursor = 0;
            }
            KeyCode::Right | KeyCode::Tab => {
                state.active_panel = (state.active_panel + 1) % 5;
                state.cursor = 0;
            }
            KeyCode::Home | KeyCode::Char('g') => {
                state.cursor = 0;
            }
            KeyCode::End | KeyCode::Char('G') => {
                let count = current_panel_len(state);
                state.cursor = count.saturating_sub(1);
            }
            _ => {}
        }
    }
    false
}

fn handle_configure_event(state: &mut EditorState, event: &Event) -> Result<bool> {
    if let Event::Key(KeyEvent {
        code, modifiers, ..
    }) = event
    {
        match code {
            KeyCode::Esc => {
                // Discard changes and go back
                state.mode = ViewMode::Main;
                state.config_target = None;
            }
            KeyCode::Enter | KeyCode::Char('s') if *modifiers == KeyModifiers::NONE => {
                // Save config and go back
                let target = state.config_target.clone();
                if let Some(ref target_name) = target {
                    let fields = state.config_fields.clone();
                    let values = state.config_values.clone();
                    let profile_name = state.profile_name.clone();
                    write_config_values(&profile_name, target_name, &fields, &values)?;
                    // Update the pool item's status — clone to avoid borrow conflict
                    let config = load_item_config(&profile_name, target_name);
                    if let Some(item) = find_pool_item_mut(state, target_name) {
                        if let Some(ref m) = item.manifest {
                            item.config_status = schema::evaluate_status(m, config.as_ref());
                        }
                    }
                    state.config_dirty = false;
                    state.dirty = true;
                }
                state.mode = ViewMode::Main;
                state.config_target = None;
            }
            KeyCode::Up | KeyCode::Char('k') if *modifiers == KeyModifiers::NONE => {
                state.config_cursor = state.config_cursor.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') if *modifiers == KeyModifiers::NONE => {
                if state.config_cursor + 1 < state.config_fields.len() {
                    state.config_cursor += 1;
                }
            }
            KeyCode::Tab => {
                if state.config_cursor + 1 < state.config_fields.len() {
                    state.config_cursor += 1;
                } else {
                    state.config_cursor = 0;
                }
            }
            KeyCode::Enter if *modifiers == KeyModifiers::NONE => {
                // Start editing the field value
                let idx = state.config_cursor;
                if idx < state.config_fields.len() {
                    // Use dialoguer for text input
                    let field = &state.config_fields[idx];
                    let current = state.config_values[idx].clone();
                    let prompt = field.label.as_deref().unwrap_or(&field.key);

                    let input = if field.r#type == FieldType::Password {
                        dialoguer::Password::new()
                            .with_prompt(prompt)
                            .allow_empty_password(true)
                            .interact()
                            .ok()
                    } else {
                        dialoguer::Input::<String>::new()
                            .with_prompt(prompt)
                            .allow_empty(true)
                            .with_initial_text(&current)
                            .interact()
                            .ok()
                    };

                    if let Some(value) = input {
                        if value != current {
                            state.config_values[idx] = value;
                            state.config_dirty = true;
                            state.dirty = true;
                        }
                    }

                    // Re-render immediately
                    state.config_cursor = idx;
                }
            }
            KeyCode::Char('r') if *modifiers == KeyModifiers::NONE => {
                // Reset this field to default
                let idx = state.config_cursor;
                if idx < state.config_fields.len() {
                    let default = state.config_fields[idx].default.clone().unwrap_or_default();
                    state.config_values[idx] = default;
                    state.config_dirty = true;
                    state.dirty = true;
                }
            }
            _ => {}
        }
    }
    Ok(false)
}

// ─── UI helpers for event handling ──────────────────────────────────────

fn toggle_current(state: &mut EditorState) {
    let idx = state.cursor;
    match state.active_panel {
        0 => {
            if idx < state.extensions.len() {
                state.extensions[idx].selected = !state.extensions[idx].selected;
            }
        }
        1 => {
            if idx < state.skills.len() {
                state.skills[idx].selected = !state.skills[idx].selected;
            }
        }
        2 => {
            if idx < state.prompts.len() {
                state.prompts[idx].selected = !state.prompts[idx].selected;
            }
        }
        3 if idx < state.mcp_servers.len() => {
            state.mcp_servers[idx].selected = !state.mcp_servers[idx].selected;
        }
        4 if idx < state.models.len() => {
            state.models[idx].selected = !state.models[idx].selected;
        }
        _ => {}
    }
}

fn open_configure(state: &mut EditorState) {
    let idx = state.cursor;
    let panel = state.active_panel;

    let item = match panel {
        0 => state.extensions.get(idx),
        1 => state.skills.get(idx),
        2 => state.prompts.get(idx),
        3 => state.mcp_servers.get(idx),
        4 => state.models.get(idx),
        _ => None,
    };

    let Some(item) = item else { return };

    let Some(manifest) = item.manifest.as_ref() else {
        return;
    };
    if manifest.config_fields.is_empty() {
        return;
    }

    let values = load_config_into_values(&state.profile_name, &item.name, &manifest.config_fields);

    state.config_target = Some(item.name.clone());
    state.config_fields = manifest.config_fields.clone();
    state.config_values = values;
    state.config_cursor = 0;
    state.config_dirty = false;
    state.mode = ViewMode::Configure;
}

#[allow(clippy::match_same_arms)]
fn current_panel(state: &EditorState) -> &[PoolItem] {
    match state.active_panel {
        0 => &state.extensions,
        1 => &state.skills,
        2 => &state.prompts,
        3 => &state.mcp_servers,
        4 => &state.models,
        _ => &state.extensions,
    }
}

fn current_panel_len(state: &EditorState) -> usize {
    current_panel(state).len()
}

fn find_pool_item_mut<'a>(state: &'a mut EditorState, name: &str) -> Option<&'a mut PoolItem> {
    // Check each panel separately to satisfy the borrow checker.
    if let Some(item) = state.extensions.iter_mut().find(|i| i.name == name) {
        return Some(item);
    }
    if let Some(item) = state.skills.iter_mut().find(|i| i.name == name) {
        return Some(item);
    }
    if let Some(item) = state.prompts.iter_mut().find(|i| i.name == name) {
        return Some(item);
    }
    if let Some(item) = state.mcp_servers.iter_mut().find(|i| i.name == name) {
        return Some(item);
    }
    if let Some(item) = state.models.iter_mut().find(|i| i.name == name) {
        return Some(item);
    }
    None
}

// ─── Rendering ──────────────────────────────────────────────────────────

fn render_main(f: &mut Frame, state: &EditorState) {
    match state.mode {
        ViewMode::Main => render_main_view(f, state),
        ViewMode::Configure => render_configure_view(f, state),
        ViewMode::Done => {}
    }
}

fn render_main_view(f: &mut Frame, state: &EditorState) {
    let area = f.area();
    let title = format!("✏️  Editing profile: {}", state.profile_name);

    // Layout: left panels (2/3) + right detail panel (1/3)
    let main_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(2, 3), Constraint::Ratio(1, 3)])
        .split(area);

    // Left side: four stacked panels
    let left_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
        ])
        .margin(1)
        .split(main_layout[0]);

    // Render each panel
    render_panel(
        f,
        left_layout[0],
        state,
        0,
        "Extensions",
        &state.extensions,
        tailwind::BLUE.c600,
    );
    render_panel(
        f,
        left_layout[1],
        state,
        1,
        "Skills",
        &state.skills,
        tailwind::EMERALD.c600,
    );
    render_panel(
        f,
        left_layout[2],
        state,
        2,
        "Prompts",
        &state.prompts,
        tailwind::AMBER.c600,
    );
    render_panel(
        f,
        left_layout[3],
        state,
        3,
        "MCP Servers",
        &state.mcp_servers,
        tailwind::VIOLET.c600,
    );
    render_panel(
        f,
        left_layout[4],
        state,
        4,
        "Models",
        &state.models,
        tailwind::CYAN.c600,
    );

    // Right side: detail panel
    let title_block = Block::default()
        .title(title)
        .title_alignment(Alignment::Center)
        .borders(Borders::TOP)
        .border_type(BorderType::Plain);

    render_detail_panel(f, main_layout[1], state);

    // Bottom help bar
    let help_bar = Paragraph::new(Span::styled(
        " [↑↓/jk] Navigate  [←→/Tab] Switch panel  [Space] Toggle  [Enter] Configure  [s] Save & exit  [q] Quit",
        Style::default().fg(Color::DarkGray),
    ));
    let help_area = Rect {
        x: area.x,
        y: area.height.saturating_sub(1),
        width: area.width,
        height: 1,
    };
    f.render_widget(help_bar, help_area);
    // Draw title on top
    f.render_widget(title_block, area);
}

fn render_panel(
    f: &mut Frame,
    area: Rect,
    state: &EditorState,
    panel_idx: usize,
    label: &str,
    items: &[PoolItem],
    accent: Color,
) {
    let is_active = state.active_panel == panel_idx;
    let border_style = if is_active {
        Style::default().fg(accent)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let selected_count = items.iter().filter(|i| i.selected).count();
    let title = format!(" {label} ({selected_count}/{}) ", items.len());

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style);

    let _inner = block.inner(area);

    let list_items: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            let is_cursor = is_active && state.cursor == idx;
            let check = if item.selected { "◉" } else { "○" };
            let status = match &item.manifest {
                Some(_) => {
                    let label = schema::status_label(&item.config_status);
                    format!(" {label}")
                }
                None => String::new(),
            };
            let display = item
                .manifest
                .as_ref()
                .and_then(|m| m.name.as_deref())
                .unwrap_or(&item.name);

            let content = if is_cursor {
                format!("▸ {check} {display}{status}")
            } else {
                format!("  {check} {display}{status}")
            };

            let style = if is_cursor {
                Style::default().fg(Color::White).bg(accent)
            } else if item.selected {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::Gray)
            };

            ListItem::new(content).style(style)
        })
        .collect();

    // Calculate scroll offset to keep cursor visible
    let visible_height = (area.height.saturating_sub(2)).max(1) as usize;
    let scroll_offset = if is_active && state.cursor >= visible_height {
        state.cursor - visible_height + 1
    } else {
        0
    };

    let mut list_state = ListState::default().with_offset(scroll_offset);

    let list = List::new(list_items).block(block);
    f.render_stateful_widget(list, area, &mut list_state);
}

fn render_detail_panel(f: &mut Frame, area: Rect, state: &EditorState) {
    let items = current_panel(state);
    let block = Block::default()
        .title(" Details ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let _inner = block.inner(area);

    if state.cursor >= items.len() {
        let empty = Paragraph::new("No item selected")
            .block(block)
            .style(Style::default().fg(Color::Gray));
        f.render_widget(empty, area);
        return;
    }

    let item = &items[state.cursor];
    let _display = item
        .manifest
        .as_ref()
        .and_then(|m| m.name.as_deref())
        .unwrap_or(&item.name);

    let mut lines = Vec::new();

    // Name
    lines.push(Line::from(Span::styled(
        &item.name,
        Style::default().fg(Color::White).bold(),
    )));

    // Description
    if let Some(ref m) = item.manifest {
        if let Some(ref desc) = m.description {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                desc,
                Style::default().fg(Color::Cyan),
            )));
        }

        // Config fields
        if !m.config_fields.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Config:",
                Style::default().fg(Color::Yellow).bold(),
            )));
            for field in &m.config_fields {
                let label = field.label.as_deref().unwrap_or(&field.key);
                let req = if field.required { " (req)" } else { "" };
                let env = field
                    .env_var
                    .as_ref()
                    .map_or(String::new(), |e| format!(" env:{e}"));
                let status_icon = if is_field_filled(item, &field.key) {
                    "✓"
                } else if field.required {
                    "✗"
                } else {
                    "○"
                };
                let line = format!("  {status_icon} {label}{req}{env}");
                lines.push(Line::from(Span::raw(line)));
            }
        }

        // Tags
        if !m.tags.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("Tags: {}", m.tags.join(", ")),
                Style::default().fg(Color::DarkGray),
            )));
        }
    } else {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "No extension.json manifest",
            Style::default().fg(Color::DarkGray),
        )));
    }

    // Config hint
    if item
        .manifest
        .as_ref()
        .is_some_and(|m| !m.config_fields.is_empty())
    {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "───  [Enter] Configure ───",
            Style::default().fg(Color::DarkGray),
        )));
    }

    let detail = Paragraph::new(lines).block(block);
    f.render_widget(detail, area);
}

fn is_field_filled(item: &PoolItem, key: &str) -> bool {
    match &item.manifest {
        Some(m) => {
            let field = m.config_fields.iter().find(|f| f.key == key);
            match field {
                Some(f) => f.default.is_some(),
                None => false,
            }
        }
        None => false,
    }
}

fn render_configure_view(f: &mut Frame, state: &EditorState) {
    let area = f.area();

    let target = state.config_target.as_deref().unwrap_or("unknown");
    let title = format!(" Configure: {target} ");

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(tailwind::VIOLET.c500));

    let inner = block.inner(area);

    // Layout: title + fields + help
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // spacer
            Constraint::Min(2),    // header
            Constraint::Length(1), // fields
        ])
        .split(inner);

    // Header
    let header = Paragraph::new(Span::styled(
        "Use ↑/↓ to select a field, Enter to edit, Esc to go back, s to save",
        Style::default().fg(Color::DarkGray),
    ));
    f.render_widget(header, chunks[0]);

    // Fields list
    let field_items: Vec<ListItem> = state
        .config_fields
        .iter()
        .enumerate()
        .map(|(idx, field)| {
            let is_cursor = state.config_cursor == idx;
            let value = &state.config_values[idx];
            let display_value: Span = if field.r#type == FieldType::Password && !value.is_empty() {
                Span::raw("█".repeat(value.len().min(40)))
            } else if value.is_empty() {
                Span::styled("<not set>", Style::default().fg(Color::DarkGray))
            } else {
                Span::raw(value.clone())
            };

            let label = field.label.as_deref().unwrap_or(&field.key);
            let req = if field.required { " *" } else { "" };
            let env = field
                .env_var
                .as_ref()
                .map_or(String::new(), |e| format!(" (${e})"));
            let help = field
                .help
                .as_ref()
                .map_or(String::new(), |h| format!(" — {h}"));

            let content = if is_cursor {
                format!("▸ {label}{req}{env}{help}")
            } else {
                format!("  {label}{req}{env}{help}")
            };

            let style = if is_cursor {
                Style::default().fg(Color::White).bg(tailwind::VIOLET.c700)
            } else if field.required && value.is_empty() {
                Style::default().fg(tailwind::RED.c400)
            } else {
                Style::default().fg(Color::White)
            };

            // Show the current value on a sub-line
            let value_line = format!("    → {display_value}");
            let value_style = if is_cursor {
                Style::default().fg(Color::Gray)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            ListItem::new(vec![
                Line::from(Span::styled(content, style)),
                Line::from(Span::styled(value_line, value_style)),
            ])
        })
        .collect();

    let field_list = List::new(field_items);
    f.render_widget(field_list, chunks[1]);

    // Help bar at bottom
    let help_text = if state.config_dirty {
        " [↑↓] Select field  [Enter] Edit  [r] Reset to default  [s] Save config  [Esc] Cancel"
    } else {
        " [↑↓] Select field  [Enter] Edit  [r] Reset to default  [s] Save & back  [Esc] Discard"
    };
    let help = Paragraph::new(Span::styled(
        help_text,
        Style::default().fg(Color::DarkGray),
    ));
    let help_area = Rect {
        x: area.x,
        y: area.height.saturating_sub(1),
        width: area.width,
        height: 1,
    };
    f.render_widget(help, help_area);
    f.render_widget(block, area);
}
