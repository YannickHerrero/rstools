pub mod ui;

use std::path::PathBuf;
use std::process::Command;

use crossterm::event::KeyEvent;
use ratatui::{layout::Rect, widgets::ListState, Frame};
use rstools_core::{
    help_popup::HelpEntry,
    keybinds::{process_normal_key, Action, InputMode, KeyState},
    telescope::TelescopeItem,
    tool::Tool,
    which_key::WhichKeyEntry,
};
use rusqlite::Connection;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKind {
    Text,
    Binary,
}

#[derive(Debug, Clone)]
pub struct ConflictFile {
    pub path: String,
    pub status: String,
    pub kind: ConflictKind,
}

pub struct MergeTool {
    mode: InputMode,
    key_state: KeyState,
    repo_root: Option<PathBuf>,
    files: Vec<ConflictFile>,
    list_state: ListState,
    sidebar_focused: bool,
    active_file: Option<String>,
}

impl MergeTool {
    pub fn new(_conn: Connection) -> anyhow::Result<Self> {
        let mut tool = Self {
            mode: InputMode::Normal,
            key_state: KeyState::default(),
            repo_root: None,
            files: Vec::new(),
            list_state: ListState::default(),
            sidebar_focused: true,
            active_file: None,
        };
        tool.refresh_conflicts();
        Ok(tool)
    }

    fn refresh_conflicts(&mut self) {
        if self.repo_root.is_none() {
            self.repo_root = Self::detect_repo_root();
        }

        self.files = self
            .repo_root
            .as_ref()
            .map(Self::list_conflicted_files)
            .unwrap_or_default();

        if self.files.is_empty() {
            self.list_state.select(None);
            self.active_file = None;
            return;
        }

        let selected = self.list_state.selected().unwrap_or(0);
        let clamped = selected.min(self.files.len().saturating_sub(1));
        self.list_state.select(Some(clamped));

        if self.active_file.is_none() {
            self.active_file = self.files.get(clamped).map(|f| f.path.clone());
        }
    }

    fn detect_repo_root() -> Option<PathBuf> {
        let output = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let path = String::from_utf8(output.stdout).ok()?;
        let trimmed = path.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(PathBuf::from(trimmed))
        }
    }

    fn list_conflicted_files(repo_root: &PathBuf) -> Vec<ConflictFile> {
        let output = Command::new("git")
            .arg("status")
            .arg("--porcelain")
            .current_dir(repo_root)
            .output();

        let Ok(output) = output else {
            return Vec::new();
        };

        if !output.status.success() {
            return Vec::new();
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut files = Vec::new();

        for line in stdout.lines() {
            if line.len() < 4 {
                continue;
            }

            let status = &line[..2];
            if !is_unmerged_status(status) {
                continue;
            }

            let path = line[3..].trim();
            if path.is_empty() {
                continue;
            }

            let kind = match std::fs::read_to_string(repo_root.join(path)) {
                Ok(_) => ConflictKind::Text,
                Err(_) => ConflictKind::Binary,
            };

            files.push(ConflictFile {
                path: path.to_string(),
                status: status.to_string(),
                kind,
            });
        }

        files
    }

    fn selected_index(&self) -> Option<usize> {
        self.list_state
            .selected()
            .filter(|_| !self.files.is_empty())
    }

    fn selected_file(&self) -> Option<&ConflictFile> {
        self.selected_index().and_then(|idx| self.files.get(idx))
    }

    fn select_active_from_sidebar(&mut self) {
        if let Some(file) = self.selected_file() {
            self.active_file = Some(file.path.clone());
            self.sidebar_focused = false;
        }
    }
}

fn is_unmerged_status(status: &str) -> bool {
    matches!(status, "DD" | "AU" | "UD" | "UA" | "DU" | "AA" | "UU")
}

impl Tool for MergeTool {
    fn name(&self) -> &str {
        "Merge"
    }

    fn description(&self) -> &str {
        "Resolve git merge conflicts"
    }

