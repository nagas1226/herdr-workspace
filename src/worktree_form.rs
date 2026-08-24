//! Centered ratatui form: branch → base ref (fuzzy) → profile cards → cancel/save.

use crate::apply;
use crate::config::{self, Profile};
use crate::form::{self, CARD_HEIGHT, CARD_WIDTH};
use crate::git;
use crate::popup;
use crate::theme::Theme;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use std::io::{self, stdout, Stdout};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Branch,
    Base,
    Profile,
    Cancel,
    Save,
}

pub fn run() -> i32 {
    popup::run(run_inner)
}

fn run_inner(agent_starts: &mut Vec<Vec<String>>) -> Result<i32, String> {
    let cfg = config::load().map_err(|e| e.message)?;
    let source = Source::from_env()?;
    let mut app = App::new(cfg.profiles, Theme::load(), source)?;

    enable_raw_mode().map_err(|e| e.to_string())?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture).map_err(|e| e.to_string())?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|e| e.to_string())?;
    let result = app_loop(&mut terminal, &mut app, agent_starts);
    let _ = disable_raw_mode();
    let mut out = io::stdout();
    let _ = execute!(out, DisableMouseCapture, LeaveAlternateScreen);
    result
}

fn app_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    agent_starts: &mut Vec<Vec<String>>,
) -> Result<i32, String> {
    loop {
        terminal.draw(|f| app.draw(f)).map_err(|e| e.to_string())?;
        if event::poll(Duration::from_millis(200)).map_err(io_err)? {
            match event::read().map_err(io_err)? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if let Some(code) = app.on_key(key.code, key.modifiers) {
                        return Ok(code);
                    }
                }
                Event::Mouse(mouse) => {
                    if let Some(code) = app.on_mouse(mouse) {
                        return Ok(code);
                    }
                }
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
        app.poll_job(agent_starts);
        if app.done {
            return Ok(app.exit_code);
        }
    }
}

fn io_err(e: io::Error) -> String {
    e.to_string()
}

struct Source {
    workspace_id: String,
    cwd: String,
}

impl Source {
    fn from_env() -> Result<Self, String> {
        let workspace_id = std::env::var("WORKTREE_FORM_WORKSPACE_ID")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| {
                std::env::var("HERDR_WORKSPACE_ID")
                    .ok()
                    .filter(|s| !s.is_empty())
            })
            .ok_or_else(|| "no current workspace; open a git workspace first".to_string())?;
        let mut cwd = std::env::var("WORKTREE_FORM_CWD").unwrap_or_default();
        if cwd.is_empty() {
            cwd = std::env::var("HERDR_WORKSPACE_CWD").unwrap_or_default();
        }
        if cwd.is_empty() {
            return Err("workspace has no directory".into());
        }
        Ok(Self { workspace_id, cwd })
    }
}

struct App {
    theme: Theme,
    profiles: Vec<Profile>,
    source: Source,
    refs: Vec<String>,
    branch: String,
    base: String,
    suggestions: Vec<String>,
    suggest_idx: usize,
    branch_committed: bool,
    base_committed: bool,
    profile_idx: usize,
    focus: Focus,
    error: String,
    job: Option<apply::SaveJob>,
    done: bool,
    exit_code: i32,
    branch_box: Rect,
    base_box: Rect,
    card_boxes: Vec<Rect>,
    cancel_box: Rect,
    save_box: Rect,
    card_cols: usize,
}

impl App {
    fn new(profiles: Vec<Profile>, theme: Theme, source: Source) -> Result<Self, String> {
        let repo = git::repo_root(&source.cwd)?;
        let refs = git::list_refs(&repo)?;
        let base = git::default_base(&refs);
        let mut app = Self {
            theme,
            profiles,
            source,
            refs,
            branch: String::new(),
            base: base.clone(),
            suggestions: Vec::new(),
            suggest_idx: 0,
            branch_committed: false,
            base_committed: !base.is_empty(),
            profile_idx: 0,
            focus: Focus::Branch,
            error: String::new(),
            job: None,
            done: false,
            exit_code: 0,
            branch_box: Rect::default(),
            base_box: Rect::default(),
            card_boxes: Vec::new(),
            cancel_box: Rect::default(),
            save_box: Rect::default(),
            card_cols: 1,
        };
        app.refresh_suggestions();
        Ok(app)
    }

