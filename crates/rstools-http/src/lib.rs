pub mod executor;
pub mod model;
pub mod request_panel;
pub mod sidebar;
pub mod ui;

use std::collections::HashMap;

use rstools_core::help_popup::HelpEntry;
use rstools_core::keybinds::{Action, InputMode, KeyState, process_normal_key};
use rstools_core::telescope::TelescopeItem;
use rstools_core::tool::Tool;
use rstools_core::which_key::WhichKeyEntry;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Frame, layout::Rect};
use rusqlite::Connection;

use executor::{HttpExecutor, HttpRequestCmd};
use model::EntryType;
use request_panel::{KvField, PanelFocus, RequestPanel, ResponseData, ResponseSection, Section};
use sidebar::{ClipboardMode, SidebarInput, SidebarState};

/// Cached response data for a query, keyed by entry_id.
/// Allows restoring the last response when switching back to a previously-run query.
struct CachedResponse {
    response: Option<ResponseData>,
    error_message: Option<String>,
}

pub struct HttpTool {
    sidebar: SidebarState,
    panel: RequestPanel,
    mode: InputMode,
    key_state: KeyState,
    conn: Connection,
    executor: HttpExecutor,
    /// Whether the sidebar is focused (vs content panel).
    sidebar_focused: bool,
    /// In-memory cache of the last response per query (keyed by entry_id).
    response_cache: HashMap<i64, CachedResponse>,
}

impl HttpTool {
    pub fn new(conn: Connection) -> anyhow::Result<Self> {
        model::init_db(&conn)?;
        let mut sidebar = SidebarState::new();
        sidebar.reload(&conn)?;
        let executor = HttpExecutor::spawn();
        Ok(Self {
            sidebar,
            panel: RequestPanel::new(),
            mode: InputMode::Normal,
            key_state: KeyState::default(),
            conn,
            executor,
            sidebar_focused: true,
            response_cache: HashMap::new(),
        })
    }

    /// Send the current request via the executor.
    fn send_request(&mut self) {
        if !self.panel.is_active() || self.panel.request_in_flight {
            return;
        }

        let url = self.panel.build_url_with_params();
        if url.is_empty() {
            self.panel.error_message = Some("URL is empty".to_string());
            return;
        }

        let cmd = HttpRequestCmd {
            method: self.panel.method,
            url,
            headers: self.panel.enabled_headers(),
            body: self.panel.body_text(),
        };

        if self.executor.send(cmd).is_ok() {
            self.panel.request_in_flight = true;
            self.panel.error_message = None;
            self.panel.response = None;
            self.panel.spinner_frame = 0;
        }
    }

