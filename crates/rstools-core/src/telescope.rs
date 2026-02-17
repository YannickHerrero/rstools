use ratatui::{
    layout::{Constraint, Flex, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};

/// A single item that can appear in the telescope results.
#[derive(Debug, Clone)]
pub struct TelescopeItem {
    /// Display text for the item.
    pub label: String,
    /// Optional description/preview.
    pub description: String,
    /// Identifier to pass back when selected (e.g., tool name, item id).
    pub id: String,
}

/// Telescope fuzzy finder overlay state.
#[derive(Debug)]
pub struct Telescope {
    /// Whether the telescope overlay is visible.
    pub visible: bool,
    /// Current search query.
    pub query: String,
    /// Cursor position within the query.
    pub cursor: usize,
    /// All available items (unfiltered).
    pub items: Vec<TelescopeItem>,
    /// Filtered items based on query.
    pub filtered: Vec<usize>, // indices into `items`
    /// Selection state for the filtered list.
    pub list_state: ListState,
    /// Title for the telescope window.
    pub title: String,
}

impl Default for Telescope {
    fn default() -> Self {
        Self {
            visible: false,
            query: String::new(),
            cursor: 0,
            items: Vec::new(),
            filtered: Vec::new(),
            list_state: ListState::default(),
            title: String::from("Find"),
        }
    }
}

impl Telescope {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open the telescope with a given title and items.
    pub fn open(&mut self, title: impl Into<String>, items: Vec<TelescopeItem>) {
        self.visible = true;
        self.title = title.into();
        self.query.clear();
        self.cursor = 0;
        self.items = items;
        self.filter();
        if !self.filtered.is_empty() {
            self.list_state.select(Some(0));
        }
    }

    /// Close and reset the telescope.
    pub fn close(&mut self) {
        self.visible = false;
        self.query.clear();
        self.cursor = 0;
        self.items.clear();
        self.filtered.clear();
        self.list_state.select(None);
    }

    /// Insert a character at the cursor position.
    pub fn insert_char(&mut self, c: char) {
        self.query.insert(self.cursor, c);
        self.cursor += c.len_utf8();
        self.filter();
    }

    /// Delete the character before the cursor.
    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let prev = self.query[..self.cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.query.drain(prev..self.cursor);
            self.cursor = prev;
            self.filter();
        }
    }

    /// Move selection down.
    pub fn move_down(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        let current = self.list_state.selected().unwrap_or(0);
        let next = if current + 1 >= self.filtered.len() {
            0
        } else {
            current + 1
        };
        self.list_state.select(Some(next));
    }

    /// Move selection up.
    pub fn move_up(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        let current = self.list_state.selected().unwrap_or(0);
        let next = if current == 0 {
            self.filtered.len().saturating_sub(1)
        } else {
            current - 1
        };
        self.list_state.select(Some(next));
    }

    /// Get the currently selected item's id, if any.
    pub fn selected_id(&self) -> Option<&str> {
        let sel = self.list_state.selected()?;
        let idx = *self.filtered.get(sel)?;
        Some(&self.items[idx].id)
    }

    /// Simple case-insensitive substring matching.
    /// Can be upgraded to proper fuzzy matching later (e.g., with nucleo).
    fn filter(&mut self) {
        let query_lower = self.query.to_lowercase();
        self.filtered = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                if query_lower.is_empty() {
                    return true;
                }
                item.label.to_lowercase().contains(&query_lower)
                    || item.description.to_lowercase().contains(&query_lower)
            })
            .map(|(i, _)| i)
            .collect();

        // Keep selection in bounds
        if self.filtered.is_empty() {
            self.list_state.select(None);
        } else {
            let sel = self.list_state.selected().unwrap_or(0);
            if sel >= self.filtered.len() {
                self.list_state.select(Some(self.filtered.len() - 1));
            }
        }
    }

    /// Render the telescope overlay.
    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        if !self.visible {
            return;
        }

        // Size: 60% width, 60% height, centered
        let popup_width = (area.width * 60 / 100)
            .max(40)
            .min(area.width.saturating_sub(4));
        let popup_height = (area.height * 60 / 100)
            .max(10)
            .min(area.height.saturating_sub(4));

        let vertical = Layout::vertical([Constraint::Length(popup_height)]).flex(Flex::Center);
        let horizontal = Layout::horizontal([Constraint::Length(popup_width)]).flex(Flex::Center);
        let [popup_area] = vertical.areas(area);
        let [popup_area] = horizontal.areas(popup_area);

        frame.render_widget(Clear, popup_area);

        // Split into search input (2 lines) + results
        let [input_area, results_area] =
            Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).areas(popup_area);

        // Search input
        let input_block = Block::default()
            .title(format!(" {} ", self.title))
            .borders(Borders::ALL);

        let input_text = Paragraph::new(Line::from(vec![
            Span::styled("> ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&self.query),
        ]))
        .block(input_block);
        frame.render_widget(input_text, input_area);

        // Place cursor
        frame.set_cursor_position((
            input_area.x + 2 + self.cursor as u16 + 1, // +1 for border, +2 for "> "
            input_area.y + 1,
        ));

        // Results list
        let items: Vec<ListItem> = self
            .filtered
            .iter()
            .map(|&idx| {
                let item = &self.items[idx];
                let line = if item.description.is_empty() {
                    Line::from(Span::raw(&item.label))
                } else {
                    Line::from(vec![
                        Span::styled(&item.label, Style::default().add_modifier(Modifier::BOLD)),
                        Span::styled(
                            format!("  {}", item.description),
                            Style::default().add_modifier(Modifier::DIM),
                        ),
                    ])
                };
                ListItem::new(line)
            })
            .collect();

        let results_block =
            Block::default().borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM);

        let results = List::new(items)
            .block(results_block)
            .highlight_style(Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED))
            .highlight_symbol("> ");

        frame.render_stateful_widget(results, results_area, &mut self.list_state);
    }
}