    fn refresh_suggestions(&mut self) {
        self.suggestions = git::suggestions(&self.base, &self.refs);
        if self.suggest_idx >= self.suggestions.len() {
            self.suggest_idx = 0;
        }
    }

    fn on_key(&mut self, code: KeyCode, mods: KeyModifiers) -> Option<i32> {
        if self.job.is_some() {
            return None;
        }
        if code == KeyCode::Esc {
            return Some(0);
        }
        match self.focus {
            Focus::Branch => self.key_branch(code, mods),
            Focus::Base => self.key_base(code, mods),
            Focus::Profile => self.key_profile(code),
            Focus::Cancel => self.key_cancel(code),
            Focus::Save => self.key_save(code),
        }
    }

    fn key_branch(&mut self, code: KeyCode, mods: KeyModifiers) -> Option<i32> {
        match code {
            KeyCode::Char(c) if !mods.contains(KeyModifiers::CONTROL) => {
                self.branch.push(c);
                self.branch_committed = false;
                self.error.clear();
            }
            KeyCode::Backspace => {
                self.branch.pop();
                self.branch_committed = false;
                self.error.clear();
            }
            KeyCode::Enter | KeyCode::Tab | KeyCode::Down => self.commit_branch(),
            _ => {}
        }
        None
    }

    fn key_base(&mut self, code: KeyCode, mods: KeyModifiers) -> Option<i32> {
        match code {
            KeyCode::Char('j') if mods.contains(KeyModifiers::CONTROL) => self.move_suggest(1),
            KeyCode::Char('k') if mods.contains(KeyModifiers::CONTROL) => self.move_suggest(-1),
            KeyCode::Char('n') if mods.contains(KeyModifiers::CONTROL) => self.move_suggest(1),
            KeyCode::Char('p') if mods.contains(KeyModifiers::CONTROL) => self.move_suggest(-1),
            KeyCode::Char(c) if !mods.contains(KeyModifiers::CONTROL) => {
                self.base.push(c);
                self.base_committed = false;
                self.error.clear();
                self.refresh_suggestions();
            }
            KeyCode::Backspace => {
                self.base.pop();
                self.base_committed = false;
                self.error.clear();
                self.refresh_suggestions();
            }
            KeyCode::Down => self.move_suggest(1),
            KeyCode::Up => {
                if self.suggestions.is_empty() || self.suggest_idx == 0 {
                    self.focus = Focus::Branch;
                } else {
                    self.move_suggest(-1);
                }
            }
            KeyCode::Tab => {
                if let Some(s) = self.suggestions.get(self.suggest_idx).cloned() {
                    self.base = s;
                    self.refresh_suggestions();
                }
            }
            KeyCode::Enter => {
                if let Some(s) = self.suggestions.get(self.suggest_idx).cloned() {
                    self.apply_base(&s);
                } else {
                    self.commit_base();
                }
            }
            KeyCode::BackTab => self.focus = Focus::Branch,
            _ => {}
        }
        None
    }

    fn move_suggest(&mut self, delta: i32) {
        if self.suggestions.is_empty() {
            if delta > 0 && self.base_committed {
                self.focus = Focus::Profile;
            }
            return;
        }
        let n = self.suggestions.len() as i32;
        let next = (self.suggest_idx as i32 + delta).rem_euclid(n);
        self.suggest_idx = next as usize;
    }

