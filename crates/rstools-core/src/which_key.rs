use ratatui::{
    layout::{Alignment, Constraint, Flex, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

/// A single entry in the which-key menu.
#[derive(Debug, Clone)]
pub struct WhichKeyEntry {
    /// The key to press (e.g., "f", "t", "1").
    pub key: String,
    /// Human-readable description (e.g., "Find (telescope)", "Todo").
    pub description: String,
    /// Whether this entry is a group (has sub-entries) or a leaf action.
    pub is_group: bool,
}

impl WhichKeyEntry {
    pub fn action(key: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            description: description.into(),
            is_group: false,
        }
    }

    pub fn group(key: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            description: description.into(),
            is_group: true,
        }
    }
}

/// The which-key popup state.
#[derive(Debug, Default)]
pub struct WhichKey {
    /// Whether the popup is currently visible.
    pub visible: bool,
    /// Current entries to display.
    pub entries: Vec<WhichKeyEntry>,
    /// Title for the current level (e.g., "Leader", "Find").
    pub title: String,
}

impl WhichKey {
    pub fn new() -> Self {
        Self::default()
    }

    /// Show the which-key popup with the given entries.
    pub fn show(&mut self, title: impl Into<String>, entries: Vec<WhichKeyEntry>) {
        self.visible = true;
        self.title = title.into();
        self.entries = entries;
    }

    /// Hide the which-key popup.
    pub fn hide(&mut self) {
        self.visible = false;
        self.entries.clear();
        self.title.clear();
    }

    /// Render the which-key popup centered on screen.
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        if !self.visible || self.entries.is_empty() {
            return;
        }

        // Calculate popup size
        let max_key_len = self.entries.iter().map(|e| e.key.len()).max().unwrap_or(1);
        let max_desc_len = self
            .entries
            .iter()
            .map(|e| e.description.len())
            .max()
            .unwrap_or(10);
        let popup_width = (max_key_len + max_desc_len + 8).min(60) as u16;
        let popup_height = (self.entries.len() as u16 + 2).min(area.height.saturating_sub(4));

        // Center the popup
        let popup_area = centered_rect(popup_width, popup_height, area);

        // Clear the area behind the popup
        frame.render_widget(Clear, popup_area);

        // Build lines
        let lines: Vec<Line> = self
            .entries
            .iter()
            .map(|entry| {
                let key_style = Style::default().add_modifier(Modifier::BOLD);
                let desc_style = if entry.is_group {
                    Style::default().add_modifier(Modifier::UNDERLINED)
                } else {
                    Style::default()
                };
                let suffix = if entry.is_group { " +" } else { "" };

                Line::from(vec![
                    Span::styled(format!("  {} ", entry.key), key_style),
                    Span::styled("-> ", Style::default().add_modifier(Modifier::DIM)),
                    Span::styled(format!("{}{}", entry.description, suffix), desc_style),
                ])
            })
            .collect();

        let block = Block::default()
            .title(format!(" {} ", self.title))
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL);

        let paragraph = Paragraph::new(lines).block(block);
        frame.render_widget(paragraph, popup_area);
    }
}

/// Returns the top-level which-key entries for the hub.
pub fn hub_leader_entries() -> Vec<WhichKeyEntry> {
    vec![
        WhichKeyEntry::group("f", "Find"),
        WhichKeyEntry::action("h", "HTTP"),
        WhichKeyEntry::group("k", "KeePass"),
        WhichKeyEntry::action("t", "Todo"),
        WhichKeyEntry::action("q", "Quit"),
        WhichKeyEntry::action("?", "Help"),
        WhichKeyEntry::action("1-9", "Switch to tool"),
        WhichKeyEntry::action("<Space>", "Tool picker"),
    ]
}

/// Returns which-key entries for the Todo tool leader group.
pub fn todo_leader_entries() -> Vec<WhichKeyEntry> {
    vec![
        WhichKeyEntry::action("a", "Add todo"),
        WhichKeyEntry::action("d", "Delete todo"),
    ]
}

/// Helper to create a centered rect within a given area.
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([Constraint::Length(height)]).flex(Flex::Center);
    let horizontal = Layout::horizontal([Constraint::Length(width)]).flex(Flex::Center);
    let [area] = vertical.areas(area);
    let [area] = horizontal.areas(area);
    area
}