    fn mode(&self) -> InputMode {
        self.mode
    }

    fn init_db(&self, _conn: &Connection) -> anyhow::Result<()> {
        Ok(())
    }

    fn which_key_entries(&self) -> Vec<WhichKeyEntry> {
        vec![WhichKeyEntry::action("r", "Refresh conflicts")]
    }

    fn telescope_items(&self) -> Vec<TelescopeItem> {
        self.files
            .iter()
            .map(|f| TelescopeItem {
                label: f.path.clone(),
                description: "merge conflict".to_string(),
                id: format!("merge:{}", f.path),
            })
            .collect()
    }

    fn handle_telescope_selection(&mut self, id: &str) -> bool {
        let Some(path) = id.strip_prefix("merge:") else {
            return false;
        };

        if let Some(idx) = self.files.iter().position(|f| f.path == path) {
            self.list_state.select(Some(idx));
            self.active_file = Some(path.to_string());
            self.sidebar_focused = false;
            return true;
        }

        false
    }

    fn help_entries(&self) -> Vec<HelpEntry> {
        vec![
            HelpEntry::with_section("Merge", "j / k", "Navigate conflicted files"),
            HelpEntry::with_section("Merge", "Enter", "Open selected conflicted file"),
            HelpEntry::with_section("Merge", "<Space>r", "Refresh conflicted files"),
        ]
    }

    fn handle_key(&mut self, key: KeyEvent) -> Action {
        let action = process_normal_key(key, &mut self.key_state);
        match action {
            Action::MoveDown(step) => {
                if !self.files.is_empty() {
                    let cur = self.list_state.selected().unwrap_or(0);
                    let next = (cur + step).min(self.files.len().saturating_sub(1));
                    self.list_state.select(Some(next));
                }
                Action::None
            }
            Action::MoveUp(step) => {
                if !self.files.is_empty() {
                    let cur = self.list_state.selected().unwrap_or(0);
                    self.list_state.select(Some(cur.saturating_sub(step)));
                }
                Action::None
            }
            Action::GotoTop => {
                if !self.files.is_empty() {
                    self.list_state.select(Some(0));
                }
                Action::None
            }
            Action::GotoBottom => {
                if !self.files.is_empty() {
                    self.list_state
                        .select(Some(self.files.len().saturating_sub(1)));
                }
                Action::None
            }
            Action::HalfPageDown => {
                if !self.files.is_empty() {
                    let cur = self.list_state.selected().unwrap_or(0);
                    let next = (cur + 10).min(self.files.len().saturating_sub(1));
                    self.list_state.select(Some(next));
                }
                Action::None
            }
            Action::HalfPageUp => {
                if !self.files.is_empty() {
                    let cur = self.list_state.selected().unwrap_or(0);
                    self.list_state.select(Some(cur.saturating_sub(10)));
                }
                Action::None
            }
            Action::Confirm => {
                self.select_active_from_sidebar();
                Action::None
            }
            Action::LeaderSequence('r') => {
                self.refresh_conflicts();
                Action::None
            }
            Action::Quit
            | Action::LeaderKey
            | Action::LeaderSequence(_)
            | Action::SwitchTool(_)
            | Action::NextTool
            | Action::PrevTool
            | Action::ToolPicker
            | Action::Telescope
            | Action::Help
            | Action::SetMode(_) => action,
            _ => Action::None,
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        ui::render_merge_tool(
            frame,
            area,
            &self.files,
            self.list_state.selected(),
            self.sidebar_focused,
            self.active_file.as_deref(),
        );
    }

    fn handle_leader_action(&mut self, key: char) -> Option<Action> {
        match key {
            'r' => {
                self.refresh_conflicts();
                Some(Action::None)
            }
            _ => None,
        }
    }

    fn reset_key_state(&mut self) {
        self.key_state.reset();
    }

    fn on_focus(&mut self) {
        self.refresh_conflicts();
    }
}
