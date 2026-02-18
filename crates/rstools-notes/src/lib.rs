pub mod model;
pub mod sidebar;
pub mod ui;

use rstools_core::help_popup::HelpEntry;
use rstools_core::keybinds::{Action, InputMode, KeyState};
use rstools_core::telescope::TelescopeItem;
use rstools_core::tool::Tool;
use rstools_core::tree_sidebar::TreeEntry;
use rstools_core::vim_editor::{EditorAction, VimEditor, VimMode};
use rstools_core::which_key::WhichKeyEntry;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{layout::Rect, Frame};
use rusqlite::Connection;

use model::EntryType;
use sidebar::{ClipboardMode, NotesSidebarExt, SidebarInput, SidebarState, TreeNode};

#[derive(Debug, Clone)]
struct GrepCandidate {
    entry_id: i64,
    note_name: String,
    note_path: String,
    line_number: usize,
    line_text: String,
}

#[derive(Debug, Clone)]
struct GrepMatch {
    entry_id: i64,
    note_name: String,
    note_path: String,
    line_number: usize,
    column: usize,
    line_text: String,
}

pub struct NotesTool {
    sidebar: SidebarState,
    editor: VimEditor,
    mode: InputMode,
    key_state: KeyState,
    conn: Connection,
    /// Whether the sidebar is focused (vs editor panel).
    sidebar_focused: bool,
    /// The currently open note's entry ID, if any.
    active_note_id: Option<i64>,
    /// The currently open note's display name.
    active_note_name: Option<String>,
    /// Pending subleader group after <leader>s.
    pending_s_group: bool,
    /// Whether full-note grep overlay is active.
    grep_active: bool,
    /// Current grep query.
    grep_query: String,
    /// All searchable note lines.
    grep_candidates: Vec<GrepCandidate>,
    /// Filtered grep matches.
    grep_matches: Vec<GrepMatch>,
    /// Selected grep match index.
    grep_selected: usize,
}

impl NotesTool {
    pub fn new(conn: Connection) -> anyhow::Result<Self> {
        model::init_db(&conn)?;
        let mut sidebar = SidebarState::new();
        NotesSidebarExt::reload(&mut sidebar, &conn)?;
        Ok(Self {
            sidebar,
            editor: VimEditor::new(),
            mode: InputMode::Normal,
            key_state: KeyState::default(),
            conn,
            sidebar_focused: true,
            active_note_id: None,
            active_note_name: None,
            pending_s_group: false,
            grep_active: false,
            grep_query: String::new(),
            grep_candidates: Vec::new(),
            grep_matches: Vec::new(),
            grep_selected: 0,
        })
    }

    /// Open a note in the editor panel.
    fn open_note(&mut self, entry_id: i64, name: &str) {
        // Save current note if dirty
        self.auto_save_current();

        // Load the new note's content
        match model::get_note_content(&self.conn, entry_id) {
            Ok(content) => {
                self.editor.set_text(&content.body);
                self.editor.mark_clean();
                self.active_note_id = Some(entry_id);
                self.active_note_name = Some(name.to_string());
                self.sidebar_focused = false;
            }
            Err(_) => {
                // Note might not exist yet; set empty
                self.editor.set_text("");
                self.editor.mark_clean();
                self.active_note_id = Some(entry_id);
                self.active_note_name = Some(name.to_string());
                self.sidebar_focused = false;
            }
        }
    }

    /// Save the current note to the database.
    fn save_current_note(&mut self) -> bool {
        if let Some(entry_id) = self.active_note_id {
            let text = self.editor.text();
            if model::save_note_content(&self.conn, entry_id, &text).is_ok() {
                self.editor.mark_clean();
                return true;
            }
        }
        false
    }

    /// Auto-save if the current note is dirty.
    fn auto_save_current(&mut self) {
        if self.editor.is_dirty() && self.active_note_id.is_some() {
            self.save_current_note();
        }
    }

