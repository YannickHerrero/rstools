use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::driver::{ColumnInfo, FilterOp, QueryFilter, QueryResult, SortDirection};

// ── Table action ────────────────────────────────────────────────────

pub enum TableAction {
    /// No action needed from the parent.
    None,
    /// Re-fetch from scratch (sort/filter changed).
    Refresh,
    /// Load the next batch of rows.
    LoadMore,
}

// ── Table view state ────────────────────────────────────────────────

pub const BATCH_SIZE: usize = 20;

pub struct TableView {
    pub columns: Vec<ColumnInfo>,
    pub rows: Vec<Vec<String>>,
    pub total_count: i64,
    /// How many rows have been loaded so far (accumulates with "load more").
    pub loaded_count: usize,
    pub page_size: usize,
    pub selected_row: usize,
    pub selected_col: usize,
    pub sort_column: Option<usize>,
    pub sort_direction: SortDirection,
    pub filters: Vec<QueryFilter>,
    pub scroll_offset_x: usize,
    // Filter input
    filter_input: Option<FilterInput>,
}

struct FilterInput {
    buffer: String,
    cursor: usize,
}

impl TableView {
    pub fn new() -> Self {
        Self {
            columns: Vec::new(),
            rows: Vec::new(),
            total_count: 0,
            loaded_count: 0,
            page_size: BATCH_SIZE,
            selected_row: 0,
            selected_col: 0,
            sort_column: None,
            sort_direction: SortDirection::Asc,
            filters: Vec::new(),
            scroll_offset_x: 0,
            filter_input: None,
        }
    }

    pub fn reset(&mut self) {
        self.columns.clear();
        self.rows.clear();
        self.total_count = 0;
        self.loaded_count = 0;
        self.page_size = BATCH_SIZE;
        self.selected_row = 0;
        self.selected_col = 0;
        self.sort_column = None;
        self.sort_direction = SortDirection::Asc;
        self.filters.clear();
        self.scroll_offset_x = 0;
        self.filter_input = None;
    }

    pub fn set_columns(&mut self, cols: Vec<ColumnInfo>) {
        self.columns = cols;
    }

    /// Set data for a fresh query (replaces all rows).
    pub fn set_data(&mut self, result: QueryResult) {
        if !result.columns.is_empty() {
            self.columns = result.columns;
        }
        self.rows = result.rows;
        self.total_count = result.total_count;
        self.loaded_count = self.rows.len();
        if self.selected_row >= self.rows.len() && !self.rows.is_empty() {
            self.selected_row = self.rows.len() - 1;
        }
    }

    /// Append rows from a "load more" fetch.
    pub fn append_data(&mut self, result: QueryResult) {
        self.total_count = result.total_count;
        self.rows.extend(result.rows);
        self.loaded_count = self.rows.len();
    }

    /// Whether there are more rows to load.
    pub fn has_more(&self) -> bool {
        (self.loaded_count as i64) < self.total_count
    }

    /// Whether the selected row is on the "load more" line.
    pub fn is_on_load_more(&self) -> bool {
        self.has_more() && self.selected_row == self.rows.len()
    }

    pub fn is_filtering(&self) -> bool {
        self.filter_input.is_some()
    }

    pub fn sort_column_name(&self) -> Option<String> {
        self.sort_column
            .and_then(|idx| self.columns.get(idx))
            .map(|c| c.name.clone())
    }

    /// Max row index the cursor can reach (includes the "load more" virtual row).
    fn max_row(&self) -> usize {
        if self.rows.is_empty() {
            return 0;
        }
        if self.has_more() {
            self.rows.len() // the virtual "load more" row
        } else {
            self.rows.len() - 1
        }
    }

    pub fn move_down(&mut self, n: usize) {
        let max = self.max_row();
        self.selected_row = (self.selected_row + n).min(max);
    }

    pub fn move_up(&mut self, n: usize) {
        self.selected_row = self.selected_row.saturating_sub(n);
    }

