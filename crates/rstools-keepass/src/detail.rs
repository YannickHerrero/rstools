//! Detail panel state for displaying entry fields.

use crate::vault::EntryDetails;

/// Which field is focused in the detail panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailField {
    Title,
    Username,
    Password,
    Url,
    Notes,
    Tags,
    /// Custom field at the given index.
    Custom(usize),
}

/// The detail panel state.
pub struct DetailPanel {
    /// The currently displayed entry details (None if no entry selected).
    pub details: Option<EntryDetails>,
    /// Whether the password is currently visible.
    pub password_visible: bool,
    /// Which custom fields have their protected values revealed.
    pub revealed_custom: Vec<bool>,
    /// Scroll offset for notes (which can be long).
    pub notes_scroll: usize,
    /// Currently focused field (for copy operations).
    pub focused_field: DetailField,
    /// Scroll offset for the overall detail view.
    pub scroll: usize,
}

impl DetailPanel {
    pub fn new() -> Self {
        Self {
            details: None,
            password_visible: false,
            revealed_custom: Vec::new(),
            notes_scroll: 0,
            focused_field: DetailField::Title,
            scroll: 0,
        }
    }

    /// Update the displayed entry.
    pub fn set_entry(&mut self, details: Option<EntryDetails>) {
        self.password_visible = false;
        self.notes_scroll = 0;
        self.scroll = 0;
        self.focused_field = DetailField::Title;
        if let Some(ref d) = details {
            self.revealed_custom = vec![false; d.custom_fields.len()];
        } else {
            self.revealed_custom.clear();
        }
        self.details = details;
    }

    /// Clear the panel.
    pub fn clear(&mut self) {
        self.set_entry(None);
    }

    /// Toggle password visibility.
    pub fn toggle_password(&mut self) {
        self.password_visible = !self.password_visible;
    }

    /// Toggle a custom field's protected value visibility.
    pub fn toggle_custom_reveal(&mut self, idx: usize) {
        if let Some(revealed) = self.revealed_custom.get_mut(idx) {
            *revealed = !*revealed;
        }
    }

    /// Scroll down in the detail view.
    pub fn scroll_down(&mut self) {
        self.scroll = self.scroll.saturating_add(1);
    }

    /// Scroll up in the detail view.
    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }

    /// Get the value of the currently focused field for clipboard copy.
    pub fn get_field_value(&self, field: DetailField) -> Option<String> {
        let details = self.details.as_ref()?;
        match field {
            DetailField::Title => Some(details.title.clone()),
            DetailField::Username => Some(details.username.clone()),
            DetailField::Password => Some(details.password.clone()),
            DetailField::Url => Some(details.url.clone()),
            DetailField::Notes => Some(details.notes.clone()),
            DetailField::Tags => Some(details.tags.join(", ")),
            DetailField::Custom(idx) => details.custom_fields.get(idx).map(|(_, v, _)| v.clone()),
        }
    }
}