    /// Create entries from a path string (e.g., "folder/subfolder/note-name").
    fn create_entries_from_path(&mut self, path: &str) {
        let path = path.trim();
        if path.is_empty() {
            return;
        }

        let trailing_slash = path.ends_with('/');
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if segments.is_empty() {
            return;
        }

        // Determine parent from selected folder
        let mut parent_id: Option<i64> = self.sidebar.selected_entry().and_then(|e| {
            if e.is_folder {
                Some(e.entry_id)
            } else {
                // If a note is selected, use its parent
                sidebar::find_parent_id(&self.sidebar.roots, e.entry_id)
            }
        });

        for (i, segment) in segments.iter().enumerate() {
            let is_last = i == segments.len() - 1;
            let entry_type = if is_last && !trailing_slash {
                EntryType::Note
            } else {
                EntryType::Folder
            };

            // Check if a folder with this name already exists under parent
            if entry_type == EntryType::Folder {
                if let Some(existing_id) = self.find_folder_by_name(parent_id, segment) {
                    // Expand the existing folder
                    let _ = model::set_entry_expanded(&self.conn, existing_id, true);
                    parent_id = Some(existing_id);
                    continue;
                }
            }

            match model::add_entry(&self.conn, parent_id, segment, entry_type) {
                Ok(new_id) => {
                    if entry_type == EntryType::Folder {
                        let _ = model::set_entry_expanded(&self.conn, new_id, true);
                        parent_id = Some(new_id);
                    } else {
                        parent_id = Some(new_id);
                    }
                }
                Err(_) => break,
            }
        }

        // Reload and select the last created entry
        let _ = NotesSidebarExt::reload(&mut self.sidebar, &self.conn);
        if let Some(id) = parent_id {
            self.sidebar.select_entry(id);
        }
    }

    /// Find a folder by name under a given parent.
    fn find_folder_by_name(&self, parent_id: Option<i64>, name: &str) -> Option<i64> {
        let nodes = match parent_id {
            None => &self.sidebar.roots,
            Some(pid) => {
                if let Some(node) = sidebar::find_node(&self.sidebar.roots, pid) {
                    &node.children
                } else {
                    return None;
                }
            }
        };

        for node in nodes {
            if node.entry.is_folder() && node.entry.name() == name {
                return Some(node.entry.id());
            }
        }
        None
    }

    /// Submit the current sidebar input (add/rename).
    fn submit_sidebar_input(&mut self) {
        let text = self.sidebar.input_buffer.clone();

        match self.sidebar.input_mode {
            SidebarInput::Adding => {
                if !text.is_empty() {
                    self.create_entries_from_path(&text);
                }
            }
            SidebarInput::Renaming => {
                if !text.is_empty() {
                    if let Some(entry) = self.sidebar.selected_entry() {
                        let entry_id = entry.entry_id;
                        let _ = model::rename_entry(&self.conn, entry_id, &text);
                        // Update active name if we're renaming the open note
                        if self.active_note_id == Some(entry_id) {
                            self.active_note_name = Some(text.clone());
                        }
                        let _ = NotesSidebarExt::reload(&mut self.sidebar, &self.conn);
                    }
                }
            }
            _ => {}
        }
        self.sidebar.cancel_input();
    }

    /// Execute a delete of the selected sidebar entry.
    fn execute_delete(&mut self) {
        if let Some(entry) = self.sidebar.selected_entry() {
            let entry_id = entry.entry_id;
            let _ = model::delete_entry(&self.conn, entry_id);

            // If we deleted the active note, clear the editor
            if self.active_note_id == Some(entry_id) {
                self.active_note_id = None;
                self.active_note_name = None;
                self.editor.set_text("");
                self.editor.mark_clean();
            }

            let _ = NotesSidebarExt::reload(&mut self.sidebar, &self.conn);
        }
    }

    /// Execute a paste from the sidebar clipboard.
    fn execute_paste(&mut self) {
        if let Some(clip) = self.sidebar.clipboard.take() {
            let target_parent = self.sidebar.selected_entry().and_then(|e| {
                if e.is_folder {
                    Some(e.entry_id)
                } else {
                    sidebar::find_parent_id(&self.sidebar.roots, e.entry_id)
                }
            });

            match clip.mode {
                ClipboardMode::Copy => {
                    let _ = model::copy_entry_recursive(&self.conn, clip.entry_id, target_parent);
                }
                ClipboardMode::Cut => {
                    let _ = model::move_entry(&self.conn, clip.entry_id, target_parent);
                }
            }

            let _ = NotesSidebarExt::reload(&mut self.sidebar, &self.conn);
        }
    }