    /// Check for async response results.
    fn poll_response(&mut self) {
        if let Some(result) = self.executor.try_recv() {
            self.panel.request_in_flight = false;
            match result {
                Ok(resp) => {
                    // Pretty-print JSON if possible
                    let body =
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&resp.body) {
                            serde_json::to_string_pretty(&json).unwrap_or(resp.body)
                        } else {
                            resp.body
                        };

                    let response_data = ResponseData {
                        status_code: resp.status_code,
                        status_text: resp.status_text,
                        elapsed_ms: resp.elapsed_ms,
                        size_bytes: resp.size_bytes,
                        headers: resp.headers,
                        body,
                        body_scroll: 0,
                        headers_scroll: 0,
                        focused_section: ResponseSection::Body,
                    };

                    self.panel.response = Some(response_data.clone());
                    self.panel.error_message = None;

                    // Cache the response for this query
                    if let Some(entry_id) = self.panel.active_entry_id {
                        self.response_cache.insert(
                            entry_id,
                            CachedResponse {
                                response: self.panel.response.clone(),
                                error_message: None,
                            },
                        );
                    }
                }
                Err(e) => {
                    self.panel.error_message = Some(e.message.clone());

                    // Cache the error for this query
                    if let Some(entry_id) = self.panel.active_entry_id {
                        self.response_cache.insert(
                            entry_id,
                            CachedResponse {
                                response: None,
                                error_message: Some(e.message),
                            },
                        );
                    }
                }
            }
        }
    }

    /// Save the current panel's response/error into the cache before switching away.
    fn cache_current_response(&mut self) {
        if let Some(entry_id) = self.panel.active_entry_id {
            if self.panel.response.is_some() || self.panel.error_message.is_some() {
                self.response_cache.insert(
                    entry_id,
                    CachedResponse {
                        response: self.panel.response.clone(),
                        error_message: self.panel.error_message.clone(),
                    },
                );
            }
        }
    }

    /// Open a query in the content panel.
    fn open_query(&mut self, entry_id: i64, name: &str) {
        // Cache the current query's response before switching
        self.cache_current_response();

        let _ = self.panel.load(entry_id, name, &self.conn);

        // Restore cached response if available (load() resets response to None)
        if let Some(cached) = self.response_cache.get(&entry_id) {
            self.panel.response = cached.response.clone().map(|mut r| {
                // Reset scroll positions when restoring
                r.body_scroll = 0;
                r.headers_scroll = 0;
                r.focused_section = ResponseSection::Body;
                r
            });
            self.panel.error_message = cached.error_message.clone();
        }

        self.sidebar_focused = false;
    }

    /// Save the current panel to the database.
    fn save_panel(&mut self) -> bool {
        if self.panel.is_active() {
            self.panel.save(&self.conn).is_ok()
        } else {
            false
        }
    }

    /// Handle key events when the sidebar is focused in Normal mode.
    /// Returns Some(action) if the key was handled, None if it should fall through.
    fn handle_sidebar_normal_key(&mut self, key: KeyEvent) -> Action {
        // Handle pending two-key sequences first (gg, gt, gT, dd is NOT used here)
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

        // Sidebar-specific keys (neo-tree style)
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
                self.sidebar.collapse_or_parent(&self.conn);
                Action::None
            }
            KeyCode::Char('l') => {
                // l only expands folders (never collapses), like neo-tree
                if let Some(entry) = self.sidebar.selected_entry() {
                    if entry.entry_type == EntryType::Folder {
                        self.sidebar.expand_selected(&self.conn);
                    }
                }
                Action::None
            }
            KeyCode::Enter => {
                if let Some(entry) = self.sidebar.selected_entry() {
                    if entry.entry_type == EntryType::Folder {
                        self.sidebar.toggle_expand(&self.conn);
                    } else {
                        // Open query in the content panel
                        let id = entry.entry_id;
                        let name = entry.name.clone();
                        self.open_query(id, &name);
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
                self.sidebar.half_page_down(20); // approximate visible lines
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
                // Stay in Normal mode — confirm delete is y/n
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
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                self.sidebar.cancel_input();
                Action::None
            }
            _ => Action::None,
        }
    }

    /// Submit the current sidebar input (add or rename).
    fn submit_sidebar_input(&mut self) {
        let input = self.sidebar.input_buffer.trim().to_string();
        let input_mode = self.sidebar.input_mode.clone();

        match input_mode {
            SidebarInput::Adding => {
                if !input.is_empty() {
                    self.create_entries_from_path(&input);
                }
            }
            SidebarInput::Renaming => {
                if !input.is_empty() {
                    if let Some(entry_id) = self.sidebar.selected_entry_id() {
                        let _ = model::rename_entry(&self.conn, entry_id, &input);
                        let _ = self.sidebar.reload(&self.conn);
                    }
                }
            }
            _ => {}
        }

        self.sidebar.cancel_input();
    }

    /// Create entries from a path like "group A/api/get-user".
    /// Intermediate segments become folders, the last segment becomes a query
    /// (unless the path ends with "/", in which case all segments are folders).
    fn create_entries_from_path(&mut self, path: &str) {
        let trailing_slash = path.ends_with('/');
        let segments: Vec<&str> = path
            .split('/')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();

        if segments.is_empty() {
            return;
        }

        // Determine the parent: if selected entry is a folder and expanded, create inside it.
        // Otherwise create at the same level as the selected entry.
        let mut parent_id = self.get_creation_parent_id();

        for (i, segment) in segments.iter().enumerate() {
            let is_last = i == segments.len() - 1;
            let entry_type = if is_last && !trailing_slash {
                EntryType::Query
            } else {
                EntryType::Folder
            };

            // Check if a folder with this name already exists at this level
            if entry_type == EntryType::Folder {
                if let Some(existing_id) = self.find_existing_folder(parent_id, segment) {
                    parent_id = Some(existing_id);
                    continue;
                }
            }

            match model::add_entry(&self.conn, parent_id, segment, entry_type) {
                Ok(new_id) => {
                    if entry_type == EntryType::Folder {
                        parent_id = Some(new_id);
                    }
                }
                Err(_) => break,
            }
        }

        let _ = self.sidebar.reload(&self.conn);

        // Expand parent folders so the new entry is visible
        self.expand_path_to_parent(parent_id);
    }

    /// Get the parent_id for creating new entries.
    /// If selected entry is an expanded folder, create inside it.
    /// If selected entry is a collapsed folder or a query, create at the same level.
    fn get_creation_parent_id(&self) -> Option<i64> {
        if let Some(entry) = self.sidebar.selected_entry() {
            if entry.entry_type == EntryType::Folder && entry.is_expanded {
                Some(entry.entry_id)
            } else {
                // Same level as selected entry
                sidebar::find_parent_id(&self.sidebar.roots, entry.entry_id)
            }
        } else {
            None // Root level
        }
    }

    /// Find an existing folder with the given name under the given parent.
    fn find_existing_folder(&self, parent_id: Option<i64>, name: &str) -> Option<i64> {
        // Search the tree for a matching folder
        let nodes = if let Some(pid) = parent_id {
            // Find the parent node and search its children
            sidebar::find_node(&self.sidebar.roots, pid)
                .map(|n| n.children.as_slice())
                .unwrap_or(&[])
        } else {
            self.sidebar.roots.as_slice()
        };

        for node in nodes {
            if node.entry.entry_type == EntryType::Folder
                && node.entry.name.eq_ignore_ascii_case(name)
            {
                return Some(node.entry.id);
            }
        }
        None
    }

    /// Expand all parent folders on the path to make the entry visible.
    fn expand_path_to_parent(&mut self, target_parent_id: Option<i64>) {
        if let Some(pid) = target_parent_id {
            // Collect the chain of ancestors
            let mut to_expand = vec![pid];
            let mut current = pid;
            while let Some(grandparent) = sidebar::find_parent_id(&self.sidebar.roots, current) {
                to_expand.push(grandparent);
                current = grandparent;
            }

            // Expand from root down
            for id in to_expand.into_iter().rev() {
                if let Some(node) = sidebar::find_node_mut(&mut self.sidebar.roots, id) {
                    node.expanded = true;
                    let _ = model::set_entry_expanded(&self.conn, id, true);
                }
            }
            self.sidebar.rebuild_flat_view();
        }
    }

    /// Execute the delete operation on the selected entry.
    fn execute_delete(&mut self) {
        if let Some(entry_id) = self.sidebar.selected_entry_id() {
            let _ = model::delete_entry(&self.conn, entry_id);
            // If we just deleted the clipboard source, clear the clipboard
            if let Some(ref clip) = self.sidebar.clipboard {
                if clip.entry_id == entry_id {
                    self.sidebar.clipboard = None;
                }
            }
            // Remove cached response for deleted entry
            self.response_cache.remove(&entry_id);
            let _ = self.sidebar.reload(&self.conn);
        }
    }

    /// Execute the paste operation from the clipboard.
    fn execute_paste(&mut self) {
        let clipboard = match self.sidebar.clipboard.take() {
            Some(c) => c,
            None => return,
        };

        let target_parent_id = self.sidebar.paste_target_parent_id();

        match clipboard.mode {
            ClipboardMode::Copy => {
                let _ =
                    model::copy_entry_recursive(&self.conn, clipboard.entry_id, target_parent_id);
                // Keep clipboard for repeated pastes
                self.sidebar.clipboard = Some(clipboard);
            }
            ClipboardMode::Cut => {
                let _ = model::move_entry(&self.conn, clipboard.entry_id, target_parent_id);
                // Clear clipboard after cut-paste
            }
        }

        let _ = self.sidebar.reload(&self.conn);
        self.expand_path_to_parent(target_parent_id);
    }

    // ── Content panel key handling ─────────────────────────────────

    /// Handle key events when the content panel is focused in Normal mode.
    fn handle_panel_normal_key(&mut self, key: KeyEvent) -> Action {
        // Handle pending two-key sequences first
        if self.key_state.leader_active {
            self.key_state.leader_active = false;
            return match key.code {
                KeyCode::Char(' ') => Action::ToolPicker,
                KeyCode::Char('f') => Action::Telescope,
                KeyCode::Char('s') => {
                    self.send_request();
                    Action::None
                }
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
                    self.panel_goto_top();
                    Action::None
                }
                ('g', KeyCode::Char('t')) => Action::NextTool,
                ('g', KeyCode::Char('T')) => Action::PrevTool,
                ('d', KeyCode::Char('d')) => {
                    // Delete row in kv sections
                    match self.panel.focused_section {
                        Section::Headers | Section::Params => self.panel.kv_delete_row(),
                        _ => {}
                    }
                    Action::None
                }
                _ => Action::None,
            };
        }

        // Ctrl-Enter sends request from anywhere
        if key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.send_request();
            return Action::None;
        }

        // Ctrl-h/j/k/l for panel-level navigation (not section cycling)
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('j') => {
                    // Move focus from request panel to response panel
                    if self.panel.panel_focus == PanelFocus::Request {
                        self.panel.focus_response();
                    }
                    return Action::None;
                }
                KeyCode::Char('k') => {
                    // Move focus from response panel to request panel,
                    // or from request panel to sidebar
                    if self.panel.panel_focus == PanelFocus::Response {
                        self.panel.focus_request();
                    } else {
                        self.sidebar_focused = true;
                    }
                    return Action::None;
                }
                KeyCode::Char('h') => {
                    // Move focus to sidebar
                    self.sidebar_focused = true;
                    return Action::None;
                }
                KeyCode::Char('l') => {
                    // Already in content panel, no-op
                    return Action::None;
                }
                KeyCode::Char('d') => {
                    // Half page down in response body
                    if self.panel.panel_focus == PanelFocus::Response {
                        if let Some(ref mut resp) = self.panel.response {
                            resp.scroll_body_down(10);
                        }
                    }
                    return Action::None;
                }
                KeyCode::Char('u') => {
                    // Half page up in response body
                    if self.panel.panel_focus == PanelFocus::Response {
                        if let Some(ref mut resp) = self.panel.response {
                            resp.scroll_body_up(10);
                        }
                    }
                    return Action::None;
                }
                _ => {}
            }
        }

        // Toggle fullscreen for the focused panel
        if key.code == KeyCode::Char('f') && key.modifiers.is_empty() {
            self.panel.toggle_fullscreen();
            return Action::None;
        }

        // Response-focused keys
        if self.panel.panel_focus == PanelFocus::Response {
            return self.handle_response_key(key);
        }

        // Request section-specific keys
        match self.panel.focused_section {
            Section::Url => self.handle_url_normal_key(key),
            Section::Params | Section::Headers => self.handle_kv_normal_key(key),
            Section::Body => self.handle_body_normal_key(key),
        }
    }

    fn handle_url_normal_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Char('i') => {
                self.panel.editing = true;
                self.panel.url_cursor_end();
                self.mode = InputMode::Insert;
                Action::None
            }
            KeyCode::Char('I') => {
                self.panel.editing = true;
                self.panel.url_cursor_home();
                self.mode = InputMode::Insert;
                Action::None
            }
            KeyCode::Char('a') => {
                self.panel.editing = true;
                self.panel.url_cursor_end();
                self.mode = InputMode::Insert;
                Action::None
            }
            KeyCode::Char('m') | KeyCode::Char('M') => {
                if key.code == KeyCode::Char('M') {
                    self.panel.cycle_method_backward();
                } else {
                    self.panel.cycle_method_forward();
                }
                Action::None
            }
            KeyCode::Tab => {
                self.panel.next_section();
                Action::None
            }
            KeyCode::BackTab => {
                self.panel.prev_section();
                Action::None
            }
            // Hub-level
            KeyCode::Char(' ') => {
                self.key_state.leader_active = true;
                Action::LeaderKey
            }
            KeyCode::Char('g') => {
                self.key_state.pending_key = Some('g');
                Action::None
            }
            KeyCode::Char(':') => Action::SetMode(InputMode::Command),
            KeyCode::Char('?') => Action::Help,
            KeyCode::Char('q') => Action::Quit,
            _ => Action::None,
        }
    }

    fn handle_kv_normal_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Char('j') => {
                self.panel.kv_move_down();
                Action::None
            }
            KeyCode::Char('k') => {
                self.panel.kv_move_up();
                Action::None
            }
            KeyCode::Char('G') => {
                self.panel.kv_goto_bottom();
                Action::None
            }
            KeyCode::Char('g') => {
                self.key_state.pending_key = Some('g');
                Action::None
            }
            KeyCode::Char('d') => {
                self.key_state.pending_key = Some('d');
                Action::None
            }
            KeyCode::Char('a') => {
                self.panel.kv_add_row();
                self.panel.editing_field = KvField::Key;
                self.panel.kv_start_edit();
                self.mode = InputMode::Insert;
                Action::None
            }
            KeyCode::Char('i') | KeyCode::Enter => {
                self.panel.editing_field = KvField::Key;
                self.panel.kv_start_edit();
                self.mode = InputMode::Insert;
                Action::None
            }
            KeyCode::Char('x') => {
                // Toggle enabled
                self.panel.kv_toggle_enabled();
                Action::None
            }
            KeyCode::Char(' ') => {
                self.key_state.leader_active = true;
                Action::LeaderKey
            }
            KeyCode::Tab => {
                self.panel.next_section();
                Action::None
            }
            KeyCode::BackTab => {
                self.panel.prev_section();
                Action::None
            }
            KeyCode::Char(':') => Action::SetMode(InputMode::Command),
            KeyCode::Char('?') => Action::Help,
            KeyCode::Char('q') => Action::Quit,
            _ => Action::None,
        }
    }

    fn handle_body_normal_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Char('i') => {
                self.panel.editing = true;
                self.mode = InputMode::Insert;
                Action::None
            }
            KeyCode::Char('a') => {
                self.panel.editing = true;
                self.panel.body_cursor_right();
                self.mode = InputMode::Insert;
                Action::None
            }
            KeyCode::Char('A') => {
                self.panel.editing = true;
                self.panel.body_cursor_end();
                self.mode = InputMode::Insert;
                Action::None
            }
            KeyCode::Char('I') => {
                self.panel.editing = true;
                self.panel.body_cursor_home();
                self.mode = InputMode::Insert;
                Action::None
            }
            KeyCode::Char('o') => {
                // Insert line below
                self.panel.body_cursor_end();
                self.panel.body_insert_newline();
                self.panel.editing = true;
                self.mode = InputMode::Insert;
                Action::None
            }
            KeyCode::Char('O') => {
                // Insert line above
                self.panel.body_cursor_home();
                self.panel.body_insert_newline();
                self.panel.body_cursor_up();
                self.panel.editing = true;
                self.mode = InputMode::Insert;
                Action::None
            }
            KeyCode::Char('j') => {
                self.panel.body_cursor_down();
                Action::None
            }
            KeyCode::Char('k') => {
                self.panel.body_cursor_up();
                Action::None
            }
            KeyCode::Char('h') => {
                self.panel.body_cursor_left();
                Action::None
            }
            KeyCode::Char('l') => {
                self.panel.body_cursor_right();
                Action::None
            }
            KeyCode::Char('0') => {
                self.panel.body_cursor_home();
                Action::None
            }
            KeyCode::Char('$') => {
                self.panel.body_cursor_end();
                Action::None
            }
            KeyCode::Char('G') => {
                self.panel.body_goto_bottom();
                Action::None
            }
            KeyCode::Char('g') => {
                self.key_state.pending_key = Some('g');
                Action::None
            }
            KeyCode::Tab => {
                self.panel.next_section();
                Action::None
            }
            KeyCode::BackTab => {
                self.panel.prev_section();
                Action::None
            }
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

    fn handle_response_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Char('j') => {
                if let Some(ref mut resp) = self.panel.response {
                    match resp.focused_section {
                        ResponseSection::Body => resp.scroll_body_down(1),
                        ResponseSection::Headers => resp.scroll_headers_down(1),
                    }
                }
                Action::None
            }
            KeyCode::Char('k') => {
                if let Some(ref mut resp) = self.panel.response {
                    match resp.focused_section {
                        ResponseSection::Body => resp.scroll_body_up(1),
                        ResponseSection::Headers => resp.scroll_headers_up(1),
                    }
                }
                Action::None
            }
            KeyCode::Char('G') => {
                if let Some(ref mut resp) = self.panel.response {
                    match resp.focused_section {
                        ResponseSection::Body => {
                            let max = resp.body_line_count().saturating_sub(1);
                            resp.body_scroll = max;
                        }
                        ResponseSection::Headers => {
                            resp.headers_scroll = resp.headers.len().saturating_sub(1);
                        }
                    }
                }
                Action::None
            }
            KeyCode::Char('g') => {
                self.key_state.pending_key = Some('g');
                Action::None
            }
            KeyCode::Tab | KeyCode::BackTab => {
                if let Some(ref mut resp) = self.panel.response {
                    resp.toggle_section();
                }
                Action::None
            }
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

    /// Handle keys in Insert mode for the content panel.
    fn handle_panel_insert_key(&mut self, key: KeyEvent) -> Action {
        // Ctrl-Enter sends from insert mode too
        if key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.send_request();
            return Action::None;
        }

        match key.code {
            KeyCode::Esc => {
                self.panel.editing = false;
                self.panel.kv_stop_edit();
                self.mode = InputMode::Normal;
                Action::SetMode(InputMode::Normal)
            }
            _ => match self.panel.focused_section {
                Section::Url => self.handle_url_insert_key(key),
                Section::Params | Section::Headers => self.handle_kv_insert_key(key),
                Section::Body => self.handle_body_insert_key(key),
            },
        }
    }

    fn handle_url_insert_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Char(c) => {
                self.panel.url_insert_char(c);
                Action::None
            }
            KeyCode::Backspace => {
                self.panel.url_backspace();
                Action::None
            }
            KeyCode::Delete => {
                self.panel.url_delete();
                Action::None
            }
            KeyCode::Left => {
                self.panel.url_cursor_left();
                Action::None
            }
            KeyCode::Right => {
                self.panel.url_cursor_right();
                Action::None
            }
            KeyCode::Home => {
                self.panel.url_cursor_home();
                Action::None
            }
            KeyCode::End => {
                self.panel.url_cursor_end();
                Action::None
            }
            _ => Action::None,
        }
    }

    fn handle_kv_insert_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Char(c) => {
                self.panel.kv_insert_char(c);
                Action::None
            }
            KeyCode::Backspace => {
                self.panel.kv_backspace();
                Action::None
            }
            KeyCode::Left => {
                self.panel.kv_cursor_left();
                Action::None
            }
            KeyCode::Right => {
                self.panel.kv_cursor_right();
                Action::None
            }
            KeyCode::Tab => {
                self.panel.kv_toggle_field();
                Action::None
            }
            KeyCode::Enter => {
                // Finish editing this row
                self.panel.kv_stop_edit();
                self.panel.editing = false;
                self.mode = InputMode::Normal;
                Action::SetMode(InputMode::Normal)
            }
            _ => Action::None,
        }
    }

    fn handle_body_insert_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Char(c) => {
                self.panel.body_insert_char(c);
                Action::None
            }
            KeyCode::Enter => {
                self.panel.body_insert_newline();
                Action::None
            }
            KeyCode::Backspace => {
                self.panel.body_backspace();
                Action::None
            }
            KeyCode::Delete => {
                self.panel.body_delete();
                Action::None
            }
            KeyCode::Left => {
                self.panel.body_cursor_left();
                Action::None
            }
            KeyCode::Right => {
                self.panel.body_cursor_right();
                Action::None
            }
            KeyCode::Up => {
                self.panel.body_cursor_up();
                Action::None
            }
            KeyCode::Down => {
                self.panel.body_cursor_down();
                Action::None
            }
            KeyCode::Home => {
                self.panel.body_cursor_home();
                Action::None
            }
            KeyCode::End => {
                self.panel.body_cursor_end();
                Action::None
            }
            _ => Action::None,
        }
    }

    // ── Mouse handling helpers ──────────────────────────────────────

    /// Handle a click inside the sidebar area.
    fn handle_sidebar_click(&mut self, mouse: MouseEvent, area: Rect, sidebar_width: u16) {
        let sidebar_area = Rect {
            x: area.x,
            y: area.y,
            width: sidebar_width,
            height: area.height,
        };

        // Account for the sidebar border block (1 line top, 1 left, 1 right)
        let inner_y_start = sidebar_area.y + 1;
        let inner_y_end = sidebar_area.y + sidebar_area.height.saturating_sub(1);

        if mouse.row >= inner_y_start && mouse.row < inner_y_end {
            let row_in_tree = (mouse.row - inner_y_start) as usize;

            // Account for scroll offset (same logic as render_tree_entries)
            let total_items = self.sidebar.flat_view.len() + 1;
            let visible_lines = (inner_y_end - inner_y_start) as usize;
            let scroll_offset = if self.sidebar.selected >= visible_lines {
                self.sidebar.selected - visible_lines + 1
            } else {
                0
            };

            let clicked_idx = scroll_offset + row_in_tree;
            if clicked_idx < total_items {
                // If clicking on a folder that's already selected, toggle expand/collapse
                let was_selected = self.sidebar.selected == clicked_idx;
                self.sidebar.selected = clicked_idx;

                if was_selected {
                    if let Some(entry) = self.sidebar.selected_entry() {
                        if entry.entry_type == EntryType::Folder {
                            self.sidebar.toggle_expand(&self.conn);
                        } else {
                            // Click on already-selected query: open it
                            let id = entry.entry_id;
                            let name = entry.name.clone();
                            self.open_query(id, &name);
                        }
                    }
                }
            }
        }
    }

    /// Handle a click inside the content panel area.
    fn handle_content_click(&mut self, mouse: MouseEvent, area: Rect, sidebar_width: u16) {
        if !self.panel.is_active() {
            return;
        }

        let content_area = Rect {
            x: area.x + sidebar_width,
            y: area.y,
            width: area.width.saturating_sub(sidebar_width),
            height: area.height,
        };

        // Determine request vs response area (30/70 split)
        let request_height = (content_area.height * 30 / 100).max(5);

        // Account for fullscreen mode
        let (in_request, in_response) = match self.panel.fullscreen {
            Some(PanelFocus::Request) => (true, false),
            Some(PanelFocus::Response) => (false, true),
            None => {
                let r = mouse.row < content_area.y + request_height;
                (r, !r)
            }
        };

        if in_request {
            self.panel.panel_focus = PanelFocus::Request;
            self.handle_request_area_click(mouse, content_area, request_height);
        } else if in_response {
            self.panel.panel_focus = PanelFocus::Response;
            self.handle_response_area_click(mouse, content_area, request_height);
        }
    }

    /// Handle a click inside the request area.
    fn handle_request_area_click(
        &mut self,
        mouse: MouseEvent,
        content_area: Rect,
        request_height: u16,
    ) {
        let request_area = match self.panel.fullscreen {
            Some(PanelFocus::Request) => content_area,
            _ => Rect {
                height: request_height,
                ..content_area
            },
        };

        // Request area has a border block. Inner area starts 1 down, 1 in from each side.
        let inner_y = request_area.y + 1;
        if mouse.row < inner_y {
            return;
        }
        let row_in_inner = mouse.row - inner_y;

        // Row 0 = URL bar, Row 1 = section tabs, Row 2+ = section content
        if row_in_inner == 0 {
            // Click on URL bar: focus URL section
            self.panel.focused_section = Section::Url;
        } else if row_in_inner == 1 {
            // Click on section tabs: determine which tab
            let inner_x = request_area.x + 1;
            let col_in_tabs = mouse.column.saturating_sub(inner_x);
            // Tabs layout: " Params │ Headers │ Body"
            // " " = 1, "Params" = 6, " │ " = 3, "Headers" = 7, " │ " = 3, "Body" = 4
            if col_in_tabs < 7 {
                // " Params" region
                self.panel.focused_section = Section::Params;
            } else if col_in_tabs < 17 {
                // " │ Headers" region
                self.panel.focused_section = Section::Headers;
            } else {
                // " │ Body" region
                self.panel.focused_section = Section::Body;
            }
        } else {
            // Click in section content area: select KV row if in Params/Headers
            let content_row = (row_in_inner - 2) as usize;
            match self.panel.focused_section {
                Section::Params => {
                    if content_row < self.panel.query_params.len() {
                        // Account for scroll offset
                        let visible_lines = request_area.height.saturating_sub(4) as usize; // -2 border -2 url+tabs
                        let scroll_offset = if self.panel.params_selected >= visible_lines {
                            self.panel.params_selected - visible_lines + 1
                        } else {
                            0
                        };
                        let clicked_idx = scroll_offset + content_row;
                        if clicked_idx < self.panel.query_params.len() {
                            self.panel.params_selected = clicked_idx;
                        }
                    }
                }
                Section::Headers => {
                    let visible_lines = request_area.height.saturating_sub(4) as usize;
                    let scroll_offset = if self.panel.headers_selected >= visible_lines {
                        self.panel.headers_selected - visible_lines + 1
                    } else {
                        0
                    };
                    let clicked_idx = scroll_offset + content_row;
                    if clicked_idx < self.panel.headers.len() {
                        self.panel.headers_selected = clicked_idx;
                    }
                }
                _ => {}
            }
        }
    }

    /// Handle a click inside the response area.
    fn handle_response_area_click(
        &mut self,
        mouse: MouseEvent,
        content_area: Rect,
        request_height: u16,
    ) {
        if self.panel.response.is_none() {
            return;
        }

        let response_area = match self.panel.fullscreen {
            Some(PanelFocus::Response) => content_area,
            _ => Rect {
                y: content_area.y + request_height,
                height: content_area.height.saturating_sub(request_height),
                ..content_area
            },
        };

        // Response area has a border. Inner starts 1 down.
        let inner_y = response_area.y + 1;
        if mouse.row < inner_y {
            return;
        }
        let row_in_inner = mouse.row - inner_y;

        // Row 0 = status line, Row 1 = response tabs (Body | Headers), Row 2+ = content
        if row_in_inner == 1 {
            // Click on response tabs
            if let Some(ref mut resp) = self.panel.response {
                let inner_x = response_area.x + 1;
                let col = mouse.column.saturating_sub(inner_x);
                // " Body │ Headers (N)"
                // " " = 1, "Body" = 4, " │ " = 3 => Headers starts at ~8
                if col < 6 {
                    resp.focused_section = ResponseSection::Body;
                } else {
                    resp.focused_section = ResponseSection::Headers;
                }
            }
        }
    }

    /// Handle scroll down in the content panel (delegates to the focused section).
    fn handle_content_scroll_down(&mut self) {
        match self.panel.panel_focus {
            PanelFocus::Request => match self.panel.focused_section {
                Section::Params => self.panel.kv_move_down(),
                Section::Headers => self.panel.kv_move_down(),
                Section::Body => self.panel.body_cursor_down(),
                Section::Url => {}
            },
            PanelFocus::Response => {
                if let Some(ref mut resp) = self.panel.response {
                    match resp.focused_section {
                        ResponseSection::Body => resp.scroll_body_down(3),
                        ResponseSection::Headers => resp.scroll_headers_down(1),
                    }
                }
            }
        }
    }

    /// Handle scroll up in the content panel (delegates to the focused section).
    fn handle_content_scroll_up(&mut self) {
        match self.panel.panel_focus {
            PanelFocus::Request => match self.panel.focused_section {
                Section::Params => self.panel.kv_move_up(),
                Section::Headers => self.panel.kv_move_up(),
                Section::Body => self.panel.body_cursor_up(),
                Section::Url => {}
            },
            PanelFocus::Response => {
                if let Some(ref mut resp) = self.panel.response {
                    match resp.focused_section {
                        ResponseSection::Body => resp.scroll_body_up(3),
                        ResponseSection::Headers => resp.scroll_headers_up(1),
                    }
                }
            }
        }
    }

    fn panel_goto_top(&mut self) {
        match self.panel.panel_focus {
            PanelFocus::Request => match self.panel.focused_section {
                Section::Headers | Section::Params => self.panel.kv_goto_top(),
                Section::Body => self.panel.body_goto_top(),
                _ => {}
            },
            PanelFocus::Response => {
                if let Some(ref mut resp) = self.panel.response {
                    match resp.focused_section {
                        ResponseSection::Body => resp.body_scroll = 0,
                        ResponseSection::Headers => resp.headers_scroll = 0,
                    }
                }
            }
        }
    }

    /// Select and open a query by entry ID from telescope.
    fn select_query_by_entry_id(&mut self, entry_id: i64) -> bool {
        let Some((name, entry_type)) = sidebar::find_node(&self.sidebar.roots, entry_id)
            .map(|n| (n.entry.name.clone(), n.entry.entry_type))
        else {
            return false;
        };

        if entry_type != EntryType::Query {
            return false;
        }

        let mut ancestors = Vec::new();
        let mut current = entry_id;
        while let Some(parent_id) = sidebar::find_parent_id(&self.sidebar.roots, current) {
            ancestors.push(parent_id);
            current = parent_id;
        }

        for ancestor_id in ancestors.into_iter().rev() {
            if let Some(node) = sidebar::find_node_mut(&mut self.sidebar.roots, ancestor_id) {
                if !node.expanded {
                    node.expanded = true;
                    let _ = model::set_entry_expanded(&self.conn, ancestor_id, true);
                }
            }
        }

        self.sidebar.rebuild_flat_view();
        if let Some(position) = self
            .sidebar
            .flat_view
            .iter()
            .position(|entry| entry.entry_id == entry_id)
        {
            self.sidebar.selected = position;
        }

        self.open_query(entry_id, &name);
        true
    }

    /// Collect all queries as telescope items.
    fn collect_telescope_items(&self) -> Vec<TelescopeItem> {
        let mut items = Vec::new();
        self.collect_items_recursive(&self.sidebar.roots, "", &mut items);
        items
    }

    fn collect_items_recursive(
        &self,
        nodes: &[sidebar::TreeNode],
        path_prefix: &str,
        items: &mut Vec<TelescopeItem>,
    ) {
        for node in nodes {
            let full_path = if path_prefix.is_empty() {
                node.entry.name.clone()
            } else {
                format!("{}/{}", path_prefix, node.entry.name)
            };

            if node.entry.entry_type == EntryType::Query {
                items.push(TelescopeItem {
                    label: node.entry.name.clone(),
                    description: full_path.clone(),
                    id: format!("http:{}", node.entry.id),
                });
            }

            self.collect_items_recursive(&node.children, &full_path, items);
        }
    }
}

