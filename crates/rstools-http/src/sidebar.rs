use crate::model::{EntryType, HttpEntry};
use anyhow::Result;
use rusqlite::Connection;

/// A node in the in-memory tree representation.
#[derive(Debug, Clone)]
pub struct TreeNode {
    pub entry: HttpEntry,
    pub children: Vec<TreeNode>,
    pub expanded: bool,
}

/// A flattened entry for rendering — one visible line in the sidebar.
#[derive(Debug, Clone)]
pub struct FlatEntry {
    pub entry_id: i64,
    pub depth: usize,
    pub name: String,
    pub entry_type: EntryType,
    pub is_expanded: bool,
    pub has_children: bool,
}

/// Clipboard operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardMode {
    Copy,
    Cut,
}

/// Item stored in the clipboard.
#[derive(Debug, Clone)]
pub struct ClipboardItem {
    pub entry_id: i64,
    pub mode: ClipboardMode,
}

/// What kind of input the sidebar is currently waiting for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidebarInput {
    /// No input active.
    None,
    /// Adding a new entry (path string).
    Adding,
    /// Renaming the selected entry.
    Renaming,
    /// Confirming deletion of an entry.
    ConfirmDelete,
}

/// The full sidebar state.
pub struct SidebarState {
    /// Tree roots (top-level entries).
    pub roots: Vec<TreeNode>,
    /// Flattened visible entries for rendering and navigation.
    pub flat_view: Vec<FlatEntry>,
    /// Currently selected index into flat_view.
    pub selected: usize,
    /// Clipboard for copy/cut/paste.
    pub clipboard: Option<ClipboardItem>,
    /// Current input mode for the sidebar.
    pub input_mode: SidebarInput,
    /// Text input buffer.
    pub input_buffer: String,
    /// Cursor position in the input buffer.
    pub input_cursor: usize,
    /// Whether the sidebar is visible.
    pub visible: bool,
}

impl SidebarState {
    pub fn new() -> Self {
        Self {
            roots: Vec::new(),
            flat_view: Vec::new(),
            selected: 0,
            clipboard: None,
            input_mode: SidebarInput::None,
            input_buffer: String::new(),
            input_cursor: 0,
            visible: true,
        }
    }

    /// Reload the tree from the database.
    pub fn reload(&mut self, conn: &Connection) -> Result<()> {
        let entries = crate::model::list_entries(conn)?;
        self.roots = build_tree(&entries, None);
        sort_tree(&mut self.roots);
        self.rebuild_flat_view();
        Ok(())
    }

    /// Rebuild the flat_view from the current tree state, preserving selection if possible.
    pub fn rebuild_flat_view(&mut self) {
        let old_id = self.selected_entry_id();
        self.flat_view.clear();
        flatten_tree(&self.roots, 0, &mut self.flat_view);

        // Try to restore selection by entry ID
        if let Some(id) = old_id {
            if let Some(pos) = self.flat_view.iter().position(|e| e.entry_id == id) {
                self.selected = pos;
                return;
            }
        }

        // Clamp selection
        if !self.flat_view.is_empty() && self.selected >= self.flat_view.len() {
            self.selected = self.flat_view.len() - 1;
        }
    }

    /// Get the currently selected flat entry, if any.
    pub fn selected_entry(&self) -> Option<&FlatEntry> {
        self.flat_view.get(self.selected)
    }

    /// Get the entry ID of the currently selected entry.
    pub fn selected_entry_id(&self) -> Option<i64> {
        self.selected_entry().map(|e| e.entry_id)
    }

    /// Move selection down.
    pub fn move_down(&mut self) {
        if !self.flat_view.is_empty() && self.selected < self.flat_view.len() - 1 {
            self.selected += 1;
        }
    }

    /// Move selection up.
    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Go to the top of the list.
    pub fn goto_top(&mut self) {
        self.selected = 0;
    }

    /// Go to the bottom of the list.
    pub fn goto_bottom(&mut self) {
        if !self.flat_view.is_empty() {
            self.selected = self.flat_view.len() - 1;
        }
    }