    /// Select a note by entry ID (used by telescope).
    fn select_note_by_entry_id(&mut self, entry_id: i64) -> bool {
        // Expand all parent folders
        self.expand_parents(entry_id);
        let _ = NotesSidebarExt::reload(&mut self.sidebar, &self.conn);

        self.sidebar.select_entry(entry_id);
        if let Some(entry) = self.sidebar.selected_entry() {
            if entry.entry_id == entry_id && !entry.is_folder {
                let name = entry.name.clone();
                self.open_note(entry_id, &name);
                return true;
            }
        }
        false
    }

    /// Expand all parent folders up to the root.
    fn expand_parents(&self, entry_id: i64) {
        let mut current_id = entry_id;
        while let Some(parent_id) = sidebar::find_parent_id(&self.sidebar.roots, current_id) {
            let _ = model::set_entry_expanded(&self.conn, parent_id, true);
            current_id = parent_id;
        }
    }

    // ── Full grep search ─────────────────────────────────────────────

    fn open_grep(&mut self) {
        self.pending_s_group = false;
        self.grep_active = true;
        self.grep_query.clear();
        self.grep_selected = 0;
        self.grep_candidates = self.collect_grep_candidates();
        self.grep_matches.clear();
    }

    fn close_grep(&mut self) {
        self.grep_active = false;
        self.grep_query.clear();
        self.grep_matches.clear();
        self.grep_selected = 0;
    }

    fn collect_grep_candidates(&self) -> Vec<GrepCandidate> {
        let Ok(entries) = model::list_entries(&self.conn) else {
            return Vec::new();
        };

        use std::collections::HashMap;
        let mut by_id: HashMap<i64, (Option<i64>, String, bool)> = HashMap::new();
        for e in &entries {
            by_id.insert(
                e.id,
                (
                    e.parent_id,
                    e.name.clone(),
                    e.entry_type == model::EntryType::Folder,
                ),
            );
        }

        let mut out = Vec::new();
        for e in entries {
            if e.entry_type == model::EntryType::Folder {
                continue;
            }

            let path = {
                let mut parts = vec![e.name.clone()];
                let mut cur = e.parent_id;
                while let Some(pid) = cur {
                    if let Some((ppid, name, _)) = by_id.get(&pid) {
                        parts.push(name.clone());
                        cur = *ppid;
                    } else {
                        break;
                    }
                }
                parts.reverse();
                parts.join("/")
            };

            let Ok(content) = model::get_note_content(&self.conn, e.id) else {
                continue;
            };

            for (idx, line) in content.body.lines().enumerate() {
                out.push(GrepCandidate {
                    entry_id: e.id,
                    note_name: e.name.clone(),
                    note_path: path.clone(),
                    line_number: idx + 1,
                    line_text: line.to_string(),
                });
            }
        }
        out
    }

    fn filter_grep(&mut self) {
        let q = self.grep_query.to_lowercase();
        if q.is_empty() {
            self.grep_matches.clear();
            self.grep_selected = 0;
            return;
        }

        self.grep_matches = self
            .grep_candidates
            .iter()
            .filter_map(|c| {
                let lower = c.line_text.to_lowercase();
                let col = lower.find(&q)?;
                Some(GrepMatch {
                    entry_id: c.entry_id,
                    note_name: c.note_name.clone(),
                    note_path: c.note_path.clone(),
                    line_number: c.line_number,
                    column: col,
                    line_text: c.line_text.clone(),
                })
            })
            .collect();

        if self.grep_matches.is_empty() {
            self.grep_selected = 0;
        } else if self.grep_selected >= self.grep_matches.len() {
            self.grep_selected = self.grep_matches.len() - 1;
        }
    }

