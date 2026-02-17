pub mod model;
pub mod sidebar;
pub mod ui;

use rstools_core::help_popup::HelpEntry;
use rstools_core::keybinds::{process_normal_key, Action, InputMode, KeyState};
use rstools_core::telescope::TelescopeItem;
use rstools_core::tool::Tool;
use rstools_core::which_key::WhichKeyEntry;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{layout::Rect, Frame};
use rusqlite::Connection;

use model::EntryType;
use sidebar::{ClipboardMode, SidebarInput, SidebarState};

pub struct HttpTool {
    sidebar: SidebarState,
    mode: InputMode,
    key_state: KeyState,
    conn: Connection,
}

impl HttpTool {
    pub fn new(conn: Connection) -> anyhow::Result<Self> {
        model::init_db(&conn)?;
        let mut sidebar = SidebarState::new();
        sidebar.reload(&conn)?;
        Ok(Self {
            sidebar,
            mode: InputMode::Normal,
            key_state: KeyState::default(),
            conn,
        })
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
                self.sidebar.collapse_or_parent();
                Action::None
            }
            KeyCode::Char('l') | KeyCode::Enter => {
                // Expand folder, or select query
                if let Some(entry) = self.sidebar.selected_entry() {
                    if entry.entry_type == EntryType::Folder {
                        self.sidebar.expand_selected();
                    }
                    // For queries, this would open them in the main panel (future)
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
            WhichKeyEntry::action("e", "Toggle explorer"),
            WhichKeyEntry::action("a", "Add entry"),
            WhichKeyEntry::action("r", "Rename entry"),
            WhichKeyEntry::action("d", "Delete entry"),
        ]
    }

    fn telescope_items(&self) -> Vec<TelescopeItem> {
        self.collect_telescope_items()
    }

    fn help_entries(&self) -> Vec<HelpEntry> {
        vec![
            HelpEntry::with_section("HTTP Explorer", "a", "Add entry (path with / for nesting)"),
            HelpEntry::with_section("HTTP Explorer", "r", "Rename selected entry"),
            HelpEntry::with_section("HTTP Explorer", "d", "Delete selected entry"),
            HelpEntry::with_section("HTTP Explorer", "y", "Copy selected entry"),
            HelpEntry::with_section("HTTP Explorer", "x", "Cut selected entry"),
            HelpEntry::with_section("HTTP Explorer", "p", "Paste entry"),
            HelpEntry::with_section("HTTP Explorer", "h", "Collapse folder / go to parent"),
            HelpEntry::with_section("HTTP Explorer", "l / Enter", "Expand folder"),
            HelpEntry::with_section("HTTP Explorer", "j / k", "Navigate up / down"),
            HelpEntry::with_section("HTTP Explorer", "gg / G", "Go to top / bottom"),
            HelpEntry::with_section("HTTP Explorer", "<Space>e", "Toggle explorer sidebar"),
        ]
    }

    fn handle_key(&mut self, key: KeyEvent) -> Action {
        // If sidebar is not visible, only handle leader key to re-open it
        if !self.sidebar.visible {
            let action = process_normal_key(key, &mut self.key_state);
            return match action {
                // Bubble hub-level actions
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
            };
        }

        match self.mode {
            InputMode::Normal => {
                // Check if in confirm delete mode
                if self.sidebar.input_mode == SidebarInput::ConfirmDelete {
                    return self.handle_confirm_delete_key(key);
                }
                self.handle_sidebar_normal_key(key)
            }
            InputMode::Insert => self.handle_sidebar_insert_key(key),
            InputMode::Command => {
                // Command mode is handled by the hub
                Action::None
            }
        }
    }

    fn handle_leader_action(&mut self, key: char) -> Option<Action> {
        match key {
            'e' => {
                self.sidebar.visible = !self.sidebar.visible;
                Some(Action::None)
            }
            'a' => {
                if self.sidebar.visible {
                    self.sidebar.start_add();
                    self.mode = InputMode::Insert;
                }
                Some(Action::None)
            }
            'd' => {
                if self.sidebar.visible {
                    self.sidebar.start_delete();
                }
                Some(Action::None)
            }
            'r' => {
                if self.sidebar.visible {
                    self.sidebar.start_rename();
                    if self.sidebar.input_mode == SidebarInput::Renaming {
                        self.mode = InputMode::Insert;
                    }
                }
                Some(Action::None)
            }
            _ => None,
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        ui::render_http_tool(frame, area, &self.sidebar);
    }

    fn reset_key_state(&mut self) {
        self.key_state.reset();
    }

    fn on_focus(&mut self) {
        let _ = self.sidebar.reload(&self.conn);
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
        tool.sidebar.collapse_or_parent(); // collapse the api folder

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
        tool.sidebar.collapse_or_parent();

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
        tool.sidebar.collapse_or_parent();

        tool.create_entries_from_path("backup/");

        // Find and select the "get-users" query (need to expand api first)
        let api_idx = tool
            .sidebar
            .flat_view
            .iter()
            .position(|e| e.name == "api")
            .unwrap();
        tool.sidebar.selected = api_idx;
        tool.sidebar.expand_selected();

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
