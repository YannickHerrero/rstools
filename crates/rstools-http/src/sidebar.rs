// Re-export generic tree sidebar types, specialized for HTTP.
// HttpEntry implements TreeEntry, so TreeSidebar<HttpEntry> is our sidebar type.

use crate::model::{self, EntryType, HttpEntry};
use anyhow::Result;
use rstools_core::tree_sidebar::TreeEntry;
pub use rstools_core::tree_sidebar::{
    find_node, find_node_mut, find_parent_id, render_tree_sidebar, ClipboardItem, ClipboardMode,
    FlatEntry, SidebarInput, TreeNode, TreeSidebar, TreeSidebarRenderConfig,
};
use rusqlite::Connection;

/// Implement TreeEntry for HttpEntry so we can use TreeSidebar<HttpEntry>.
impl TreeEntry for HttpEntry {
    fn id(&self) -> i64 {
        self.id
    }
    fn parent_id(&self) -> Option<i64> {
        self.parent_id
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn is_folder(&self) -> bool {
        self.entry_type == EntryType::Folder
    }
    fn is_expanded(&self) -> bool {
        self.expanded
    }
}

/// Type alias for the HTTP sidebar.
pub type SidebarState = TreeSidebar<HttpEntry>;

/// Extension trait for HTTP-specific sidebar operations that need DB access.
pub trait HttpSidebarExt {
    fn reload(&mut self, conn: &Connection) -> Result<()>;
    fn toggle_expand_persist(&mut self, conn: &Connection) -> bool;
    fn expand_selected_persist(&mut self, conn: &Connection) -> bool;
    fn collapse_or_parent_persist(&mut self, conn: &Connection);
}

impl HttpSidebarExt for SidebarState {
    /// Reload the tree from the database.
    fn reload(&mut self, conn: &Connection) -> Result<()> {
        let entries = model::list_entries(conn)?;
        self.reload_from_entries(&entries);
        Ok(())
    }

    /// Toggle expansion of the selected folder, persisting to DB.
    fn toggle_expand_persist(&mut self, conn: &Connection) -> bool {
        if let Some((entry_id, new_state)) = self.toggle_expand() {
            let _ = model::set_entry_expanded(conn, entry_id, new_state);
            true
        } else {
            false
        }
    }

    /// Expand the selected folder, persisting to DB.
    fn expand_selected_persist(&mut self, conn: &Connection) -> bool {
        if let Some((entry_id, new_state)) = self.expand_selected() {
            let _ = model::set_entry_expanded(conn, entry_id, new_state);
            true
        } else {
            false
        }
    }

