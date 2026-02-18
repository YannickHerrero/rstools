// Re-export generic tree sidebar types, specialized for Notes.
// NoteEntry implements TreeEntry, so TreeSidebar<NoteEntry> is our sidebar type.

use crate::model::{self, EntryType, NoteEntry};
use anyhow::Result;
use rstools_core::tree_sidebar::TreeEntry;
pub use rstools_core::tree_sidebar::{
    find_node, find_parent_id, render_tree_sidebar, ClipboardItem, ClipboardMode, FlatEntry,
    SidebarInput, TreeNode, TreeSidebar, TreeSidebarRenderConfig,
};
use rusqlite::Connection;

/// Implement TreeEntry for NoteEntry so we can use TreeSidebar<NoteEntry>.
impl TreeEntry for NoteEntry {
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

/// Type alias for the Notes sidebar.
pub type SidebarState = TreeSidebar<NoteEntry>;

/// Extension trait for Notes-specific sidebar operations that need DB access.
pub trait NotesSidebarExt {
    fn reload(&mut self, conn: &Connection) -> Result<()>;
    fn toggle_expand_persist(&mut self, conn: &Connection) -> bool;
    fn expand_selected_persist(&mut self, conn: &Connection) -> bool;
    fn collapse_or_parent_persist(&mut self, conn: &Connection);
}

impl NotesSidebarExt for SidebarState {
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
        let folder_id = model::add_entry(&conn, None, "Folder", EntryType::Folder).unwrap();
        model::add_entry(&conn, Some(folder_id), "Note", EntryType::Note).unwrap();

        let mut sidebar = SidebarState::new();
        sidebar.reload(&conn).unwrap();
        assert_eq!(sidebar.flat_view.len(), 1); // Only root folder visible (collapsed)
    }

    #[test]
    fn test_expand_persist() {
        let conn = setup_db();
        let folder_id = model::add_entry(&conn, None, "Folder", EntryType::Folder).unwrap();
        model::add_entry(&conn, Some(folder_id), "Note", EntryType::Note).unwrap();

        let mut sidebar = SidebarState::new();
        sidebar.reload(&conn).unwrap();

        // Expand the folder
        sidebar.toggle_expand_persist(&conn);
        assert_eq!(sidebar.flat_view.len(), 2); // Folder + Note

        // Verify persisted
        let entries = model::list_entries(&conn).unwrap();
        let folder = entries.iter().find(|e| e.id == folder_id).unwrap();
        assert!(folder.expanded);
    }
}