    fn key_profile(&mut self, code: KeyCode) -> Option<i32> {
        let n = self.profiles.len();
        if n == 0 {
            return None;
        }
        let cols = self.card_cols.max(1);
        match code {
            KeyCode::Left | KeyCode::BackTab => {
                self.profile_idx = if self.profile_idx == 0 {
                    n - 1
                } else {
                    self.profile_idx - 1
                };
            }
            KeyCode::Right | KeyCode::Tab => {
                self.profile_idx = (self.profile_idx + 1) % n;
            }
            KeyCode::Up => {
                if self.profile_idx < cols {
                    self.focus = Focus::Base;
                } else {
                    self.profile_idx -= cols;
                }
            }
            KeyCode::Down => {
                if self.profile_idx + cols >= n {
                    self.focus = Focus::Save;
                } else {
                    self.profile_idx += cols;
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => self.focus = Focus::Save,
            _ => {}
        }
        None
    }

    fn key_cancel(&mut self, code: KeyCode) -> Option<i32> {
        match code {
            KeyCode::Enter | KeyCode::Char(' ') => Some(0),
            KeyCode::Right | KeyCode::Tab => {
                self.focus = Focus::Save;
                None
            }
            KeyCode::Up => {
                self.focus = if self.profiles.is_empty() {
                    Focus::Base
                } else {
                    Focus::Profile
                };
                None
            }
            _ => None,
        }
    }

    fn key_save(&mut self, code: KeyCode) -> Option<i32> {
        match code {
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.save();
                None
            }
            KeyCode::Left | KeyCode::BackTab => {
                self.focus = Focus::Cancel;
                None
            }
            KeyCode::Up => {
                self.focus = if self.profiles.is_empty() {
                    Focus::Base
                } else {
                    Focus::Profile
                };
                None
            }
            _ => None,
        }
    }

    fn commit_branch(&mut self) {
        if self.branch.trim().is_empty() {
            self.error = "branch name is empty".into();
            return;
        }
        if let Err(e) = git::valid_branch_name(&self.branch) {
            self.error = e;
            return;
        }
        self.branch = self.branch.trim().to_string();
        self.branch_committed = true;
        self.error.clear();
        self.focus = Focus::Base;
    }

    fn commit_base(&mut self) {
        if let Some(s) = self.suggestions.get(self.suggest_idx).cloned() {
            self.apply_base(&s);
            return;
        }
        self.apply_base(&self.base.clone());
    }

    fn apply_base(&mut self, value: &str) {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            self.error = "base ref is empty".into();
            return;
        }
        if !self.refs.iter().any(|r| r == trimmed) {
            self.error = format!("unknown ref `{trimmed}`");
            return;
        }
        self.base = trimmed.to_string();
        self.base_committed = true;
        self.error.clear();
        self.refresh_suggestions();
        if self.profiles.is_empty() {
            self.error = "no profiles in config.yaml".into();
            return;
        }
        self.focus = Focus::Profile;
    }

    fn save(&mut self) {
        if !self.branch_committed {
            self.commit_branch();
        }
        if !self.base_committed {
            self.commit_base();
        }
        if !self.branch_committed || !self.base_committed {
            return;
        }
        let Some(profile) = self.profiles.get(self.profile_idx).cloned() else {
            self.error = "select a profile".into();
            return;
        };
        let workspace_id = self.source.workspace_id.clone();
        let cwd = self.source.cwd.clone();
        let branch = self.branch.trim().to_string();
        let base = self.base.trim().to_string();
        self.error.clear();
        self.job = Some(apply::SaveJob::spawn(move |on_progress| {
            apply::apply_worktree_with_progress(
                &mut apply::HerdrCli,
                &workspace_id,
                &cwd,
                &branch,
                &base,
                &profile,
                on_progress,
            )
        }));
    }

    fn poll_job(&mut self, agent_starts: &mut Vec<Vec<String>>) {
        let Some(job) = self.job.as_mut() else {
            return;
        };
        job.poll();
        if let Some(result) = job.take_done() {
            self.job = None;
            match result {
                Ok(r) => {
                    agent_starts.extend(r.agent_starts);
                    self.done = true;
                    self.exit_code = 0;
                }
                Err(e) => self.error = e,
            }
        }
    }