    /// Half-page down.
    pub fn half_page_down(&mut self, visible_lines: usize) {
        let half = visible_lines / 2;
        self.selected = (self.selected + half).min(self.flat_view.len().saturating_sub(1));
    }

    /// Half-page up.
    pub fn half_page_up(&mut self, visible_lines: usize) {
        let half = visible_lines / 2;
        self.selected = self.selected.saturating_sub(half);
    }

    /// Toggle expansion of the selected folder.
    /// Returns true if the entry was a folder that was toggled.
    pub fn toggle_expand(&mut self) -> bool {
        if let Some(entry) = self.selected_entry() {
            if entry.entry_type == EntryType::Folder {
                let entry_id = entry.entry_id;
                if let Some(node) = find_node_mut(&mut self.roots, entry_id) {
                    node.expanded = !node.expanded;
                    self.rebuild_flat_view();
                    return true;
                }
            }
        }
        false
    }

    /// Expand the selected folder (no-op if already expanded or not a folder).
    pub fn expand_selected(&mut self) -> bool {
        if let Some(entry) = self.selected_entry() {
            if entry.entry_type == EntryType::Folder && !entry.is_expanded {
                let entry_id = entry.entry_id;
                if let Some(node) = find_node_mut(&mut self.roots, entry_id) {
                    node.expanded = true;
                    self.rebuild_flat_view();
                    return true;
                }
            }
        }
        false
    }

    /// Collapse the selected folder, or move to parent if already collapsed or a query.
    pub fn collapse_or_parent(&mut self) {
        if let Some(entry) = self.selected_entry() {
            let entry_id = entry.entry_id;

            // If it's an expanded folder, collapse it
            if entry.entry_type == EntryType::Folder && entry.is_expanded {
                if let Some(node) = find_node_mut(&mut self.roots, entry_id) {
                    node.expanded = false;
                    self.rebuild_flat_view();
                    return;
                }
            }

            // Otherwise, move to parent
            let parent_id = find_parent_id(&self.roots, entry_id);
            if let Some(pid) = parent_id {
                if let Some(pos) = self.flat_view.iter().position(|e| e.entry_id == pid) {
                    self.selected = pos;
                }
            }
        }
    }

    /// Start the "add entry" input mode.
    pub fn start_add(&mut self) {
        self.input_mode = SidebarInput::Adding;
        self.input_buffer.clear();
        self.input_cursor = 0;
    }

    /// Start the "rename entry" input mode, pre-filled with current name.
    pub fn start_rename(&mut self) {
        let name = self.selected_entry().map(|e| e.name.clone());
        if let Some(name) = name {
            self.input_mode = SidebarInput::Renaming;
            self.input_cursor = name.len();
            self.input_buffer = name;
        }
    }

    /// Start the delete confirmation.
    pub fn start_delete(&mut self) {
        if self.selected_entry().is_some() {
            self.input_mode = SidebarInput::ConfirmDelete;
            self.input_buffer.clear();
            self.input_cursor = 0;
        }
    }

    /// Cancel any active input.
    pub fn cancel_input(&mut self) {
        self.input_mode = SidebarInput::None;
        self.input_buffer.clear();
        self.input_cursor = 0;
    }

    /// Copy the selected entry to clipboard.
    pub fn copy_selected(&mut self) {
        if let Some(entry) = self.selected_entry() {
            self.clipboard = Some(ClipboardItem {
                entry_id: entry.entry_id,
                mode: ClipboardMode::Copy,
            });
        }
    }

    /// Cut the selected entry to clipboard.
    pub fn cut_selected(&mut self) {
        if let Some(entry) = self.selected_entry() {
            self.clipboard = Some(ClipboardItem {
                entry_id: entry.entry_id,
                mode: ClipboardMode::Cut,
            });
        }
    }

    /// Get the parent_id for pasting: if selected entry is a folder, paste inside it;
    /// otherwise paste in the same parent as the selected entry.
    pub fn paste_target_parent_id(&self) -> Option<i64> {
        if let Some(entry) = self.selected_entry() {
            if entry.entry_type == EntryType::Folder {
                Some(entry.entry_id)
            } else {
                // Find the parent of the selected entry
                find_parent_id(&self.roots, entry.entry_id)
            }
        } else {
            None // Root level
        }
    }

