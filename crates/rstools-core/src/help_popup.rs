use ratatui::{
    layout::{Alignment, Constraint, Flex, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

/// A single entry in the help popup.
#[derive(Debug, Clone)]
pub struct HelpEntry {
    /// The key or key combination (e.g., "j/k", "<Space>f", "dd").
    pub key: String,
    /// Human-readable description (e.g., "Move down/up").
    pub description: String,
    /// Optional section header this entry belongs to.
    pub section: Option<String>,
}

impl HelpEntry {
    pub fn new(key: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            description: description.into(),
            section: None,
        }
    }

    pub fn with_section(
        section: impl Into<String>,
        key: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            key: key.into(),
            description: description.into(),
            section: Some(section.into()),
        }
    }
}

/// The help popup state.
#[derive(Debug, Default)]
pub struct HelpPopup {
    /// Whether the popup is currently visible.
    pub visible: bool,
    /// Title for the help popup (e.g., "Help", "Todo Help").
    title: String,
    /// All help entries to display.
    entries: Vec<HelpEntry>,
    /// Scroll offset for long help content.
    scroll: u16,
}

impl HelpPopup {
    pub fn new() -> Self {
        Self::default()
    }

    /// Show the help popup with the given title and entries.
    pub fn show(&mut self, title: impl Into<String>, entries: Vec<HelpEntry>) {
        self.visible = true;
        self.title = title.into();
        self.entries = entries;
        self.scroll = 0;
    }

    /// Hide the help popup.
    pub fn hide(&mut self) {
        self.visible = false;
        self.entries.clear();
        self.title.clear();
        self.scroll = 0;
    }

    /// Scroll down by one line.
    pub fn scroll_down(&mut self) {
        self.scroll = self.scroll.saturating_add(1);
    }

    /// Scroll up by one line.
    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }

    /// Render the help popup centered on screen.
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        if !self.visible || self.entries.is_empty() {
            return;
        }

        // Build lines, grouping by section
        let lines = self.build_lines();

        // Calculate popup size — use most of the screen
        let popup_width = (area.width.saturating_sub(8)).min(60);
        let popup_height = (area.height.saturating_sub(6)).min(lines.len() as u16 + 2);

        // Center the popup
        let popup_area = centered_rect(popup_width, popup_height, area);

        // Clear the area behind the popup
        frame.render_widget(Clear, popup_area);

        let block = Block::default()
            .title(format!(" {} ", self.title))
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL);

        // Clamp scroll
        let max_scroll = (lines.len() as u16).saturating_sub(popup_height.saturating_sub(2));
        let scroll = self.scroll.min(max_scroll);

        let paragraph = Paragraph::new(lines).block(block).scroll((scroll, 0));
        frame.render_widget(paragraph, popup_area);
    }

    /// Build display lines from entries, inserting section headers.
    fn build_lines(&self) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> = Vec::new();
        let mut current_section: Option<&str> = None;

        for entry in &self.entries {
            // Insert section header if it changed
            if let Some(ref section) = entry.section {
                if current_section != Some(section.as_str()) {
                    if !lines.is_empty() {
                        lines.push(Line::from(""));
                    }
                    lines.push(Line::from(Span::styled(
                        format!(" {}", section),
                        Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                    )));
                    current_section = Some(section.as_str());
                }
            }

            let key_style = Style::default().add_modifier(Modifier::BOLD);
            let desc_style = Style::default();

            lines.push(Line::from(vec![
                Span::styled(format!("  {:>12} ", entry.key), key_style),
                Span::styled("  ", Style::default().add_modifier(Modifier::DIM)),
                Span::styled(entry.description.clone(), desc_style),
            ]));
        }

        // Footer
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            " Esc/q/?  close    j/k  scroll",
            Style::default().add_modifier(Modifier::DIM),
        )));

        lines
    }
}

/// Returns the global keybind help entries (shown when no tool or on dashboard).
pub fn global_help_entries() -> Vec<HelpEntry> {
    vec![
        HelpEntry::with_section("Navigation", "j / k", "Move down / up"),
        HelpEntry::with_section("Navigation", "gg", "Go to top"),
        HelpEntry::with_section("Navigation", "G", "Go to bottom"),
        HelpEntry::with_section("Navigation", "Ctrl-d / Ctrl-u", "Half-page down / up"),
        HelpEntry::with_section("Navigation", "gt / gT", "Next / previous tool tab"),
        HelpEntry::with_section("Actions", "Enter", "Confirm / select / toggle"),
        HelpEntry::with_section("Actions", "dd", "Delete item"),
        HelpEntry::with_section("Actions", "a / o", "Add item / add below"),
        HelpEntry::with_section("Actions", "e", "Edit item"),
        HelpEntry::with_section("Actions", "i", "Enter Insert mode"),
        HelpEntry::with_section("Actions", "/", "Search / filter"),
        HelpEntry::with_section("Leader (Space)", "<Space>", "Open leader menu"),
        HelpEntry::with_section("Leader (Space)", "<Space><Space>", "Tool picker"),
        HelpEntry::with_section("Leader (Space)", "<Space>f", "Find (telescope)"),
        HelpEntry::with_section("Leader (Space)", "<Space>t", "Todo"),
        HelpEntry::with_section("Leader (Space)", "<Space>1-9", "Switch to tool"),
        HelpEntry::with_section("Leader (Space)", "<Space>q", "Quit"),
        HelpEntry::with_section("Other", ":", "Command mode"),
        HelpEntry::with_section("Other", ":q", "Close tool / quit"),
        HelpEntry::with_section("Other", ":qa", "Quit all"),
        HelpEntry::with_section("Other", "?", "This help"),
        HelpEntry::with_section("Other", "Ctrl-c", "Force quit"),
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