    fn on_mouse(&mut self, mouse: MouseEvent) -> Option<i32> {
        if self.job.is_some() {
            return None;
        }
        let pos = Position::new(mouse.column, mouse.row);
        if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
            return None;
        }
        if self.branch_box.contains(pos) {
            self.focus = Focus::Branch;
        } else if self.base_box.contains(pos) {
            self.focus = Focus::Base;
        } else {
            for (i, r) in self.card_boxes.iter().enumerate() {
                if r.contains(pos) {
                    self.profile_idx = i;
                    self.focus = Focus::Profile;
                }
            }
        }
        if self.cancel_box.contains(pos) {
            return Some(0);
        }
        if self.save_box.contains(pos) {
            self.save();
        }
        None
    }

    fn show_suggestions(&self) -> bool {
        !self.base_committed
    }

    fn draw(&mut self, f: &mut Frame) {
        let area = f.area();
        if let Some(job) = self.job.as_ref() {
            form::draw_progress(f, area, &self.theme, job.progress());
            return;
        }
        f.render_widget(Clear, area);
        f.render_widget(Block::default().style(self.theme.base()), area);

        let inner = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(area);
        let body = form::pad(inner[0], 1, 2);
        f.render_widget(
            Paragraph::new(Span::styled(
                "  ctrl+j/k select ref · tab complete · enter next · esc cancel",
                self.theme.muted(),
            )),
            inner[1],
        );

        let suggest_h = if self.show_suggestions() { 4 } else { 0 };
        let cards_h = form::cards_area_height(body.width, self.profiles.len());
        let chunks = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(suggest_h),
            Constraint::Length(1),
            Constraint::Length(cards_h),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(body);

        self.branch_box = chunks[0];
        self.draw_input(
            f,
            chunks[0],
            "Branch",
            &self.branch,
            self.focus == Focus::Branch,
            "feature/my-work",
        );

        self.base_box = chunks[2];
        self.draw_input(
            f,
            chunks[2],
            "Base ref",
            &self.base,
            self.focus == Focus::Base,
            "origin/main",
        );
        if self.show_suggestions() {
            self.draw_suggestions(f, chunks[3]);
        }
        self.draw_cards(f, chunks[5]);

        if !self.error.is_empty() {
            f.render_widget(
                Paragraph::new(Span::styled(self.error.as_str(), self.theme.error()))
                    .wrap(Wrap { trim: true }),
                chunks[6],
            );
        }

        self.draw_buttons(f, chunks[7]);

        match self.focus {
            Focus::Branch => f.set_cursor_position(form::cursor_in_input(
                self.branch_box,
                self.branch.chars().count(),
            )),
            Focus::Base => f.set_cursor_position(form::cursor_in_input(
                self.base_box,
                self.base.chars().count(),
            )),
            _ => {}
        }
    }

    fn draw_input(
        &self,
        f: &mut Frame,
        area: Rect,
        title: &str,
        value: &str,
        focused: bool,
        placeholder: &str,
    ) {
        let block = Block::bordered()
            .borders(Borders::ALL)
            .title(format!(" {title} "))
            .border_style(self.theme.input_border(focused))
            .style(self.theme.base());
        let inner = block.inner(area);
        f.render_widget(block, area);
        let (text, style) = if value.is_empty() && !placeholder.is_empty() {
            (placeholder, self.theme.muted())
        } else {
            (value, self.theme.input(focused))
        };
        f.render_widget(
            Paragraph::new(Span::styled(format!(" {text}"), style)),
            inner,
        );
    }

    fn draw_suggestions(&self, f: &mut Frame, area: Rect) {
        if self.suggestions.is_empty() {
            f.render_widget(
                Paragraph::new(Span::styled("  no matching refs", self.theme.muted())),
                area,
            );
            return;
        }
        let lines: Vec<Line> = self
            .suggestions
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let selected = self.focus == Focus::Base && i == self.suggest_idx;
                Line::from(Span::styled(format!("  {s}"), self.theme.suggest(selected)))
            })
            .collect();
        f.render_widget(Paragraph::new(lines), area);
    }

    fn draw_cards(&mut self, f: &mut Frame, area: Rect) {
        let n = self.profiles.len();
        if n == 0 {
            self.card_boxes.clear();
            return;
        }
        let cols = form::card_columns(area.width, n);
        self.card_cols = cols;
        let rows = n.div_ceil(cols);
        let row_constraints: Vec<Constraint> =
            (0..rows).map(|_| Constraint::Length(CARD_HEIGHT)).collect();
        let row_rects = Layout::vertical(row_constraints).spacing(1).split(area);

        self.card_boxes = Vec::with_capacity(n);
        for (row_i, row) in row_rects.iter().enumerate() {
            let start = row_i * cols;
            let count = (n - start).min(cols);
            let mut constraints = Vec::with_capacity(count + 1);
            for _ in 0..count {
                constraints.push(Constraint::Length(CARD_WIDTH));
            }
            constraints.push(Constraint::Fill(1));
            let cells = Layout::horizontal(constraints).spacing(1).split(*row);
            for col_i in 0..count {
                let i = start + col_i;
                let profile = &self.profiles[i];
                let cell = cells[col_i];
                self.card_boxes.push(cell);
                let focused = self.focus == Focus::Profile && i == self.profile_idx;
                let selected = i == self.profile_idx;
                let block = Block::bordered()
                    .borders(Borders::ALL)
                    .title(format!(" {} ", profile.name))
                    .border_style(self.theme.card_border(focused, selected))
                    .style(self.theme.base());
                let body: Vec<Line> = profile
                    .card_lines()
                    .into_iter()
                    .map(|l| Line::from(Span::styled(l, self.theme.muted())))
                    .collect();
                f.render_widget(
                    Paragraph::new(body).block(block).wrap(Wrap { trim: true }),
                    cell,
                );
            }
        }
    }

    fn draw_buttons(&mut self, f: &mut Frame, area: Rect) {
        let row = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Length(12),
            Constraint::Length(1),
            Constraint::Length(12),
        ])
        .split(area);
        self.cancel_box = row[1];
        self.save_box = row[3];
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "[ Cancel ]",
                self.theme.button(self.focus == Focus::Cancel, false),
            )))
            .centered(),
            row[1],
        );
        let save_ok = self.branch_committed && self.base_committed && !self.profiles.is_empty();
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "[ Save ]",
                self.theme.button(self.focus == Focus::Save, save_ok),
            )))
            .centered(),
            row[3],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let run = |args: &[&str]| {
            assert!(Command::new(args[0])
                .args(&args[1..])
                .current_dir(dir.path())
                .status()
                .unwrap()
                .success());
        };
        run(&["git", "init", "-q", "-b", "main"]);
        run(&["git", "config", "user.email", "t@t.t"]);
        run(&["git", "config", "user.name", "t"]);
        std::fs::write(dir.path().join("a"), "a").unwrap();
        run(&["git", "add", "a"]);
        run(&["git", "commit", "-q", "-m", "init"]);
        run(&["git", "branch", "feature/login"]);
        dir
    }

    fn app_for(dir: &tempfile::TempDir) -> App {
        let profiles = config::parse_yaml(config::EXAMPLE_YAML).unwrap().profiles;
        App::new(
            profiles,
            Theme::load(),
            Source {
                workspace_id: "w1".into(),
                cwd: dir.path().to_string_lossy().into_owned(),
            },
        )
        .unwrap()
    }

    #[test]
    fn defaults_base_to_main() {
        let repo = git_repo();
        let app = app_for(&repo);
        assert_eq!(app.base, "main");
        assert!(app.base_committed);
        assert_eq!(app.focus, Focus::Branch);
    }

    #[test]
    fn commit_branch_then_base_unlocks_profiles() {
        let repo = git_repo();
        let mut app = app_for(&repo);
        app.branch = "wt/demo".into();
        app.commit_branch();
        assert!(app.branch_committed);
        assert_eq!(app.focus, Focus::Base);
        app.base = "flog".into();
        app.base_committed = false;
        app.refresh_suggestions();
        assert!(
            app.suggestions.iter().any(|s| s == "feature/login"),
            "{:?}",
            app.suggestions
        );
        app.suggest_idx = app
            .suggestions
            .iter()
            .position(|s| s == "feature/login")
            .unwrap();
        app.commit_base();
        assert_eq!(app.base, "feature/login");
        assert!(app.base_committed);
        assert_eq!(app.focus, Focus::Profile);
    }

    #[test]
    fn empty_branch_is_rejected() {
        let repo = git_repo();
        let mut app = app_for(&repo);
        app.commit_branch();
        assert!(!app.branch_committed);
        assert!(app.error.contains("empty"));
    }

    #[test]
    fn unknown_base_is_rejected() {
        let repo = git_repo();
        let mut app = app_for(&repo);
        app.suggestions.clear();
        app.apply_base("no-such-ref");
        assert!(!app.base_committed || app.base != "no-such-ref");
        assert!(app.error.contains("unknown ref"));
    }
}