    fn confirm_grep_selection(&mut self) {
        let Some(m) = self.grep_matches.get(self.grep_selected).cloned() else {
            return;
        };

        self.open_note(m.entry_id, &m.note_name);
        if self.editor.buffer.line_count() > 0 {
            let target_row = m.line_number.saturating_sub(1);
            self.editor.buffer.cursor_row = target_row.min(self.editor.buffer.line_count() - 1);
            let line_len = self.editor.buffer.current_line().len();
            self.editor.buffer.cursor_col = m.column.min(line_len);
            self.editor.buffer.desired_col = self.editor.buffer.cursor_col;
        }
        self.close_grep();
    }

    fn handle_grep_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc => self.close_grep(),
            KeyCode::Enter => self.confirm_grep_selection(),
            KeyCode::Down | KeyCode::Tab => {
                if !self.grep_matches.is_empty() {
                    self.grep_selected = (self.grep_selected + 1) % self.grep_matches.len();
                }
            }
            KeyCode::Up | KeyCode::BackTab => {
                if !self.grep_matches.is_empty() {
                    self.grep_selected = if self.grep_selected == 0 {
                        self.grep_matches.len() - 1
                    } else {
                        self.grep_selected - 1
                    };
                }
            }
            KeyCode::Char(c) => {
                self.grep_query.push(c);
                self.filter_grep();
            }
            KeyCode::Backspace => {
                self.grep_query.pop();
                self.filter_grep();
            }
            _ => {}
        }
        Action::None
    }

    // ── Key handling ─────────────────────────────────────────────────

    /// Handle key events when the sidebar is focused in Normal mode.
    fn handle_sidebar_normal_key(&mut self, key: KeyEvent) -> Action {
        // Handle pending two-key sequences (gg, gt, gT)
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

        if let Some(pending) = self.key_state.pending_key.take() {
            return match (pending, key.code) {
                ('g', KeyCode::Char('g')) => {
                    self.sidebar.goto_top();
                    Action::None
                }
                ('g', KeyCode::Char('t')) => Action::NextTool,
                ('g', KeyCode::Char('T')) => Action::PrevTool,
                _ => Action::None,
            };
        }

        match key.code {
            // Navigation
            KeyCode::Char('j') => {
                self.sidebar.move_down();
                Action::None
            }
            KeyCode::Char('k') => {
                self.sidebar.move_up();
                Action::None
            }
            KeyCode::Char('h') => {
                self.sidebar.collapse_or_parent_persist(&self.conn);
                Action::None
            }
            KeyCode::Char('l') => {
                if let Some(entry) = self.sidebar.selected_entry() {
                    if entry.is_folder {
                        self.sidebar.expand_selected_persist(&self.conn);
                    }
                }
                Action::None
            }
            KeyCode::Enter => {
                if let Some(entry) = self.sidebar.selected_entry() {
                    if entry.is_folder {
                        self.sidebar.toggle_expand_persist(&self.conn);
                    } else {
                        let id = entry.entry_id;
                        let name = entry.name.clone();
                        self.open_note(id, &name);
                    }
                }
                Action::None
            }
            KeyCode::Char('G') => {
                self.sidebar.goto_bottom();
                Action::None
            }
            KeyCode::Char('g') => {
                self.key_state.pending_key = Some('g');
                Action::None
            }
            KeyCode::Char('d') if key.modifiers == KeyModifiers::CONTROL => {
                self.sidebar.half_page_down(20);
                Action::None
            }
            KeyCode::Char('u') if key.modifiers == KeyModifiers::CONTROL => {
                self.sidebar.half_page_up(20);
                Action::None
            }

            // Neo-tree actions
            KeyCode::Char('a') => {
                self.sidebar.start_add();
                self.mode = InputMode::Insert;
                Action::None
            }
            KeyCode::Char('r') => {
                self.sidebar.start_rename();
                if self.sidebar.input_mode == SidebarInput::Renaming {
                    self.mode = InputMode::Insert;
                }
                Action::None
            }
            KeyCode::Char('d') => {
                self.sidebar.start_delete();
                Action::None
            }
            KeyCode::Char('y') => {
                self.sidebar.copy_selected();
                Action::None
            }
            KeyCode::Char('x') => {
                self.sidebar.cut_selected();
                Action::None
            }
            KeyCode::Char('p') => {
                self.execute_paste();
                Action::None
            }

            // Hub-level actions
            KeyCode::Char(' ') => {
                self.key_state.leader_active = true;
                Action::LeaderKey
            }
            KeyCode::Char(':') => Action::SetMode(InputMode::Command),
            KeyCode::Char('?') => Action::Help,
            KeyCode::Char('q') => Action::Quit,

            _ => Action::None,
        }
    }

    /// Handle key events when the sidebar is in insert mode (adding/renaming).
    fn handle_sidebar_insert_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc => {
                self.sidebar.cancel_input();
                self.mode = InputMode::Normal;
                Action::SetMode(InputMode::Normal)
            }
            KeyCode::Enter => {
                self.submit_sidebar_input();
                self.mode = InputMode::Normal;
                Action::SetMode(InputMode::Normal)
            }
            KeyCode::Char(c) => {
                self.sidebar.input_insert_char(c);
                Action::None
            }
            KeyCode::Backspace => {
                self.sidebar.input_backspace();
                Action::None
            }
            KeyCode::Left => {
                self.sidebar.input_cursor_left();
                Action::None
            }
            KeyCode::Right => {
                self.sidebar.input_cursor_right();
                Action::None
            }
            _ => Action::None,
        }
    }

    /// Handle key events when the sidebar is in delete confirmation mode.
    fn handle_confirm_delete_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                self.execute_delete();
                self.sidebar.cancel_input();
                Action::None
            }
            _ => {
                self.sidebar.cancel_input();
                Action::None
            }
        }
    }

    /// Handle key events for the editor panel in Normal mode.
    fn handle_editor_normal_key(&mut self, key: KeyEvent) -> Action {
        // Handle leader key state
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

        // Ctrl-h: move focus to sidebar
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('h') => {
                    if self.sidebar.visible {
                        self.sidebar_focused = true;
                    }
                    return Action::None;
                }
                // Consume other Ctrl-jkl as no-ops to avoid triggering editor keys
                KeyCode::Char('j') | KeyCode::Char('k') | KeyCode::Char('l') => {
                    return Action::None;
                }
                _ => {}
            }
        }

        // Hub-level keys before passing to editor
        match key.code {
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

        // Pass key to VimEditor
        let action = self.editor.handle_key(key);
        match action {
            EditorAction::ModeChanged(VimMode::Insert) => {
                self.mode = InputMode::Insert;
                Action::SetMode(InputMode::Insert)
            }
            _ => Action::None,
        }
    }

    /// Handle key events for the editor panel in Insert mode.
    fn handle_editor_insert_key(&mut self, key: KeyEvent) -> Action {
        let action = self.editor.handle_key(key);
        match action {
            EditorAction::ModeChanged(VimMode::Normal) => {
                self.mode = InputMode::Normal;
                Action::SetMode(InputMode::Normal)
            }
            _ => Action::None,
        }
    }

    // ── Telescope helpers ────────────────────────────────────────────

    /// Collect all notes as telescope items.
    fn collect_telescope_items(&self) -> Vec<TelescopeItem> {
        let mut items = Vec::new();
        self.collect_items_recursive(&self.sidebar.roots, "", &mut items);
        items
    }

    fn collect_items_recursive(
        &self,
        nodes: &[TreeNode<model::NoteEntry>],
        path_prefix: &str,
        items: &mut Vec<TelescopeItem>,
    ) {
        for node in nodes {
            let name = node.entry.name().to_string();
            let full_path = if path_prefix.is_empty() {
                name.clone()
            } else {
                format!("{}/{}", path_prefix, name)
            };

            if !node.entry.is_folder() {
                items.push(TelescopeItem {
                    label: name,
                    description: full_path.clone(),
                    id: format!("notes:{}", node.entry.id()),
                });
            }

            self.collect_items_recursive(&node.children, &full_path, items);
        }
    }
}