impl Tool for HttpTool {
    fn name(&self) -> &str {
        "HTTP"
    }

    fn description(&self) -> &str {
        "HTTP client & API explorer"
    }

    fn mode(&self) -> InputMode {
        self.mode
    }

    fn init_db(&self, conn: &Connection) -> anyhow::Result<()> {
        model::init_db(conn)
    }

    fn which_key_entries(&self) -> Vec<WhichKeyEntry> {
        vec![
            WhichKeyEntry::action('s', "Send request"),
            WhichKeyEntry::action('e', "Toggle sidebar"),
            WhichKeyEntry::action('m', "Cycle method"),
        ]
    }

    fn telescope_items(&self) -> Vec<TelescopeItem> {
        self.collect_telescope_items()
    }

    fn handle_telescope_selection(&mut self, id: &str) -> bool {
        let Some(raw_id) = id.strip_prefix("http:") else {
            return false;
        };

        let Ok(entry_id) = raw_id.parse::<i64>() else {
            return false;
        };

        self.select_query_by_entry_id(entry_id)
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
            HelpEntry::with_section("Sidebar", "l / Enter", "Expand folder / open query"),
            HelpEntry::with_section("Sidebar", "j / k", "Navigate up / down"),
            HelpEntry::with_section("Sidebar", "gg / G", "Go to top / bottom"),
            HelpEntry::with_section("Sidebar", "Ctrl-l", "Move focus to content panel"),
            // Request Panel
            HelpEntry::with_section(
                "Request",
                "Tab / S-Tab",
                "Cycle sections (URL/Params/Headers/Body)",
            ),
            HelpEntry::with_section("Request", "Ctrl-h/j/k/l", "Navigate between panels"),
            HelpEntry::with_section("Request", "Ctrl-Enter", "Send request"),
            HelpEntry::with_section("Request", "f", "Toggle fullscreen panel"),
            HelpEntry::with_section("Request", "<Space>s", "Send request"),
            HelpEntry::with_section("Request", ":w", "Save request to database"),
            HelpEntry::with_section("Request", "m / M", "Cycle method forward / backward"),
            // URL section
            HelpEntry::with_section("URL", "i / a", "Edit URL"),
            // Params / Headers
            HelpEntry::with_section("Key-Value", "a", "Add new row"),
            HelpEntry::with_section("Key-Value", "i / Enter", "Edit selected row"),
            HelpEntry::with_section("Key-Value", "dd", "Delete selected row"),
            HelpEntry::with_section("Key-Value", "x", "Toggle row enabled/disabled"),
            HelpEntry::with_section("Key-Value", "Tab (edit)", "Switch between key/value fields"),
            // Body
            HelpEntry::with_section("Body", "i / a / A / I", "Enter insert mode"),
            HelpEntry::with_section("Body", "o / O", "Insert line below / above"),
            HelpEntry::with_section("Body", "hjkl", "Cursor movement"),
            // Response
            HelpEntry::with_section("Response", "j / k", "Scroll response"),
            HelpEntry::with_section("Response", "gg / G", "Go to top / bottom"),
            HelpEntry::with_section("Response", "Tab", "Switch Body / Headers"),
            // General
            HelpEntry::with_section("General", "<Space>e", "Toggle explorer sidebar"),
        ]
    }

    fn handle_key(&mut self, key: KeyEvent) -> Action {
        match self.mode {
            InputMode::Normal => {
                if self.sidebar.visible && self.sidebar_focused {
                    // Sidebar is focused
                    if self.sidebar.input_mode == SidebarInput::ConfirmDelete {
                        return self.handle_confirm_delete_key(key);
                    }

                    // Ctrl-hjkl panel navigation from sidebar
                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        match key.code {
                            KeyCode::Char('l') => {
                                if self.panel.is_active() {
                                    self.sidebar_focused = false;
                                }
                                return Action::None;
                            }
                            // Ctrl-j/k/h are no-ops in the sidebar — consume them
                            // so they don't trigger j/k/h navigation.
                            KeyCode::Char('j') | KeyCode::Char('k') | KeyCode::Char('h') => {
                                return Action::None;
                            }
                            _ => {}
                        }
                    }

                    self.handle_sidebar_normal_key(key)
                } else if self.panel.is_active() {
                    // Content panel is focused
                    self.handle_panel_normal_key(key)
                } else {
                    // No panel active, handle basic navigation
                    let action = process_normal_key(key, &mut self.key_state);
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
                    self.handle_panel_insert_key(key)
                }
            }
            InputMode::Command => {
                // Command mode is handled by the hub
                Action::None
            }
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent, area: Rect) -> Action {
        // Don't handle mouse in Insert mode (except scroll)
        let is_scroll = matches!(
            mouse.kind,
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp
        );

        if self.mode == InputMode::Insert && !is_scroll {
            return Action::None;
        }

        // Determine sidebar vs content area boundaries
        let sidebar_width = if self.sidebar.visible {
            ui::SIDEBAR_WIDTH.min(area.width.saturating_sub(10))
        } else {
            0
        };

        let in_sidebar = self.sidebar.visible && mouse.column < area.x + sidebar_width;
        let in_content = mouse.column >= area.x + sidebar_width
            && mouse.column < area.x + area.width
            && mouse.row >= area.y
            && mouse.row < area.y + area.height;

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if in_sidebar {
                    // Click in sidebar: focus it and select entry
                    self.sidebar_focused = true;
                    self.handle_sidebar_click(mouse, area, sidebar_width);
                } else if in_content {
                    // Click in content: focus it
                    self.sidebar_focused = false;
                    self.handle_content_click(mouse, area, sidebar_width);
                }
                Action::None
            }
            MouseEventKind::ScrollDown => {
                if in_sidebar && self.sidebar_focused {
                    self.sidebar.move_down();
                } else if in_content || (in_sidebar && !self.sidebar_focused) {
                    self.handle_content_scroll_down();
                }
                Action::None
            }
            MouseEventKind::ScrollUp => {
                if in_sidebar && self.sidebar_focused {
                    self.sidebar.move_up();
                } else if in_content || (in_sidebar && !self.sidebar_focused) {
                    self.handle_content_scroll_up();
                }
                Action::None
            }
            _ => Action::None,
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
                self.send_request();
                Some(Action::None)
            }
            _ => None,
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        ui::render_http_tool(
            frame,
            area,
            &self.sidebar,
            &self.panel,
            self.sidebar_focused,
        );
    }

    fn tick(&mut self) {
        self.poll_response();
        self.panel.tick_spinner();
    }

    fn reset_key_state(&mut self) {
        self.key_state.reset();
    }

    fn on_focus(&mut self) {
        let _ = self.sidebar.reload(&self.conn);
    }

    fn handle_command(&mut self, cmd: &str) -> bool {
        match cmd.trim() {
            "w" | "write" => self.save_panel(),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstools_core::db::open_memory_db;

    fn setup_tool() -> HttpTool {
        let conn = open_memory_db().unwrap();
        HttpTool::new(conn).unwrap()
    }

    #[test]
    fn test_create_simple_query() {
        let mut tool = setup_tool();
        tool.create_entries_from_path("get-users");

        assert_eq!(tool.sidebar.flat_view.len(), 1);
        assert_eq!(tool.sidebar.flat_view[0].name, "get-users");
        assert_eq!(tool.sidebar.flat_view[0].entry_type, EntryType::Query);
    }

    #[test]
    fn test_create_nested_path() {
        let mut tool = setup_tool();
        tool.create_entries_from_path("group A/api/get-user");

        // All folders should be expanded to show the new entry
        assert_eq!(tool.sidebar.flat_view.len(), 3);
        assert_eq!(tool.sidebar.flat_view[0].name, "group A");
        assert_eq!(tool.sidebar.flat_view[0].entry_type, EntryType::Folder);
        assert_eq!(tool.sidebar.flat_view[0].depth, 0);
        assert_eq!(tool.sidebar.flat_view[1].name, "api");
        assert_eq!(tool.sidebar.flat_view[1].entry_type, EntryType::Folder);
        assert_eq!(tool.sidebar.flat_view[1].depth, 1);
        assert_eq!(tool.sidebar.flat_view[2].name, "get-user");
        assert_eq!(tool.sidebar.flat_view[2].entry_type, EntryType::Query);
        assert_eq!(tool.sidebar.flat_view[2].depth, 2);
    }

    #[test]
    fn test_create_folder_only_with_trailing_slash() {
        let mut tool = setup_tool();
        tool.create_entries_from_path("group A/api/");

        assert_eq!(tool.sidebar.flat_view.len(), 2);
        assert_eq!(tool.sidebar.flat_view[0].name, "group A");
        assert_eq!(tool.sidebar.flat_view[0].entry_type, EntryType::Folder);
        assert_eq!(tool.sidebar.flat_view[1].name, "api");
        assert_eq!(tool.sidebar.flat_view[1].entry_type, EntryType::Folder);
    }

    #[test]
    fn test_create_reuses_existing_folders() {
        let mut tool = setup_tool();
        // Create first entry — creates "api" folder + "get-users" query
        tool.create_entries_from_path("api/get-users");

        // Select the "api" folder (it's expanded after creation), so new entries
        // go inside it. Just type the query name since we're already in "api/".
        tool.sidebar.selected = 0; // "api" folder (expanded)
        tool.create_entries_from_path("post-user");

        // Should not create a second "api" folder
        let entries = model::list_entries(&tool.conn).unwrap();
        let folders: Vec<_> = entries
            .iter()
            .filter(|e| e.entry_type == EntryType::Folder)
            .collect();
        assert_eq!(folders.len(), 1); // Only one "api" folder
        assert_eq!(folders[0].name, "api");

        let queries: Vec<_> = entries
            .iter()
            .filter(|e| e.entry_type == EntryType::Query)
            .collect();
        assert_eq!(queries.len(), 2);
    }

    #[test]
    fn test_create_from_root_reuses_folders() {
        let mut tool = setup_tool();
        tool.create_entries_from_path("api/get-users");

        // Collapse "api" folder so we're at root level context
        tool.sidebar.selected = 0;
        tool.sidebar.collapse_or_parent(&tool.conn); // collapse the api folder

        // Now create another entry in same "api" folder from root context
        tool.create_entries_from_path("api/post-user");

        let entries = model::list_entries(&tool.conn).unwrap();
        let folders: Vec<_> = entries
            .iter()
            .filter(|e| e.entry_type == EntryType::Folder)
            .collect();
        assert_eq!(folders.len(), 1); // Only one "api" folder

        let queries: Vec<_> = entries
            .iter()
            .filter(|e| e.entry_type == EntryType::Query)
            .collect();
        assert_eq!(queries.len(), 2);
    }

    #[test]
    fn test_rename_entry() {
        let mut tool = setup_tool();
        tool.create_entries_from_path("get-users");

        // Select the entry and rename it
        tool.sidebar.selected = 0;
        tool.sidebar.start_rename();
        assert_eq!(tool.sidebar.input_mode, SidebarInput::Renaming);
        assert_eq!(tool.sidebar.input_buffer, "get-users");

        // Clear and type new name
        tool.sidebar.input_buffer = "list-users".to_string();
        tool.sidebar.input_cursor = tool.sidebar.input_buffer.len();

        // Submit rename
        let entry_id = tool.sidebar.selected_entry_id().unwrap();
        model::rename_entry(&tool.conn, entry_id, &tool.sidebar.input_buffer).unwrap();
        tool.sidebar.reload(&tool.conn).unwrap();

        assert_eq!(tool.sidebar.flat_view[0].name, "list-users");
    }

    #[test]
    fn test_delete_entry() {
        let mut tool = setup_tool();
        tool.create_entries_from_path("api/get-users");
        tool.create_entries_from_path("api/post-user");

        // Select the folder and delete it
        tool.sidebar.selected = 0;
        tool.execute_delete();

        assert_eq!(tool.sidebar.flat_view.len(), 0);
        let entries = model::list_entries(&tool.conn).unwrap();
        assert_eq!(entries.len(), 0); // All children deleted by cascade
    }

    #[test]
    fn test_copy_paste() {
        let mut tool = setup_tool();
        // Create entries at root level (nothing selected initially)
        tool.create_entries_from_path("api/get-users");

        // Collapse api so we create backup at root level
        tool.sidebar.selected = 0;
        tool.sidebar.collapse_or_parent(&tool.conn);

        tool.create_entries_from_path("backup/");

        // Select "api" folder and copy it
        let api_idx = tool
            .sidebar
            .flat_view
            .iter()
            .position(|e| e.name == "api")
            .unwrap();
        tool.sidebar.selected = api_idx;
        tool.sidebar.copy_selected();
        assert!(tool.sidebar.clipboard.is_some());

        // Navigate to "backup" folder and paste
        let backup_idx = tool
            .sidebar
            .flat_view
            .iter()
            .position(|e| e.name == "backup")
            .unwrap();
        tool.sidebar.selected = backup_idx;
        tool.execute_paste();

        // Verify: "backup" should now contain a copy of "api" with "get-users"
        let entries = model::list_entries(&tool.conn).unwrap();
        let api_folders: Vec<_> = entries
            .iter()
            .filter(|e| e.name == "api" && e.entry_type == EntryType::Folder)
            .collect();
        assert_eq!(api_folders.len(), 2); // Original + copy

        // Clipboard should still be available after copy-paste
        assert!(tool.sidebar.clipboard.is_some());
    }

    #[test]
    fn test_cut_paste() {
        let mut tool = setup_tool();
        tool.create_entries_from_path("api/get-users");

        // Collapse api so we create backup at root level
        tool.sidebar.selected = 0;
        tool.sidebar.collapse_or_parent(&tool.conn);

        tool.create_entries_from_path("backup/");

        // Find and select the "get-users" query (need to expand api first)
        let api_idx = tool
            .sidebar
            .flat_view
            .iter()
            .position(|e| e.name == "api")
            .unwrap();
        tool.sidebar.selected = api_idx;
        tool.sidebar.expand_selected(&tool.conn);

        let query_idx = tool
            .sidebar
            .flat_view
            .iter()
            .position(|e| e.name == "get-users")
            .unwrap();
        tool.sidebar.selected = query_idx;
        tool.sidebar.cut_selected();

        // Navigate to "backup" folder and paste
        let backup_idx = tool
            .sidebar
            .flat_view
            .iter()
            .position(|e| e.name == "backup")
            .unwrap();
        tool.sidebar.selected = backup_idx;
        tool.execute_paste();

        // Verify: "get-users" should now be under "backup"
        let entries = model::list_entries(&tool.conn).unwrap();
        let backup_folder = entries.iter().find(|e| e.name == "backup").unwrap();
        let get_users = entries.iter().find(|e| e.name == "get-users").unwrap();
        assert_eq!(get_users.parent_id, Some(backup_folder.id));

        // Clipboard should be cleared after cut-paste
        assert!(tool.sidebar.clipboard.is_none());
    }

    #[test]
    fn test_telescope_items() {
        let mut tool = setup_tool();
        tool.create_entries_from_path("api/get-users");
        tool.create_entries_from_path("api/post-user");
        tool.create_entries_from_path("health-check");

        let items = tool.collect_telescope_items();
        // Only queries should appear (not folders)
        assert_eq!(items.len(), 3);

        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"get-users"));
        assert!(labels.contains(&"post-user"));
        assert!(labels.contains(&"health-check"));
    }
}
