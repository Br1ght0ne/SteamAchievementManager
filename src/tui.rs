use crate::{
    common::{AchievementState, GameEntry, StatValue, parse_stat_assignment},
    steam,
};
use anyhow::{Context, Result};
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
use std::{
    cmp::min,
    collections::{BTreeMap, HashSet},
    io,
    time::Duration,
};

#[derive(Clone, Debug, PartialEq)]
enum QueueOperation {
    SetAchievement {
        app_id: u32,
        achievement: String,
        unlock: bool,
    },
    ResetAll {
        app_id: u32,
    },
    SetStat {
        app_id: u32,
        stat: String,
        value: StatValue,
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
    input_mode: bool,
    input_buffer: String,
    show_help: bool,
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
            input_mode: false,
            input_buffer: String::new(),
            show_help: false,
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

    fn queue_set_stat(&mut self, app_id: u32, stat: &str, value: StatValue) {
        self.queue.retain(|op| {
            !matches!(
                op,
                QueueOperation::SetStat {
                    app_id: op_app,
                    stat: op_stat,
                    ..
                } if *op_app == app_id && op_stat == stat
            )
        });
        self.queue.push(QueueOperation::SetStat {
            app_id,
            stat: stat.to_string(),
            value,
        });
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

pub(crate) fn run_tui() -> Result<()> {
    let games = steam::discover_games();
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

            if state.show_help {
                match key.code {
                    KeyCode::Char('?') | KeyCode::Esc | KeyCode::Enter => {
                        state.show_help = false;
                    }
                    _ => {}
                }
                continue;
            }

            if state.input_mode {
                match key.code {
                    KeyCode::Esc => {
                        state.input_mode = false;
                        state.input_buffer.clear();
                        state.status = "Cancelled stat input.".to_string();
                    }
                    KeyCode::Backspace => {
                        state.input_buffer.pop();
                    }
                    KeyCode::Enter => {
                        if let Some(game) = state.selected_game().cloned() {
                            match parse_stat_assignment(&state.input_buffer) {
                                Ok((stat, value)) => {
                                    state.queue_set_stat(game.app_id, &stat, value);
                                    state.status = format!(
                                        "Queued stat update for app {}: {}",
                                        game.app_id, state.input_buffer
                                    );
                                }
                                Err(err) => {
                                    state.status = format!("Invalid stat input: {err}");
                                }
                            }
                        }
                        state.input_mode = false;
                        state.input_buffer.clear();
                    }
                    KeyCode::Char(ch) => state.input_buffer.push(ch),
                    _ => {}
                }
                continue;
            }

            match key.code {
                KeyCode::Char('q') => return Ok(()),
                KeyCode::Char('?') => {
                    state.show_help = true;
                }
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
                        match steam::load_achievements(game.app_id) {
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
                KeyCode::Char('s') if state.selected_game().is_some() => {
                    state.input_mode = true;
                    state.input_buffer.clear();
                    state.status =
                        "Stat input mode: type STAT_NAME=value then press Enter.".to_string();
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
                                && let Ok(items) = steam::load_achievements(app_id)
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
    let mut per_app_stats: BTreeMap<u32, Vec<(String, StatValue)>> = BTreeMap::new();
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
            QueueOperation::SetStat {
                app_id,
                stat,
                value,
            } => {
                per_app_stats
                    .entry(*app_id)
                    .or_default()
                    .push((stat.clone(), value.clone()));
            }
        }
    }

    let mut changed = HashSet::<u32>::new();

    for app_id in resets {
        steam::reset_all(app_id)?;
        changed.insert(app_id);
    }

    let app_ids: HashSet<u32> = per_app_unlocks
        .keys()
        .chain(per_app_locks.keys())
        .chain(per_app_stats.keys())
        .copied()
        .collect();

    for app_id in app_ids {
        let unlocks = per_app_unlocks.remove(&app_id).unwrap_or_default();
        let locks = per_app_locks.remove(&app_id).unwrap_or_default();
        let stats = per_app_stats.remove(&app_id).unwrap_or_default();
        steam::apply_achievement_changes(app_id, &unlocks, &locks)?;
        steam::apply_stat_changes(app_id, &stats)?;
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
        "vim: h/l focus • j/k move • g/G top/bottom • Enter load • space toggle • u unlock • x lock/remove • s add-stat (STAT=value) • A unlock-all • X lock-all • r reset-all • c commit • ? help • q quit".into(),
    ]))
    .style(Style::default().fg(Color::DarkGray))
    .block(Block::default().title(state.status.as_str()).borders(Borders::ALL));

    frame.render_widget(Clear, root[1]);
    frame.render_widget(footer, root[1]);

    if state.input_mode {
        let popup = centered_rect(70, 20, frame.area());
        frame.render_widget(Clear, popup);
        let modal = Paragraph::new(format!(
            "Set stat for selected game:\n{}\n",
            state.input_buffer
        ))
        .block(
            Block::default()
                .title("STAT INPUT (Esc to cancel)")
                .borders(Borders::ALL),
        )
        .style(Style::default().fg(Color::White).bg(Color::Black));
        frame.render_widget(modal, popup);
    }

    if state.show_help {
        let popup = centered_rect(80, 70, frame.area());
        frame.render_widget(Clear, popup);
        let help_text = Paragraph::new(
            "SAM TUI help\n\n\
Global:\n\
  q = quit\n\
  ? = toggle this help popup\n\n\
Navigation (vim):\n\
  h/l = focus previous/next pane\n\
  j/k = move selection down/up\n\
  g/G = jump to top/bottom\n\n\
Games pane:\n\
  Enter = load selected game's achievements\n\n\
Achievements pane:\n\
  space = toggle lock/unlock (queued)\n\
  u = queue unlock selected achievement\n\
  x = queue lock selected achievement\n\
  A = queue unlock-all\n\
  X = queue lock-all\n\
  r = queue reset-all stats+achievements\n\
  s = queue stat update (STAT_NAME=value)\n\n\
Queue pane:\n\
  x, d, Delete, Backspace = remove selected queued operation\n\
  c = commit all queued operations\n\n\
In stat input mode:\n\
  Enter = queue stat change\n\
  Esc = cancel\n",
        )
        .block(
            Block::default()
                .title("Help (press ?, Esc, or Enter to close)")
                .borders(Borders::ALL),
        )
        .style(Style::default().fg(Color::White).bg(Color::Black));
        frame.render_widget(help_text, popup);
    }
}

fn centered_rect(
    percent_x: u16,
    percent_y: u16,
    area: ratatui::layout::Rect,
) -> ratatui::layout::Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
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
                QueueOperation::SetStat {
                    app_id,
                    stat,
                    value,
                } => match value {
                    StatValue::Int(v) => ListItem::new(format!("set stat {stat}={v} ({app_id})")),
                    StatValue::Float(v) => {
                        ListItem::new(format!("set stat {stat}={v:.3} ({app_id})"))
                    }
                },
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
