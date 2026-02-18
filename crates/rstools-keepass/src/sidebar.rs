use crate::model::{self, KeePassFile};
use anyhow::Result;
use rusqlite::Connection;

/// The full sidebar state for KeePass file history.
pub struct SidebarState {
    /// List of tracked files, ordered by most recently opened.
    pub files: Vec<KeePassFile>,
    /// Currently selected index.
    pub selected: usize,
    /// Whether the sidebar is visible.
    pub visible: bool,
    /// Whether a delete confirmation is pending.
    pub confirm_delete: bool,
}

impl SidebarState {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            selected: 0,
            visible: true,
            confirm_delete: false,
        }
    }

    /// Reload the file list from the database.
    pub fn reload(&mut self, conn: &Connection) -> Result<()> {
        self.files = model::list_files(conn)?;
        // Keep selection in bounds
        if !self.files.is_empty() && self.selected >= self.files.len() {
            self.selected = self.files.len() - 1;
        }
        Ok(())
    }

    /// Get the currently selected file, if any.
    pub fn selected_file(&self) -> Option<&KeePassFile> {
        self.files.get(self.selected)
    }

    /// Move selection down.
    pub fn move_down(&mut self) {
        if !self.files.is_empty() && self.selected < self.files.len() - 1 {
            self.selected += 1;
        }
    }

    /// Move selection up.
    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Jump to top.
    pub fn goto_top(&mut self) {
        self.selected = 0;
    }

    /// Jump to bottom.
    pub fn goto_bottom(&mut self) {
        if !self.files.is_empty() {
            self.selected = self.files.len() - 1;
        }
    }

    /// Half-page down.
    pub fn half_page_down(&mut self, visible_lines: usize) {
        let half = visible_lines / 2;
        self.selected = (self.selected + half).min(self.files.len().saturating_sub(1));
    }

    /// Half-page up.
    pub fn half_page_up(&mut self, visible_lines: usize) {
        let half = visible_lines / 2;
        self.selected = self.selected.saturating_sub(half);
    }

    /// Check if a PIN is valid (not expired) for the selected file.
    pub fn selected_has_valid_pin(&self) -> bool {
        if let Some(file) = self.selected_file() {
            if file.has_pin {
                if let Some(ref expires) = file.pin_expires_at {
                    return !crate::crypto::is_pin_expired(expires);
                }
            }
        }
        false
    }
}