    /// Handle a key event. Returns a `TableAction` describing what the parent should do.
    pub fn handle_key(&mut self, key: KeyEvent) -> TableAction {
        // Filter input mode
        if self.filter_input.is_some() {
            return if self.handle_filter_key(key) {
                TableAction::Refresh
            } else {
                TableAction::None
            };
        }

        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_down(1);
                TableAction::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_up(1);
                TableAction::None
            }
            KeyCode::Char('l') | KeyCode::Right => {
                if self.selected_col + 1 < self.columns.len() {
                    self.selected_col += 1;
                    self.adjust_scroll_x();
                }
                TableAction::None
            }
            KeyCode::Char('h') | KeyCode::Left => {
                if self.selected_col > 0 {
                    self.selected_col -= 1;
                    self.adjust_scroll_x();
                }
                TableAction::None
            }
            KeyCode::Char('g') => {
                self.selected_row = 0;
                TableAction::None
            }
            KeyCode::Char('G') => {
                self.selected_row = self.max_row();
                TableAction::None
            }
            KeyCode::Char('d') if key.modifiers == KeyModifiers::CONTROL => {
                let half = self.rows.len() / 2;
                self.move_down(half.max(1));
                TableAction::None
            }
            KeyCode::Char('u') if key.modifiers == KeyModifiers::CONTROL => {
                let half = self.rows.len() / 2;
                self.move_up(half.max(1));
                TableAction::None
            }
            KeyCode::Enter => {
                if self.is_on_load_more() {
                    TableAction::LoadMore
                } else {
                    TableAction::None
                }
            }
            KeyCode::Char('s') => {
                // Cycle sort on current column
                if !self.columns.is_empty() {
                    if self.sort_column == Some(self.selected_col) {
                        match self.sort_direction {
                            SortDirection::Asc => {
                                self.sort_direction = SortDirection::Desc;
                            }
                            SortDirection::Desc => {
                                self.sort_column = None;
                                self.sort_direction = SortDirection::Asc;
                            }
                        }
                    } else {
                        self.sort_column = Some(self.selected_col);
                        self.sort_direction = SortDirection::Asc;
                    }
                    self.selected_row = 0;
                    TableAction::Refresh
                } else {
                    TableAction::None
                }
            }
            KeyCode::Char('/') => {
                self.filter_input = Some(FilterInput {
                    buffer: String::new(),
                    cursor: 0,
                });
                TableAction::None
            }
            KeyCode::Char('c') if key.modifiers == KeyModifiers::NONE => {
                // Clear filters
                if !self.filters.is_empty() {
                    self.filters.clear();
                    self.selected_row = 0;
                    TableAction::Refresh
                } else {
                    TableAction::None
                }
            }
            _ => TableAction::None,
        }
    }

    fn handle_filter_key(&mut self, key: KeyEvent) -> bool {
        let Some(ref mut fi) = self.filter_input else {
            return false;
        };

        match key.code {
            KeyCode::Esc => {
                self.filter_input = None;
                false
            }
            KeyCode::Enter => {
                let text = fi.buffer.clone();
                self.filter_input = None;
                if !text.is_empty() && !self.columns.is_empty() {
                    let col_name = self
                        .columns
                        .get(self.selected_col)
                        .map(|c| c.name.clone())
                        .unwrap_or_default();
                    self.filters.push(QueryFilter {
                        column: col_name,
                        operator: FilterOp::Contains,
                        value: text,
                    });
                    self.selected_row = 0;
                    true
                } else {
                    false
                }
            }
            KeyCode::Char(c) => {
                fi.buffer.insert(fi.cursor, c);
                fi.cursor += c.len_utf8();
                false
            }
            KeyCode::Backspace => {
                if fi.cursor > 0 {
                    let prev = fi.buffer[..fi.cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    fi.buffer.drain(prev..fi.cursor);
                    fi.cursor = prev;
                }
                false
            }
            KeyCode::Left => {
                if fi.cursor > 0 {
                    fi.cursor = fi.buffer[..fi.cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                }
                false
            }
            KeyCode::Right => {
                if fi.cursor < fi.buffer.len() {
                    fi.cursor = fi.buffer[fi.cursor..]
                        .char_indices()
                        .nth(1)
                        .map(|(i, _)| fi.cursor + i)
                        .unwrap_or(fi.buffer.len());
                }
                false
            }
            _ => false,
        }
    }

    fn adjust_scroll_x(&mut self) {
        // Keep selected column visible (simple logic)
        if self.selected_col < self.scroll_offset_x {
            self.scroll_offset_x = self.selected_col;
        }
        // We'll handle the "too far right" case in rendering
    }

    pub fn filter_text(&self) -> Option<&str> {
        self.filter_input.as_ref().map(|fi| fi.buffer.as_str())
    }

    pub fn filter_cursor(&self) -> usize {
        self.filter_input.as_ref().map(|fi| fi.cursor).unwrap_or(0)
    }
}