    /// Collapse or navigate to parent, persisting to DB.
    fn collapse_or_parent_persist(&mut self, conn: &Connection) {
        if let Some((entry_id, new_state)) = self.collapse_or_parent() {
            let _ = model::set_entry_expanded(conn, entry_id, new_state);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        sidebar.toggle_expand_persist(&conn);
        assert_eq!(sidebar.flat_view.len(), 2);
        assert_eq!(sidebar.flat_view[1].name, "get-users");

        // Collapse
        sidebar.toggle_expand_persist(&conn);
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
        sidebar.move_down(); // moves to blank root line
        assert_eq!(sidebar.selected, 3);
        assert!(sidebar.is_on_blank_line());
        assert!(sidebar.selected_entry().is_none());
        sidebar.move_down(); // should not go past blank line
        assert_eq!(sidebar.selected, 3);

        sidebar.goto_top();
        assert_eq!(sidebar.selected, 0);

        sidebar.goto_bottom(); // goes to blank root line
        assert_eq!(sidebar.selected, 3);
        assert!(sidebar.is_on_blank_line());

        sidebar.move_up();
        assert_eq!(sidebar.selected, 2);
        assert!(!sidebar.is_on_blank_line());
        assert!(sidebar.selected_entry().is_some());
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
        assert!(sidebar.flat_view[0].is_folder);
        assert_eq!(sidebar.flat_view[1].name, "c-folder");
        assert!(sidebar.flat_view[1].is_folder);
        assert_eq!(sidebar.flat_view[2].name, "b-query");
        assert!(!sidebar.flat_view[2].is_folder);
        assert_eq!(sidebar.flat_view[3].name, "z-query");
        assert!(!sidebar.flat_view[3].is_folder);
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

    #[test]
    fn test_guide_depths() {
        let conn = setup_db();
        let folder_a = model::add_entry(&conn, None, "a-folder", EntryType::Folder).unwrap();
        let sub = model::add_entry(&conn, Some(folder_a), "sub", EntryType::Folder).unwrap();
        model::add_entry(&conn, Some(sub), "query-1", EntryType::Query).unwrap();
        model::add_entry(&conn, Some(sub), "query-2", EntryType::Query).unwrap();
        model::add_entry(&conn, Some(folder_a), "query-3", EntryType::Query).unwrap();
        model::add_entry(&conn, None, "b-query", EntryType::Query).unwrap();

        let mut sidebar = SidebarState::new();
        sidebar.reload(&conn).unwrap();

        // Expand all folders
        sidebar.selected = 0; // a-folder
        sidebar.toggle_expand_persist(&conn);
        sidebar.selected = 1; // sub
        sidebar.toggle_expand_persist(&conn);

        assert_eq!(sidebar.flat_view.len(), 6);

        // a-folder: depth 0, no guides
        assert_eq!(sidebar.flat_view[0].guide_depths, Vec::<bool>::new());

        // sub: depth 1, child of expanded a-folder → [true]
        assert_eq!(sidebar.flat_view[1].guide_depths, vec![true]);

        // query-1: depth 2, both ancestors expanded → [true, true]
        assert_eq!(sidebar.flat_view[2].guide_depths, vec![true, true]);

        // query-2: depth 2, same guides
        assert_eq!(sidebar.flat_view[3].guide_depths, vec![true, true]);

        // query-3: depth 1, child of expanded a-folder → [true]
        assert_eq!(sidebar.flat_view[4].guide_depths, vec![true]);

        // b-query: depth 0, no guides
        assert_eq!(sidebar.flat_view[5].guide_depths, Vec::<bool>::new());
    }

    #[test]
    fn test_guide_depths_last_sibling() {
        let conn = setup_db();
        let folder = model::add_entry(&conn, None, "only-folder", EntryType::Folder).unwrap();
        model::add_entry(&conn, Some(folder), "child", EntryType::Query).unwrap();

        let mut sidebar = SidebarState::new();
        sidebar.reload(&conn).unwrap();

        sidebar.selected = 0;
        sidebar.toggle_expand_persist(&conn);

        assert_eq!(sidebar.flat_view[1].guide_depths, vec![true]);
    }

    #[test]
    fn test_blank_line_empty_tree() {
        let conn = setup_db();
        let mut sidebar = SidebarState::new();
        sidebar.reload(&conn).unwrap();

        assert_eq!(sidebar.selected, 0);
        assert!(sidebar.flat_view.is_empty());
        assert!(sidebar.selected_entry().is_none());
        assert!(!sidebar.is_on_blank_line());
    }

    #[test]
    fn test_blank_line_with_entries() {
        let conn = setup_db();
        model::add_entry(&conn, None, "item", EntryType::Query).unwrap();

        let mut sidebar = SidebarState::new();
        sidebar.reload(&conn).unwrap();

        assert_eq!(sidebar.flat_view.len(), 1);

        // Move past last entry to blank line
        sidebar.move_down();
        assert_eq!(sidebar.selected, 1);
        assert!(sidebar.is_on_blank_line());
        assert!(sidebar.selected_entry().is_none());

        // Move back up
        sidebar.move_up();
        assert_eq!(sidebar.selected, 0);
        assert!(!sidebar.is_on_blank_line());
        assert!(sidebar.selected_entry().is_some());
    }
}
