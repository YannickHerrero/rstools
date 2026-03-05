pub mod conflict;
pub mod ui;

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{layout::Rect, widgets::ListState, Frame};
use rstools_core::{
    help_popup::HelpEntry,
    keybinds::{process_normal_key, Action, InputMode, KeyState},
    telescope::TelescopeItem,
    tool::Tool,
    vim_editor::{EditorAction, VimEditor, VimMode},
    which_key::WhichKeyEntry,
};
use rusqlite::Connection;

use crate::conflict::{
    apply_hunk_choice, has_conflict_markers, hunk_preview, parse_conflicts, HunkChoice,
};

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
    active_kind: Option<ConflictKind>,
    editor: VimEditor,
    drafts: HashMap<String, String>,
    current_hunk: usize,
    pending_c_action: bool,
    notification: Option<String>,
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
            active_kind: None,
            editor: VimEditor::new(),
            drafts: HashMap::new(),
            current_hunk: 0,
            pending_c_action: false,
            notification: None,
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
            self.active_kind = None;
            return;
        }

        let selected = self.list_state.selected().unwrap_or(0);
        let clamped = selected.min(self.files.len().saturating_sub(1));
        self.list_state.select(Some(clamped));

        if let Some(active_path) = self.active_file.as_ref() {
            if let Some(file) = self.files.iter().find(|f| &f.path == active_path) {
                self.active_kind = Some(file.kind);
                return;
            }
            self.active_file = None;
            self.active_kind = None;
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

    fn open_selected_file(&mut self) {
        let Some(selected) = self.selected_file().cloned() else {
            return;
        };

        self.save_current_draft();
        self.active_file = Some(selected.path.clone());
        self.active_kind = Some(selected.kind);
        self.sidebar_focused = false;
        self.pending_c_action = false;

        match selected.kind {
            ConflictKind::Text => {
                if let Some(text) = self.read_active_text() {
                    self.editor.set_text(&text);
                    self.editor.mark_clean();
                    self.current_hunk = 0;
                    self.sync_hunk_state();
                    self.mode = InputMode::Normal;
                }
            }
            ConflictKind::Binary => {
                self.editor.set_text("");
                self.current_hunk = 0;
                self.mode = InputMode::Normal;
            }
        }
    }

    fn read_active_text(&self) -> Option<String> {
        let repo_root = self.repo_root.as_ref()?;
        let active = self.active_file.as_ref()?;

        if let Some(draft) = self.drafts.get(active) {
            return Some(draft.clone());
        }

        std::fs::read_to_string(repo_root.join(active)).ok()
    }

    fn save_current_draft(&mut self) {
        if self.active_kind != Some(ConflictKind::Text) {
            return;
        }
        if let Some(path) = self.active_file.as_ref() {
            self.drafts.insert(path.clone(), self.editor.text());
        }
    }

    fn show_notification(&mut self, message: impl Into<String>) {
        self.notification = Some(message.into());
    }

    fn run_git(&self, args: &[&str]) -> bool {
        let Some(repo_root) = self.repo_root.as_ref() else {
            return false;
        };

        let output = Command::new("git")
            .args(args)
            .current_dir(repo_root)
            .output();
        let Ok(output) = output else {
            return false;
        };

        output.status.success()
    }

    fn write_active_file(&mut self) -> bool {
        if self.active_kind != Some(ConflictKind::Text) {
            return false;
        }

        let Some(repo_root) = self.repo_root.as_ref() else {
            self.show_notification("Not inside a git repository");
            return false;
        };
        let Some(active_path) = self.active_file.as_ref().cloned() else {
            return false;
        };

        let text = self.editor.text();
        if std::fs::write(repo_root.join(&active_path), text.as_bytes()).is_err() {
            self.show_notification("Failed to write file");
            return false;
        }

        self.drafts.insert(active_path.clone(), text.clone());

        if !has_conflict_markers(&text) {
            if self.run_git(&["add", "--", &active_path]) {
                self.refresh_conflicts();
                if self.active_file.as_deref() != Some(active_path.as_str()) {
                    self.sidebar_focused = true;
                    self.show_notification("Saved and staged resolved file");
                } else {
                    self.show_notification("Saved and staged");
                }
                return true;
            }
            self.show_notification("Saved, but failed to stage");
            return false;
        }

        self.show_notification("Saved (still contains conflict markers)");
        true
    }

    fn apply_binary_choice(&mut self, ours: bool) {
        if self.active_kind != Some(ConflictKind::Binary) {
            return;
        }

        let Some(path) = self.active_file.as_ref().cloned() else {
            return;
        };

        let checkout_side = if ours { "--ours" } else { "--theirs" };
        if !self.run_git(&["checkout", checkout_side, "--", &path]) {
            self.show_notification("Failed to apply selected binary side");
            return;
        }

        if self.run_git(&["add", "--", &path]) {
            self.refresh_conflicts();
            self.sidebar_focused = true;
            self.pending_c_action = false;
            if ours {
                self.show_notification("Applied ours and staged binary file");
            } else {
                self.show_notification("Applied theirs and staged binary file");
            }
        } else {
            self.show_notification("Applied side, but failed to stage binary file");
        }
    }

    fn sync_hunk_state(&mut self) {
        let parsed = parse_conflicts(&self.editor.text());
        if parsed.hunks.is_empty() {
            self.current_hunk = 0;
            return;
        }

        if self.current_hunk >= parsed.hunks.len() {
            self.current_hunk = parsed.hunks.len() - 1;
        }

        if let Some(hunk) = parsed.hunks.get(self.current_hunk) {
            let max_row = self.editor.buffer.line_count().saturating_sub(1);
            self.editor.buffer.cursor_row = hunk.start_line.min(max_row);
            self.editor.buffer.cursor_col = 0;
            self.editor.buffer.desired_col = 0;
        }
    }

    fn move_next_hunk(&mut self) {
        let parsed = parse_conflicts(&self.editor.text());
        if parsed.hunks.is_empty() {
            return;
        }
        self.current_hunk = (self.current_hunk + 1).min(parsed.hunks.len() - 1);
        self.sync_hunk_state();
    }

    fn move_prev_hunk(&mut self) {
        let parsed = parse_conflicts(&self.editor.text());
        if parsed.hunks.is_empty() {
            return;
        }
        self.current_hunk = self.current_hunk.saturating_sub(1);
        self.sync_hunk_state();
    }

    fn apply_current_hunk_choice(&mut self, choice: HunkChoice) {
        let current = self.editor.text();
        let Some(next) = apply_hunk_choice(&current, self.current_hunk, choice) else {
            return;
        };

        self.editor.set_text(&next);
        self.editor.mark_clean();
        self.save_current_draft();
        self.sync_hunk_state();
    }

    fn handle_sidebar_normal_key(&mut self, key: KeyEvent) -> Action {
        if key.modifiers == KeyModifiers::CONTROL {
            match key.code {
                KeyCode::Char('l') => {
                    if self.active_kind == Some(ConflictKind::Text) {
                        self.sidebar_focused = false;
                    }
                    return Action::None;
                }
                KeyCode::Char('h') | KeyCode::Char('j') | KeyCode::Char('k') => {
                    return Action::None;
                }
                _ => {}
            }
        }

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
                self.open_selected_file();
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

    fn handle_editor_normal_key(&mut self, key: KeyEvent) -> Action {
        if self.key_state.leader_active {
            self.key_state.leader_active = false;
            return match key.code {
                KeyCode::Char(' ') => Action::ToolPicker,
                KeyCode::Char('f') => Action::Telescope,
                KeyCode::Char(c @ '1'..='9') => {
                    let idx = (c as u8 - b'1') as usize;
                    Action::SwitchTool(idx)
                }
                KeyCode::Char('q') => Action::Quit,
                KeyCode::Char(c) => Action::LeaderSequence(c),
                KeyCode::Esc => Action::None,
                _ => Action::None,
            };
        }

        if self.pending_c_action {
            self.pending_c_action = false;
            return match key.code {
                KeyCode::Char('o') => {
                    self.apply_current_hunk_choice(HunkChoice::Ours);
                    Action::None
                }
                KeyCode::Char('t') => {
                    self.apply_current_hunk_choice(HunkChoice::Theirs);
                    Action::None
                }
                KeyCode::Char('b') => {
                    self.apply_current_hunk_choice(HunkChoice::Both);
                    Action::None
                }
                _ => Action::None,
            };
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('h') => {
                    self.sidebar_focused = true;
                    return Action::None;
                }
                KeyCode::Char('d') => {
                    self.move_next_hunk();
                    return Action::None;
                }
                KeyCode::Char('u') => {
                    self.move_prev_hunk();
                    return Action::None;
                }
                KeyCode::Char('j') | KeyCode::Char('k') | KeyCode::Char('l') => {
                    return Action::None;
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Char('c') if key.modifiers == KeyModifiers::NONE => {
                self.pending_c_action = true;
                return Action::None;
            }
            KeyCode::Char(' ') if key.modifiers == KeyModifiers::NONE => {
                self.key_state.leader_active = true;
                return Action::LeaderKey;
            }
            KeyCode::Char(':') if key.modifiers == KeyModifiers::NONE => {
                return Action::SetMode(InputMode::Command);
            }
            KeyCode::Char('?') if key.modifiers == KeyModifiers::NONE => {
                return Action::Help;
            }
            _ => {}
        }

        match self.editor.handle_key(key) {
            EditorAction::ModeChanged(VimMode::Insert) => {
                self.mode = InputMode::Insert;
                self.save_current_draft();
                self.sync_hunk_state();
                Action::SetMode(InputMode::Insert)
            }
            EditorAction::EnterCommandMode => Action::SetMode(InputMode::Command),
            _ => {
                self.save_current_draft();
                self.sync_hunk_state();
                Action::None
            }
        }
    }

    fn handle_editor_insert_key(&mut self, key: KeyEvent) -> Action {
        match self.editor.handle_key(key) {
            EditorAction::ModeChanged(VimMode::Normal) => {
                self.mode = InputMode::Normal;
                self.save_current_draft();
                self.sync_hunk_state();
                Action::SetMode(InputMode::Normal)
            }
            _ => {
                self.save_current_draft();
                self.sync_hunk_state();
                Action::None
            }
        }
    }

    fn handle_binary_normal_key(&mut self, key: KeyEvent) -> Action {
        if self.key_state.leader_active {
            self.key_state.leader_active = false;
            return match key.code {
                KeyCode::Char(' ') => Action::ToolPicker,
                KeyCode::Char('f') => Action::Telescope,
                KeyCode::Char(c @ '1'..='9') => {
                    let idx = (c as u8 - b'1') as usize;
                    Action::SwitchTool(idx)
                }
                KeyCode::Char('q') => Action::Quit,
                KeyCode::Char(c) => Action::LeaderSequence(c),
                KeyCode::Esc => Action::None,
                _ => Action::None,
            };
        }

        if self.pending_c_action {
            self.pending_c_action = false;
            return match key.code {
                KeyCode::Char('o') => {
                    self.apply_binary_choice(true);
                    Action::None
                }
                KeyCode::Char('t') => {
                    self.apply_binary_choice(false);
                    Action::None
                }
                _ => Action::None,
            };
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('h') => {
                    self.sidebar_focused = true;
                    return Action::None;
                }
                KeyCode::Char('j') | KeyCode::Char('k') | KeyCode::Char('l') => {
                    return Action::None;
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Char('c') if key.modifiers == KeyModifiers::NONE => {
                self.pending_c_action = true;
                Action::None
            }
            KeyCode::Char(' ') if key.modifiers == KeyModifiers::NONE => {
                self.key_state.leader_active = true;
                Action::LeaderKey
            }
            KeyCode::Char(':') if key.modifiers == KeyModifiers::NONE => {
                Action::SetMode(InputMode::Command)
            }
            KeyCode::Char('?') if key.modifiers == KeyModifiers::NONE => Action::Help,
            KeyCode::Char('q') if key.modifiers == KeyModifiers::NONE => Action::Quit,
            _ => Action::None,
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
            self.open_selected_file();
            return true;
        }

        false
    }

    fn help_entries(&self) -> Vec<HelpEntry> {
        vec![
            HelpEntry::with_section("Merge Sidebar", "j / k", "Navigate conflicted files"),
            HelpEntry::with_section("Merge Sidebar", "Enter", "Open selected conflicted file"),
            HelpEntry::with_section(
                "Merge Editor",
                "Ctrl-d / Ctrl-u",
                "Next / previous conflict hunk",
            ),
            HelpEntry::with_section(
                "Merge Editor",
                "co / ct / cb",
                "Apply ours / theirs / both to current hunk",
            ),
            HelpEntry::with_section("Merge Editor", "Ctrl-h", "Move focus to sidebar"),
            HelpEntry::with_section("Merge", ":w", "Save file and stage if fully resolved"),
            HelpEntry::with_section("Merge", ":wq", "Save and close current tool"),
            HelpEntry::with_section("Merge", "<Space>r", "Refresh conflicted files"),
        ]
    }

    fn handle_key(&mut self, key: KeyEvent) -> Action {
        self.notification = None;

        if self.active_kind == Some(ConflictKind::Binary) && !self.sidebar_focused {
            return self.handle_binary_normal_key(key);
        }

        match self.mode {
            InputMode::Insert => self.handle_editor_insert_key(key),
            InputMode::Normal => {
                if self.sidebar_focused {
                    self.handle_sidebar_normal_key(key)
                } else {
                    self.handle_editor_normal_key(key)
                }
            }
            InputMode::Command => Action::None,
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let hunk_info = if self.active_kind == Some(ConflictKind::Text) {
            let text = self.editor.text();
            let parsed = parse_conflicts(&text);
            let preview = hunk_preview(&text, self.current_hunk, 3);
            Some((self.current_hunk, parsed.hunks.len(), preview))
        } else {
            None
        };

        ui::render_merge_tool(
            frame,
            area,
            &self.files,
            self.list_state.selected(),
            self.sidebar_focused,
            self.active_file.as_deref(),
            self.active_kind,
            &self.editor,
            hunk_info,
            self.notification.as_deref(),
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
        self.pending_c_action = false;
    }

    fn on_focus(&mut self) {
        self.refresh_conflicts();
    }

    fn on_blur(&mut self) {
        self.save_current_draft();
    }

    fn handle_paste(&mut self, text: &str) -> Action {
        if self.active_kind != Some(ConflictKind::Text) || self.sidebar_focused {
            return Action::None;
        }
        self.editor.paste_text(text);
        self.save_current_draft();
        self.sync_hunk_state();
        match self.editor.mode {
            VimMode::Insert => {
                self.mode = InputMode::Insert;
                Action::SetMode(InputMode::Insert)
            }
            _ => {
                self.mode = InputMode::Normal;
                Action::None
            }
        }
    }

    fn handle_command(&mut self, cmd: &str) -> bool {
        match cmd.trim() {
            "w" | "write" => self.write_active_file(),
            _ => false,
        }
    }
}
