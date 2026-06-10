use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};
use regex::Regex;
use std::{
    cmp::min,
    collections::{BTreeMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};
use steamworks::{AppId, Client};

#[derive(Clone, Debug)]
struct AchievementState {
    name: String,
    achieved: bool,
}

#[derive(Clone, Debug)]
struct GameEntry {
    app_id: u32,
    name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum QueueOperation {
    SetAchievement {
        app_id: u32,
        achievement: String,
        unlock: bool,
    },
    ResetAll {
        app_id: u32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FocusPane {
    Games,
    Achievements,
    Queue,
}

struct TuiState {
    games: Vec<GameEntry>,
    games_selected: usize,
    achievements: Vec<AchievementState>,
    achievements_selected: usize,
    queue: Vec<QueueOperation>,
    queue_selected: usize,
    focus: FocusPane,
    loaded_app_id: Option<u32>,
    status: String,
}

impl TuiState {
    fn new(games: Vec<GameEntry>) -> Self {
        Self {
            games,
            games_selected: 0,
            achievements: Vec::new(),
            achievements_selected: 0,
            queue: Vec::new(),
            queue_selected: 0,
            focus: FocusPane::Games,
            loaded_app_id: None,
            status: "Press <Enter> on a game to load achievements. q to quit.".to_string(),
        }
    }

    fn selected_game(&self) -> Option<&GameEntry> {
        self.games.get(self.games_selected)
    }

    fn selected_achievement(&self) -> Option<&AchievementState> {
        self.achievements.get(self.achievements_selected)
    }

    fn queued_state_for(&self, app_id: u32, achievement: &str) -> Option<bool> {
        self.queue.iter().rev().find_map(|op| match op {
            QueueOperation::SetAchievement {
                app_id: op_app,
                achievement: op_achievement,
                unlock,
            } if *op_app == app_id && op_achievement == achievement => Some(*unlock),
            _ => None,
        })
    }

    fn queue_set_achievement(&mut self, app_id: u32, achievement: &str, unlock: bool) {
        self.queue.retain(|op| {
            !matches!(
                op,
                QueueOperation::SetAchievement {
                    app_id: op_app,
                    achievement: op_achievement,
                    ..
                } if *op_app == app_id && op_achievement == achievement
            )
        });

        let original = self
            .achievements
            .iter()
            .find(|a| a.name == achievement)
            .map(|a| a.achieved)
            .unwrap_or(false);

        if unlock != original {
            self.queue.push(QueueOperation::SetAchievement {
                app_id,
                achievement: achievement.to_string(),
                unlock,
            });
        }

        self.queue_selected = min(self.queue_selected, self.queue.len().saturating_sub(1));
    }

    fn queue_reset(&mut self, app_id: u32) {
        self.queue.push(QueueOperation::ResetAll { app_id });
        self.queue_selected = self.queue.len().saturating_sub(1);
    }

    fn remove_selected_queue_item(&mut self) {
        if self.queue.is_empty() {
            return;
        }
        self.queue.remove(self.queue_selected);
        self.queue_selected = min(self.queue_selected, self.queue.len().saturating_sub(1));
    }

    fn move_focus_left(&mut self) {
        self.focus = match self.focus {
            FocusPane::Games => FocusPane::Games,
            FocusPane::Achievements => FocusPane::Games,
            FocusPane::Queue => FocusPane::Achievements,
        };
    }

    fn move_focus_right(&mut self) {
        self.focus = match self.focus {
            FocusPane::Games => FocusPane::Achievements,
            FocusPane::Achievements => FocusPane::Queue,
            FocusPane::Queue => FocusPane::Queue,
        };
    }

    fn move_selection_down(&mut self) {
        match self.focus {
            FocusPane::Games => {
                if self.games_selected + 1 < self.games.len() {
                    self.games_selected += 1;
                }
            }
            FocusPane::Achievements => {
                if self.achievements_selected + 1 < self.achievements.len() {
                    self.achievements_selected += 1;
                }
            }
            FocusPane::Queue => {
                if self.queue_selected + 1 < self.queue.len() {
                    self.queue_selected += 1;
                }
            }
        }
    }

    fn move_selection_up(&mut self) {
        match self.focus {
            FocusPane::Games => self.games_selected = self.games_selected.saturating_sub(1),
            FocusPane::Achievements => {
                self.achievements_selected = self.achievements_selected.saturating_sub(1)
            }
            FocusPane::Queue => self.queue_selected = self.queue_selected.saturating_sub(1),
        }
    }

    fn jump_top(&mut self) {
        match self.focus {
            FocusPane::Games => self.games_selected = 0,
            FocusPane::Achievements => self.achievements_selected = 0,
            FocusPane::Queue => self.queue_selected = 0,
        }
    }

    fn jump_bottom(&mut self) {
        match self.focus {
            FocusPane::Games => {
                self.games_selected = self.games.len().saturating_sub(1);
            }
            FocusPane::Achievements => {
                self.achievements_selected = self.achievements.len().saturating_sub(1);
            }
            FocusPane::Queue => {
                self.queue_selected = self.queue.len().saturating_sub(1);
            }
        }
    }
}

/// Steam Achievement Manager – CLI + TUI
#[derive(Parser)]
#[command(name = "sam", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Launch ratatui interface
    Tui,
    /// List all achievements for a game
    List {
        /// Steam App ID
        app_id: u32,
    },
    /// Unlock one or more achievements
    Unlock {
        /// Steam App ID
        app_id: u32,
        /// Achievement API name(s) to unlock
        #[arg(required = true)]
        achievements: Vec<String>,
    },
    /// Lock (re-lock) one or more achievements
    Lock {
        /// Steam App ID
        app_id: u32,
        /// Achievement API name(s) to lock
        #[arg(required = true)]
        achievements: Vec<String>,
    },
    /// Reset all achievements and stats for a game
    Reset {
        /// Steam App ID
        app_id: u32,
    },
}

fn init_client(app_id: u32) -> Result<Client> {
    Client::init_app(AppId(app_id))
        .context("Failed to initialize Steam client. Is Steam running and are you logged in?")
}

fn run_callbacks(client: &Client) {
    for _ in 0..50 {
        client.run_callbacks();
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn load_achievements(app_id: u32) -> Result<Vec<AchievementState>> {
    let client = init_client(app_id)?;
    let user_stats = client.user_stats();
    run_callbacks(&client);

    let names = user_stats.get_achievement_names().unwrap_or_default();
    let mut out = Vec::with_capacity(names.len());
    for name in names {
        let achieved = user_stats.achievement(&name).get().unwrap_or(false);
        out.push(AchievementState { name, achieved });
    }
    Ok(out)
}

fn apply_achievement_changes(app_id: u32, unlocks: &[String], locks: &[String]) -> Result<()> {
    if unlocks.is_empty() && locks.is_empty() {
        return Ok(());
    }

    let client = init_client(app_id)?;
    let user_stats = client.user_stats();
    run_callbacks(&client);

    for achievement in unlocks {
        user_stats
            .achievement(achievement)
            .set()
            .map_err(|()| anyhow!("Failed to unlock achievement: {achievement}"))?;
    }

    for achievement in locks {
        user_stats
            .achievement(achievement)
            .clear()
            .map_err(|()| anyhow!("Failed to lock achievement: {achievement}"))?;
    }

    user_stats
        .store_stats()
        .map_err(|()| anyhow!("Failed to store stats"))?;
    run_callbacks(&client);

    Ok(())
}

fn reset_all(app_id: u32) -> Result<()> {
    let client = init_client(app_id)?;
    let user_stats = client.user_stats();
    run_callbacks(&client);

    user_stats
        .reset_all_stats(true)
        .map_err(|()| anyhow!("Failed to reset stats/achievements"))?;
    user_stats
        .store_stats()
        .map_err(|()| anyhow!("Failed to store stats"))?;
    run_callbacks(&client);
    Ok(())
}

fn cmd_list(app_id: u32) -> Result<()> {
    let achievements = load_achievements(app_id)?;
    if achievements.is_empty() {
        println!("No achievements found for app {app_id}.");
        return Ok(());
    }

    println!("Achievements for app {app_id}:");
    println!("{:<50} STATUS", "NAME");
    println!("{}", "-".repeat(60));

    for item in achievements {
        let status = if item.achieved {
            "✓ Unlocked"
        } else {
            "✗ Locked"
        };
        println!("{:<50} {status}", item.name);
    }

    Ok(())
}

fn cmd_unlock(app_id: u32, achievements: &[String]) -> Result<()> {
    apply_achievement_changes(app_id, achievements, &[])?;
    println!("Unlocked {} achievement(s).", achievements.len());
    Ok(())
}

fn cmd_lock(app_id: u32, achievements: &[String]) -> Result<()> {
    apply_achievement_changes(app_id, &[], achievements)?;
    println!("Locked {} achievement(s).", achievements.len());
    Ok(())
}

fn cmd_reset(app_id: u32) -> Result<()> {
    reset_all(app_id)?;
    println!("Reset all achievements and stats for app {app_id}.");
    Ok(())
}

fn discover_games() -> Vec<GameEntry> {
    let mut all_games = BTreeMap::<u32, String>::new();
    for path in discover_steam_libraries() {
        for game in discover_games_in_library(&path) {
            all_games.entry(game.app_id).or_insert(game.name);
        }
    }

    all_games
        .into_iter()
        .map(|(app_id, name)| GameEntry { app_id, name })
        .collect()
}

fn discover_steam_libraries() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Ok(home) = std::env::var("HOME") {
        roots.push(PathBuf::from(&home).join(".steam/steam"));
        roots.push(PathBuf::from(home).join(".local/share/Steam"));
    }

    if let Ok(profile) = std::env::var("USERPROFILE") {
        roots.push(PathBuf::from(&profile).join("AppData/Local/Steam"));
    }

    if let Ok(pfx86) = std::env::var("PROGRAMFILES(X86)") {
        roots.push(PathBuf::from(pfx86).join("Steam"));
    }

    let mut libraries = HashSet::<PathBuf>::new();
    for root in roots {
        let steamapps = root.join("steamapps");
        if !steamapps.exists() {
            continue;
        }

        libraries.insert(root.clone());

        let library_folders = steamapps.join("libraryfolders.vdf");
        if library_folders.exists() {
            for path in parse_library_paths(&library_folders) {
                libraries.insert(path);
            }
        }
    }

    let mut out: Vec<_> = libraries.into_iter().collect();
    out.sort();
    out
}

fn parse_library_paths(path: &Path) -> Vec<PathBuf> {
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };

    let path_re = Regex::new(r#""path"\s*"([^"]+)""#).expect("regex compile");
    path_re
        .captures_iter(&content)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().replace("\\\\", "\\")))
        .map(PathBuf::from)
        .collect()
}

fn discover_games_in_library(library_root: &Path) -> Vec<GameEntry> {
    let steamapps = library_root.join("steamapps");
    let Ok(entries) = fs::read_dir(&steamapps) else {
        return Vec::new();
    };

    let mut games = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        if !file_name.starts_with("appmanifest_") || !file_name.ends_with(".acf") {
            continue;
        }

        if let Some(game) = parse_app_manifest(&path) {
            games.push(game);
        }
    }

    games
}

fn parse_app_manifest(path: &Path) -> Option<GameEntry> {
    let content = fs::read_to_string(path).ok()?;
    let app_id_re = Regex::new(r#""appid"\s*"(\d+)""#).ok()?;
    let name_re = Regex::new(r#""name"\s*"([^"]+)""#).ok()?;

    let app_id = app_id_re
        .captures(&content)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<u32>().ok())?;

    let name = name_re
        .captures(&content)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| format!("App {app_id}"));

    Some(GameEntry { app_id, name })
}

fn run_tui() -> Result<()> {
    let games = discover_games();
    let mut state = TuiState::new(games);

    enable_raw_mode().context("Failed to enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("Failed to enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("Failed to initialize terminal")?;

    let result = tui_event_loop(&mut terminal, &mut state);

    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
    terminal.show_cursor().ok();

    result
}

fn tui_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut TuiState,
) -> Result<()> {
    loop {
        terminal.draw(|frame| draw_ui(frame, state))?;

        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
        {
            if key.kind != KeyEventKind::Press {
                continue;
            }

            match key.code {
                KeyCode::Char('q') => return Ok(()),
                KeyCode::Char('h') => state.move_focus_left(),
                KeyCode::Char('l') => state.move_focus_right(),
                KeyCode::Char('j') | KeyCode::Down => state.move_selection_down(),
                KeyCode::Char('k') | KeyCode::Up => state.move_selection_up(),
                KeyCode::Char('g') => state.jump_top(),
                KeyCode::Char('G') => state.jump_bottom(),
                KeyCode::Enter => {
                    if state.focus == FocusPane::Games
                        && let Some(game) = state.selected_game().cloned()
                    {
                        state.loaded_app_id = Some(game.app_id);
                        match load_achievements(game.app_id) {
                            Ok(items) => {
                                state.achievements = items;
                                state.achievements_selected = 0;
                                state.focus = FocusPane::Achievements;
                                state.status = format!(
                                    "Loaded {} achievement(s) for {} ({})",
                                    state.achievements.len(),
                                    game.name,
                                    game.app_id
                                );
                            }
                            Err(err) => {
                                state.status = format!(
                                    "Failed to load achievements for {} ({}): {err}",
                                    game.name, game.app_id
                                );
                            }
                        }
                    }
                }
                KeyCode::Char(' ') => {
                    if state.focus == FocusPane::Achievements
                        && let (Some(game), Some(achievement)) = (
                            state.selected_game().cloned(),
                            state.selected_achievement().cloned(),
                        )
                    {
                        let current = state
                            .queued_state_for(game.app_id, &achievement.name)
                            .unwrap_or(achievement.achieved);
                        state.queue_set_achievement(game.app_id, &achievement.name, !current);
                        state.status = format!(
                            "Queued {} {}",
                            if !current { "unlock" } else { "lock" },
                            achievement.name
                        );
                    }
                }
                KeyCode::Char('u') => {
                    if state.focus == FocusPane::Achievements
                        && let (Some(game), Some(achievement)) = (
                            state.selected_game().cloned(),
                            state.selected_achievement().cloned(),
                        )
                    {
                        state.queue_set_achievement(game.app_id, &achievement.name, true);
                        state.status = format!("Queued unlock {}", achievement.name);
                    }
                }
                KeyCode::Char('x') => {
                    if state.focus == FocusPane::Achievements
                        && let (Some(game), Some(achievement)) = (
                            state.selected_game().cloned(),
                            state.selected_achievement().cloned(),
                        )
                    {
                        state.queue_set_achievement(game.app_id, &achievement.name, false);
                        state.status = format!("Queued lock {}", achievement.name);
                    } else if state.focus == FocusPane::Queue {
                        state.remove_selected_queue_item();
                    }
                }
                KeyCode::Char('A') => {
                    if let Some(game) = state.selected_game().cloned() {
                        let names: Vec<String> =
                            state.achievements.iter().map(|a| a.name.clone()).collect();
                        for name in names {
                            state.queue_set_achievement(game.app_id, &name, true);
                        }
                        state.status = format!("Queued unlock-all for app {}", game.app_id);
                    }
                }
                KeyCode::Char('X') => {
                    if let Some(game) = state.selected_game().cloned() {
                        let names: Vec<String> =
                            state.achievements.iter().map(|a| a.name.clone()).collect();
                        for name in names {
                            state.queue_set_achievement(game.app_id, &name, false);
                        }
                        state.status = format!("Queued lock-all for app {}", game.app_id);
                    }
                }
                KeyCode::Char('r') => {
                    if let Some(game) = state.selected_game().cloned() {
                        state.queue_reset(game.app_id);
                        state.status = format!("Queued reset-all for app {}", game.app_id);
                    }
                }
                KeyCode::Char('d') | KeyCode::Delete | KeyCode::Backspace
                    if state.focus == FocusPane::Queue =>
                {
                    state.remove_selected_queue_item();
                }
                KeyCode::Char('c') => {
                    let op_count = state.queue.len();
                    match commit_queue(state) {
                        Ok(changed_apps) => {
                            state.status = format!(
                                "Committed {op_count} queued operation(s) across {changed_apps} app(s)."
                            );
                            if let Some(app_id) = state.loaded_app_id
                                && let Ok(items) = load_achievements(app_id)
                            {
                                state.achievements = items;
                                state.achievements_selected = 0;
                            }
                        }
                        Err(err) => {
                            state.status = format!("Commit failed: {err}");
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

fn commit_queue(state: &mut TuiState) -> Result<usize> {
    if state.queue.is_empty() {
        return Ok(0);
    }

    let mut per_app_unlocks: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    let mut per_app_locks: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    let mut resets: Vec<u32> = Vec::new();

    for op in &state.queue {
        match op {
            QueueOperation::SetAchievement {
                app_id,
                achievement,
                unlock,
            } => {
                if *unlock {
                    per_app_unlocks
                        .entry(*app_id)
                        .or_default()
                        .push(achievement.clone());
                } else {
                    per_app_locks
                        .entry(*app_id)
                        .or_default()
                        .push(achievement.clone());
                }
            }
            QueueOperation::ResetAll { app_id } => resets.push(*app_id),
        }
    }

    let mut changed = HashSet::<u32>::new();

    for app_id in resets {
        reset_all(app_id)?;
        changed.insert(app_id);
    }

    let app_ids: HashSet<u32> = per_app_unlocks
        .keys()
        .chain(per_app_locks.keys())
        .copied()
        .collect();

    for app_id in app_ids {
        let unlocks = per_app_unlocks.remove(&app_id).unwrap_or_default();
        let locks = per_app_locks.remove(&app_id).unwrap_or_default();
        apply_achievement_changes(app_id, &unlocks, &locks)?;
        changed.insert(app_id);
    }

    state.queue.clear();
    state.queue_selected = 0;

    Ok(changed.len())
}

fn draw_ui(frame: &mut ratatui::Frame<'_>, state: &TuiState) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(3)])
        .split(frame.area());

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(30),
            Constraint::Percentage(40),
            Constraint::Percentage(30),
        ])
        .split(root[0]);

    draw_games_pane(frame, state, columns[0]);
    draw_achievements_pane(frame, state, columns[1]);
    draw_queue_pane(frame, state, columns[2]);

    let footer = Paragraph::new(Line::from(vec![
        "vim: h/l focus • j/k move • g/G top/bottom • Enter load • space toggle • u unlock • x lock/remove • A unlock-all • X lock-all • r reset-all • c commit • q quit".into(),
    ]))
    .style(Style::default().fg(Color::DarkGray))
    .block(Block::default().title(state.status.as_str()).borders(Borders::ALL));

    frame.render_widget(Clear, root[1]);
    frame.render_widget(footer, root[1]);
}

fn pane_style(is_focused: bool) -> Style {
    if is_focused {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    }
}

fn draw_games_pane(frame: &mut ratatui::Frame<'_>, state: &TuiState, area: ratatui::layout::Rect) {
    let items: Vec<ListItem<'_>> = if state.games.is_empty() {
        vec![ListItem::new("No installed games found")]
    } else {
        state
            .games
            .iter()
            .map(|g| ListItem::new(format!("{} ({})", g.name, g.app_id)))
            .collect()
    };

    let mut list_state = ratatui::widgets::ListState::default();
    if !state.games.is_empty() {
        list_state.select(Some(state.games_selected));
    }

    let list = List::new(items)
        .block(
            Block::default()
                .title("Games")
                .title_style(pane_style(state.focus == FocusPane::Games))
                .borders(Borders::ALL),
        )
        .highlight_style(Style::default().bg(Color::Blue).fg(Color::White))
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(list, area, &mut list_state);
}

fn draw_achievements_pane(
    frame: &mut ratatui::Frame<'_>,
    state: &TuiState,
    area: ratatui::layout::Rect,
) {
    let app_id = state
        .loaded_app_id
        .or_else(|| state.selected_game().map(|g| g.app_id));

    let items: Vec<ListItem<'_>> = if state.achievements.is_empty() {
        vec![ListItem::new("Load a game with <Enter>")]
    } else {
        state
            .achievements
            .iter()
            .map(|a| {
                let effective = app_id
                    .and_then(|id| state.queued_state_for(id, &a.name))
                    .unwrap_or(a.achieved);
                let icon = if effective { "✓" } else { "✗" };
                let pending = if app_id
                    .and_then(|id| state.queued_state_for(id, &a.name))
                    .is_some()
                {
                    " *"
                } else {
                    ""
                };
                ListItem::new(format!("[{icon}] {}{pending}", a.name))
            })
            .collect()
    };

    let mut list_state = ratatui::widgets::ListState::default();
    if !state.achievements.is_empty() {
        list_state.select(Some(state.achievements_selected));
    }

    let list = List::new(items)
        .block(
            Block::default()
                .title("Achievements")
                .title_style(pane_style(state.focus == FocusPane::Achievements))
                .borders(Borders::ALL),
        )
        .highlight_style(Style::default().bg(Color::Blue).fg(Color::White))
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(list, area, &mut list_state);
}

fn draw_queue_pane(frame: &mut ratatui::Frame<'_>, state: &TuiState, area: ratatui::layout::Rect) {
    let items: Vec<ListItem<'_>> = if state.queue.is_empty() {
        vec![ListItem::new("Queue is empty")]
    } else {
        state
            .queue
            .iter()
            .map(|op| match op {
                QueueOperation::SetAchievement {
                    app_id,
                    achievement,
                    unlock,
                } => ListItem::new(format!(
                    "{} {} ({app_id})",
                    if *unlock { "unlock" } else { "lock" },
                    achievement
                )),
                QueueOperation::ResetAll { app_id } => {
                    ListItem::new(format!("reset all stats+achievements ({app_id})"))
                }
            })
            .collect()
    };

    let mut list_state = ratatui::widgets::ListState::default();
    if !state.queue.is_empty() {
        list_state.select(Some(state.queue_selected));
    }

    let list = List::new(items)
        .block(
            Block::default()
                .title("Operation Queue")
                .title_style(pane_style(state.focus == FocusPane::Queue))
                .borders(Borders::ALL),
        )
        .highlight_style(Style::default().bg(Color::Blue).fg(Color::White))
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(list, area, &mut list_state);
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Tui => run_tui(),
        Command::List { app_id } => cmd_list(app_id),
        Command::Unlock {
            app_id,
            achievements,
        } => cmd_unlock(app_id, &achievements),
        Command::Lock {
            app_id,
            achievements,
        } => cmd_lock(app_id, &achievements),
        Command::Reset { app_id } => cmd_reset(app_id),
    }
}