    /// Insert a character into the input buffer at the cursor position.
    pub fn input_insert_char(&mut self, c: char) {
        self.input_buffer.insert(self.input_cursor, c);
        self.input_cursor += c.len_utf8();
    }

    /// Delete the character before the cursor in the input buffer.
    pub fn input_backspace(&mut self) {
        if self.input_cursor > 0 {
            // Find the previous character boundary
            let prev = self.input_buffer[..self.input_cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.input_buffer.drain(prev..self.input_cursor);
            self.input_cursor = prev;
        }
    }

    /// Move cursor left in the input buffer.
    pub fn input_cursor_left(&mut self) {
        if self.input_cursor > 0 {
            self.input_cursor = self.input_buffer[..self.input_cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
    }

    /// Move cursor right in the input buffer.
    pub fn input_cursor_right(&mut self) {
        if self.input_cursor < self.input_buffer.len() {
            self.input_cursor = self.input_buffer[self.input_cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.input_cursor + i)
                .unwrap_or(self.input_buffer.len());
        }
    }
}

/// Build a tree from a flat list of entries, starting from entries with the given parent_id.
fn build_tree(entries: &[HttpEntry], parent_id: Option<i64>) -> Vec<TreeNode> {
    entries
        .iter()
        .filter(|e| e.parent_id == parent_id)
        .map(|e| {
            let children = build_tree(entries, Some(e.id));
            TreeNode {
                entry: e.clone(),
                children,
                expanded: false,
            }
        })
        .collect()
}

/// Sort tree nodes: folders first, then queries, both alphabetically. Recursive.
fn sort_tree(nodes: &mut Vec<TreeNode>) {
    nodes.sort_by(|a, b| {
        let type_ord = match (&a.entry.entry_type, &b.entry.entry_type) {
            (EntryType::Folder, EntryType::Query) => std::cmp::Ordering::Less,
            (EntryType::Query, EntryType::Folder) => std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        };
        type_ord.then_with(|| {
            a.entry
                .name
                .to_lowercase()
                .cmp(&b.entry.name.to_lowercase())
        })
    });
    for node in nodes.iter_mut() {
        sort_tree(&mut node.children);
    }
}

/// Flatten visible tree nodes into a list for rendering.
fn flatten_tree(nodes: &[TreeNode], depth: usize, out: &mut Vec<FlatEntry>) {
    for node in nodes {
        out.push(FlatEntry {
            entry_id: node.entry.id,
            depth,
            name: node.entry.name.clone(),
            entry_type: node.entry.entry_type,
            is_expanded: node.expanded,
            has_children: !node.children.is_empty(),
        });
        if node.expanded {
            flatten_tree(&node.children, depth + 1, out);
        }
    }
}

/// Find a mutable reference to a node by entry ID.
fn find_node_mut(nodes: &mut Vec<TreeNode>, id: i64) -> Option<&mut TreeNode> {
    for node in nodes.iter_mut() {
        if node.entry.id == id {
            return Some(node);
        }
        if let Some(found) = find_node_mut(&mut node.children, id) {
            return Some(found);
        }
    }
    None
}

/// Find the parent ID of an entry by searching the tree.
fn find_parent_id(nodes: &[TreeNode], target_id: i64) -> Option<i64> {
    for node in nodes {
        for child in &node.children {
            if child.entry.id == target_id {
                return Some(node.entry.id);
            }
        }
        if let Some(found) = find_parent_id(&node.children, target_id) {
            return Some(found);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model;
    use rstools_core::db::open_memory_db;

    fn setup_db() -> Connection {
        let conn = open_memory_db().unwrap();
        model::init_db(&conn).unwrap();
        conn
    }

    #[test]
    fn test_reload_empty() {
        let conn = setup_db();
        let mut sidebar = SidebarState::new();
        sidebar.reload(&conn).unwrap();
        assert!(sidebar.flat_view.is_empty());
        assert!(sidebar.roots.is_empty());
    }

    #[test]
    fn test_reload_with_entries() {
        let conn = setup_db();
        model::add_entry(&conn, None, "folder-a", EntryType::Folder).unwrap();
        model::add_entry(&conn, None, "query-b", EntryType::Query).unwrap();

        let mut sidebar = SidebarState::new();
        sidebar.reload(&conn).unwrap();

        assert_eq!(sidebar.flat_view.len(), 2);
        assert_eq!(sidebar.flat_view[0].name, "folder-a");
        assert_eq!(sidebar.flat_view[1].name, "query-b");
    }

    #[test]
    fn test_expand_collapse() {
        let conn = setup_db();
        let folder_id = model::add_entry(&conn, None, "api", EntryType::Folder).unwrap();
        model::add_entry(&conn, Some(folder_id), "get-users", EntryType::Query).unwrap();

        let mut sidebar = SidebarState::new();
        sidebar.reload(&conn).unwrap();

        // Initially collapsed — only the folder is visible
        assert_eq!(sidebar.flat_view.len(), 1);

        // Expand
        sidebar.toggle_expand();
        assert_eq!(sidebar.flat_view.len(), 2);
        assert_eq!(sidebar.flat_view[1].name, "get-users");

        // Collapse
        sidebar.toggle_expand();
        assert_eq!(sidebar.flat_view.len(), 1);
    }

    #[test]
    fn test_navigation() {
        let conn = setup_db();
        model::add_entry(&conn, None, "a", EntryType::Folder).unwrap();
        model::add_entry(&conn, None, "b", EntryType::Folder).unwrap();
        model::add_entry(&conn, None, "c", EntryType::Query).unwrap();

        let mut sidebar = SidebarState::new();
        sidebar.reload(&conn).unwrap();

        assert_eq!(sidebar.selected, 0);
        sidebar.move_down();
        assert_eq!(sidebar.selected, 1);
        sidebar.move_down();
        assert_eq!(sidebar.selected, 2);
        sidebar.move_down(); // should not go past end
        assert_eq!(sidebar.selected, 2);

        sidebar.goto_top();
        assert_eq!(sidebar.selected, 0);

        sidebar.goto_bottom();
        assert_eq!(sidebar.selected, 2);

        sidebar.move_up();
        assert_eq!(sidebar.selected, 1);
    }

    #[test]
    fn test_sort_folders_first() {
        let conn = setup_db();
        model::add_entry(&conn, None, "z-query", EntryType::Query).unwrap();
        model::add_entry(&conn, None, "a-folder", EntryType::Folder).unwrap();
        model::add_entry(&conn, None, "b-query", EntryType::Query).unwrap();
        model::add_entry(&conn, None, "c-folder", EntryType::Folder).unwrap();

        let mut sidebar = SidebarState::new();
        sidebar.reload(&conn).unwrap();

        assert_eq!(sidebar.flat_view[0].name, "a-folder");
        assert_eq!(sidebar.flat_view[0].entry_type, EntryType::Folder);
        assert_eq!(sidebar.flat_view[1].name, "c-folder");
        assert_eq!(sidebar.flat_view[1].entry_type, EntryType::Folder);
        assert_eq!(sidebar.flat_view[2].name, "b-query");
        assert_eq!(sidebar.flat_view[2].entry_type, EntryType::Query);
        assert_eq!(sidebar.flat_view[3].name, "z-query");
        assert_eq!(sidebar.flat_view[3].entry_type, EntryType::Query);
    }

    #[test]
    fn test_input_buffer_operations() {
        let mut sidebar = SidebarState::new();

        sidebar.input_insert_char('h');
        sidebar.input_insert_char('e');
        sidebar.input_insert_char('l');
        sidebar.input_insert_char('l');
        sidebar.input_insert_char('o');
        assert_eq!(sidebar.input_buffer, "hello");
        assert_eq!(sidebar.input_cursor, 5);

        sidebar.input_backspace();
        assert_eq!(sidebar.input_buffer, "hell");
        assert_eq!(sidebar.input_cursor, 4);

        sidebar.input_cursor_left();
        assert_eq!(sidebar.input_cursor, 3);

        sidebar.input_insert_char('X');
        assert_eq!(sidebar.input_buffer, "helXl");
        assert_eq!(sidebar.input_cursor, 4);

        sidebar.input_cursor_right();
        assert_eq!(sidebar.input_cursor, 5);
    }
}