impl Tool for NotesTool {
    fn name(&self) -> &str {
        "Notes"
    }

    fn description(&self) -> &str {
        "Plain text notes with vim editor"
    }

    fn mode(&self) -> InputMode {
        self.mode
    }

    fn init_db(&self, conn: &Connection) -> anyhow::Result<()> {
        model::init_db(conn)
    }

    fn which_key_entries(&self) -> Vec<WhichKeyEntry> {
        vec![
            WhichKeyEntry::action('e', "Toggle sidebar"),
            WhichKeyEntry::group('s', "Search/Save"),
        ]
    }

    fn telescope_items(&self) -> Vec<TelescopeItem> {
        self.collect_telescope_items()
    }

    fn handle_telescope_selection(&mut self, id: &str) -> bool {
        let Some(raw_id) = id.strip_prefix("notes:") else {
            return false;
        };

        let Ok(entry_id) = raw_id.parse::<i64>() else {
            return false;
        };

        self.select_note_by_entry_id(entry_id)
    }

    fn help_entries(&self) -> Vec<HelpEntry> {
        vec![
            // Sidebar
            HelpEntry::with_section("Sidebar", "a", "Add entry (path with / for nesting)"),
            HelpEntry::with_section("Sidebar", "r", "Rename selected entry"),
            HelpEntry::with_section("Sidebar", "d", "Delete selected entry"),
            HelpEntry::with_section("Sidebar", "y", "Copy selected entry"),
            HelpEntry::with_section("Sidebar", "x", "Cut selected entry"),
            HelpEntry::with_section("Sidebar", "p", "Paste entry"),
            HelpEntry::with_section("Sidebar", "h", "Collapse folder / go to parent"),
            HelpEntry::with_section("Sidebar", "l / Enter", "Expand folder / open note"),
            HelpEntry::with_section("Sidebar", "j / k", "Navigate up / down"),
            HelpEntry::with_section("Sidebar", "gg / G", "Go to top / bottom"),
            HelpEntry::with_section("Sidebar", "Ctrl-l", "Move focus to editor"),
            // Editor
            HelpEntry::with_section("Editor", "i / a / A / I", "Enter insert mode"),
            HelpEntry::with_section("Editor", "o / O", "Insert line below / above"),
            HelpEntry::with_section("Editor", "v / V", "Visual / visual-line mode"),
            HelpEntry::with_section("Editor", "d/c/y + motion", "Delete/change/yank"),
            HelpEntry::with_section("Editor", "dd / yy / cc", "Line-wise operators"),
            HelpEntry::with_section("Editor", "u / Ctrl-r", "Undo / redo"),
            HelpEntry::with_section("Editor", "p / P", "Paste after / before"),
            HelpEntry::with_section("Editor", "Ctrl-h", "Move focus to sidebar"),
            HelpEntry::with_section("Editor", ":w", "Save note to database"),
            // General
            HelpEntry::with_section("General", "<Space>e", "Toggle sidebar"),
            HelpEntry::with_section("General", "<Space>s s", "Save note"),
            HelpEntry::with_section("General", "<Space>s g", "Grep note contents"),
        ]
    }

    fn handle_key(&mut self, key: KeyEvent) -> Action {
        if self.pending_s_group {
            self.pending_s_group = false;
            return match key.code {
                KeyCode::Char('s') => {
                    self.save_current_note();
                    Action::None
                }
                KeyCode::Char('g') => {
                    self.open_grep();
                    Action::None
                }
                _ => Action::None,
            };
        }

        if self.grep_active {
            return self.handle_grep_key(key);
        }

        match self.mode {
            InputMode::Normal => {
                if self.sidebar.visible && self.sidebar_focused {
                    // Sidebar is focused
                    if self.sidebar.input_mode == SidebarInput::ConfirmDelete {
                        return self.handle_confirm_delete_key(key);
                    }

                    // Ctrl-l to move focus to editor panel
                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        match key.code {
                            KeyCode::Char('l') => {
                                if self.active_note_id.is_some() {
                                    self.sidebar_focused = false;
                                }
                                return Action::None;
                            }
                            KeyCode::Char('j') | KeyCode::Char('k') | KeyCode::Char('h') => {
                                return Action::None;
                            }
                            _ => {}
                        }
                    }

                    self.handle_sidebar_normal_key(key)
                } else if self.active_note_id.is_some() {
                    // Editor panel is focused
                    self.handle_editor_normal_key(key)
                } else {
                    // Nothing active — handle hub-level keys only
                    let action =
                        rstools_core::keybinds::process_normal_key(key, &mut self.key_state);
                    match action {
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
            }
            InputMode::Insert => {
                if self.sidebar.visible
                    && self.sidebar_focused
                    && self.sidebar.input_mode != SidebarInput::None
                {
                    self.handle_sidebar_insert_key(key)
                } else {
                    self.handle_editor_insert_key(key)
                }
            }
            InputMode::Command => {
                // Command mode is handled by the hub
                Action::None
            }
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        ui::render_notes_tool(
            frame,
            area,
            &self.sidebar,
            &self.editor,
            self.sidebar_focused,
            self.active_note_name.as_deref(),
        );

        if self.grep_active {
            let rows: Vec<String> = self
                .grep_matches
                .iter()
                .map(|m| format!("{}:{}  {}", m.note_path, m.line_number, m.line_text.trim()))
                .collect();

            let (preview_title, preview_text, preview_target_line) =
                if let Some(m) = self.grep_matches.get(self.grep_selected) {
                    let title = m.note_path.clone();
                    let body = model::get_note_content(&self.conn, m.entry_id)
                        .map(|c| c.body)
                        .unwrap_or_else(|_| "(unable to load note content)".to_string());
                    (title, body, Some(m.line_number.saturating_sub(1)))
                } else {
                    (
                        "No selection".to_string(),
                        "Type to grep note contents...".to_string(),
                        None,
                    )
                };

            ui::render_grep_overlay(
                frame,
                area,
                &self.grep_query,
                &rows,
                self.grep_selected,
                &preview_title,
                &preview_text,
                preview_target_line,
            );
        }
    }

    fn handle_leader_action(&mut self, key: char) -> Option<Action> {
        match key {
            'e' => {
                self.sidebar.visible = !self.sidebar.visible;
                if self.sidebar.visible {
                    self.sidebar_focused = true;
                }
                Some(Action::None)
            }
            's' => {
                self.pending_s_group = true;
                Some(Action::None)
            }
            _ => None,
        }
    }

    fn reset_key_state(&mut self) {
        self.key_state.reset();
        self.pending_s_group = false;
    }

    fn handle_paste(&mut self, text: &str) -> Action {
        if self.active_note_id.is_some() && !self.sidebar_focused {
            self.editor.paste_text(text);
            // Sync mode: if editor ended up in Insert, update our mode
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
        } else if self.sidebar.input_mode != SidebarInput::None {
            // If sidebar is in input mode (adding/renaming), insert into input buffer
            for c in text.chars() {
                if c != '\n' && c != '\r' {
                    self.sidebar.input_insert_char(c);
                }
            }
            Action::None
        } else {
            Action::None
        }
    }

    fn on_focus(&mut self) {
        let _ = NotesSidebarExt::reload(&mut self.sidebar, &self.conn);
    }

    fn handle_command(&mut self, cmd: &str) -> bool {
        match cmd.trim() {
            "w" | "write" => self.save_current_note(),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstools_core::db::open_memory_db;

    fn setup_tool() -> NotesTool {
        let conn = open_memory_db().unwrap();
        NotesTool::new(conn).unwrap()
    }

    #[test]
    fn test_create_simple_note() {
        let mut tool = setup_tool();
        tool.create_entries_from_path("my-note");

        assert_eq!(tool.sidebar.flat_view.len(), 1);
        assert_eq!(tool.sidebar.flat_view[0].name, "my-note");
        assert!(!tool.sidebar.flat_view[0].is_folder);
    }

    #[test]
    fn test_create_nested_path() {
        let mut tool = setup_tool();
        tool.create_entries_from_path("work/project/todo");

        assert_eq!(tool.sidebar.flat_view.len(), 3);
        assert_eq!(tool.sidebar.flat_view[0].name, "work");
        assert!(tool.sidebar.flat_view[0].is_folder);
        assert_eq!(tool.sidebar.flat_view[1].name, "project");
        assert!(tool.sidebar.flat_view[1].is_folder);
        assert_eq!(tool.sidebar.flat_view[2].name, "todo");
        assert!(!tool.sidebar.flat_view[2].is_folder);
    }

    #[test]
    fn test_create_folder_only_with_trailing_slash() {
        let mut tool = setup_tool();
        tool.create_entries_from_path("work/projects/");

        assert_eq!(tool.sidebar.flat_view.len(), 2);
        assert!(tool.sidebar.flat_view[0].is_folder);
        assert!(tool.sidebar.flat_view[1].is_folder);
    }

    #[test]
    fn test_open_and_save_note() {
        let mut tool = setup_tool();
        tool.create_entries_from_path("test-note");

        let entry_id = tool.sidebar.flat_view[0].entry_id;
        tool.open_note(entry_id, "test-note");

        assert_eq!(tool.active_note_id, Some(entry_id));
        assert!(!tool.editor.is_dirty());

        // Simulate editing: enter insert mode and type
        tool.editor
            .handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        for c in "Hello, world!".chars() {
            tool.editor
                .handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        tool.editor
            .handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(tool.editor.is_dirty());

        // Save
        assert!(tool.save_current_note());
        assert!(!tool.editor.is_dirty());

        // Verify persisted
        let content = model::get_note_content(&tool.conn, entry_id).unwrap();
        assert_eq!(content.body, "Hello, world!");
    }

    #[test]
    fn test_delete_active_note_clears_editor() {
        let mut tool = setup_tool();
        tool.create_entries_from_path("doomed");

        let entry_id = tool.sidebar.flat_view[0].entry_id;
        tool.open_note(entry_id, "doomed");
        tool.editor.set_text("some content");

        // Select the entry for deletion
        tool.sidebar.select_entry(entry_id);
        tool.execute_delete();

        assert_eq!(tool.active_note_id, None);
        assert_eq!(tool.editor.text(), "");
    }

    #[test]
    fn test_telescope_items() {
        let mut tool = setup_tool();
        tool.create_entries_from_path("folder/note-a");
        tool.create_entries_from_path("note-b");

        let items = tool.telescope_items();
        assert_eq!(items.len(), 2); // Only notes, not folders

        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"note-a"));
        assert!(labels.contains(&"note-b"));
    }

    #[test]
    fn test_leader_s_group_for_save_and_grep() {
        let mut tool = setup_tool();

        // first 's' enters subgroup and waits for second key
        let action = tool.handle_leader_action('s');
        assert!(matches!(action, Some(Action::None)));
        assert!(tool.pending_s_group);

        // second key 'g' opens grep overlay
        let action = tool.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
        assert!(matches!(action, Action::None));
        assert!(tool.grep_active);
    }

    #[test]
    fn test_grep_finds_note_content() {
        let mut tool = setup_tool();
        tool.create_entries_from_path("work/note1");
        tool.create_entries_from_path("note2");

        let note1_id = tool
            .sidebar
            .flat_view
            .iter()
            .find(|e| e.name == "note1")
            .unwrap()
            .entry_id;
        let note2_id = tool
            .sidebar
            .flat_view
            .iter()
            .find(|e| e.name == "note2")
            .unwrap()
            .entry_id;

        model::save_note_content(&tool.conn, note1_id, "hello rust world\nsecond line").unwrap();
        model::save_note_content(&tool.conn, note2_id, "another note").unwrap();

        tool.open_grep();
        tool.grep_query = "rust".to_string();
        tool.filter_grep();

        assert_eq!(tool.grep_matches.len(), 1);
        assert_eq!(tool.grep_matches[0].entry_id, note1_id);
        assert_eq!(tool.grep_matches[0].line_number, 1);
    }
}
