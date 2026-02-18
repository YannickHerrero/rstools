use anyhow::Result;
use rusqlite::Connection;

use crate::model::{self, HttpMethod};

// ── Section / focus enums ────────────────────────────────────────────

/// Which section of the request panel is focused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Url,
    Params,
    Headers,
    Body,
}

impl Section {
    pub fn next(self) -> Self {
        match self {
            Section::Url => Section::Params,
            Section::Params => Section::Headers,
            Section::Headers => Section::Body,
            Section::Body => Section::Url,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Section::Url => Section::Body,
            Section::Params => Section::Url,
            Section::Headers => Section::Params,
            Section::Body => Section::Headers,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Section::Url => "URL",
            Section::Params => "Params",
            Section::Headers => "Headers",
            Section::Body => "Body",
        }
    }
}

/// Which sub-section of the response panel is focused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseSection {
    Body,
    Headers,
}

/// Which field is being edited in a key-value row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KvField {
    Key,
    Value,
}

/// Where focus is within the content panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelFocus {
    Request,
    Response,
}

// ── Key-value row ────────────────────────────────────────────────────

/// A single key-value row for headers or query params.
#[derive(Debug, Clone)]
pub struct KvRow {
    /// Database ID (0 for new unsaved rows).
    pub db_id: i64,
    pub key: String,
    pub value: String,
    pub enabled: bool,
    /// Cursor position within the currently edited field.
    pub cursor: usize,
}

impl KvRow {
    pub fn new_empty() -> Self {
        Self {
            db_id: 0,
            key: String::new(),
            value: String::new(),
            enabled: true,
            cursor: 0,
        }
    }
}

// ── Response data ────────────────────────────────────────────────────

/// Holds the result of an HTTP request.
#[derive(Debug, Clone)]
pub struct ResponseData {
    pub status_code: u16,
    pub status_text: String,
    pub elapsed_ms: u128,
    pub size_bytes: usize,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub body_scroll: usize,
    pub headers_scroll: usize,
    pub focused_section: ResponseSection,
}

impl ResponseData {
    pub fn body_line_count(&self) -> usize {
        self.body.lines().count().max(1)
    }

    pub fn scroll_body_down(&mut self, amount: usize) {
        let max = self.body_line_count().saturating_sub(1);
        self.body_scroll = (self.body_scroll + amount).min(max);
    }

    pub fn scroll_body_up(&mut self, amount: usize) {
        self.body_scroll = self.body_scroll.saturating_sub(amount);
    }

    pub fn scroll_headers_down(&mut self, amount: usize) {
        let max = self.headers.len().saturating_sub(1);
        self.headers_scroll = (self.headers_scroll + amount).min(max);
    }

    pub fn scroll_headers_up(&mut self, amount: usize) {
        self.headers_scroll = self.headers_scroll.saturating_sub(amount);
    }

    pub fn toggle_section(&mut self) {
        self.focused_section = match self.focused_section {
            ResponseSection::Body => ResponseSection::Headers,
            ResponseSection::Headers => ResponseSection::Body,
        };
    }
}

// ── Request panel state ──────────────────────────────────────────────

/// The full in-memory state of the request content panel.
pub struct RequestPanel {
    /// The entry_id of the currently opened query (None = no query selected).
    pub active_entry_id: Option<i64>,
    /// The DB request id (set after loading from DB).
    pub request_db_id: Option<i64>,
    /// The name of the currently opened query.
    pub active_entry_name: String,

    // Request fields
    pub method: HttpMethod,
    pub url: String,
    pub url_cursor: usize,

    pub headers: Vec<KvRow>,
    pub headers_selected: usize,

    pub query_params: Vec<KvRow>,
    pub params_selected: usize,

    pub body_lines: Vec<String>,
    pub body_cursor_row: usize,
    pub body_cursor_col: usize,

    // Focus
    pub focused_section: Section,
    pub panel_focus: PanelFocus,
    /// Whether the user is currently editing inline (insert mode within a section).
    pub editing: bool,
    /// Which field is being edited in kv sections.
    pub editing_field: KvField,

    // Layout
    /// When set, the corresponding panel is rendered fullscreen (hiding the other).
    pub fullscreen: Option<PanelFocus>,

    // Dirty tracking
    pub dirty: bool,

    // Response
    pub response: Option<ResponseData>,
    pub request_in_flight: bool,
    pub spinner_frame: u8,
    pub error_message: Option<String>,
}

impl RequestPanel {
    pub fn new() -> Self {
        Self {
            active_entry_id: None,
            request_db_id: None,
            active_entry_name: String::new(),
            method: HttpMethod::Get,
            url: String::new(),
            url_cursor: 0,
            headers: Vec::new(),
            headers_selected: 0,
            query_params: Vec::new(),
            params_selected: 0,
            body_lines: vec![String::new()],
            body_cursor_row: 0,
            body_cursor_col: 0,
            focused_section: Section::Url,
            panel_focus: PanelFocus::Request,
            editing: false,
            editing_field: KvField::Key,
            fullscreen: None,
            dirty: false,
            response: None,
            request_in_flight: false,
            spinner_frame: 0,
            error_message: None,
        }
    }

    /// Whether a query is currently loaded.
    pub fn is_active(&self) -> bool {
        self.active_entry_id.is_some()
    }

    // ── Load / Save ──────────────────────────────────────────────────

    /// Load a query's request data from the database.
    pub fn load(&mut self, entry_id: i64, entry_name: &str, conn: &Connection) -> Result<()> {
        let req_id = model::ensure_request(conn, entry_id)?;
        let req = model::load_request(conn, entry_id)?.unwrap();
        let db_headers = model::load_headers(conn, req_id)?;
        let db_params = model::load_query_params(conn, req_id)?;

        self.active_entry_id = Some(entry_id);
        self.request_db_id = Some(req_id);
        self.active_entry_name = entry_name.to_string();

        self.method = req.method;
        self.url = req.url;
        self.url_cursor = self.url.len();

        self.headers = db_headers
            .into_iter()
            .map(|h| KvRow {
                db_id: h.id,
                key: h.key,
                value: h.value,
                enabled: h.enabled,
                cursor: 0,
            })
            .collect();
        self.headers_selected = 0;

        self.query_params = db_params
            .into_iter()
            .map(|p| KvRow {
                db_id: p.id,
                key: p.key,
                value: p.value,
                enabled: p.enabled,
                cursor: 0,
            })
            .collect();
        self.params_selected = 0;

        self.body_lines = if req.body.is_empty() {
            vec![String::new()]
        } else {
            req.body.lines().map(|l| l.to_string()).collect()
        };
        self.body_cursor_row = 0;
        self.body_cursor_col = 0;

        self.focused_section = Section::Url;
        self.panel_focus = PanelFocus::Request;
        self.editing = false;
        self.fullscreen = None;
        self.dirty = false;
        self.response = None;
        self.error_message = None;

        Ok(())
    }

    /// Save the current request data to the database.
    pub fn save(&mut self, conn: &Connection) -> Result<()> {
        let req_id = match self.request_db_id {
            Some(id) => id,
            None => return Ok(()),
        };

        let body = self.body_lines.join("\n");
        model::save_request(conn, req_id, self.method, &self.url, &body)?;

        let headers: Vec<(String, String, bool)> = self
            .headers
            .iter()
            .map(|h| (h.key.clone(), h.value.clone(), h.enabled))
            .collect();
        model::replace_headers(conn, req_id, &headers)?;

        let params: Vec<(String, String, bool)> = self
            .query_params
            .iter()
            .map(|p| (p.key.clone(), p.value.clone(), p.enabled))
            .collect();
        model::replace_query_params(conn, req_id, &params)?;

        self.dirty = false;
        Ok(())
    }

    /// Clear the panel (no query selected).
    pub fn clear(&mut self) {
        *self = Self::new();
    }

    // ── Method ───────────────────────────────────────────────────────

    pub fn cycle_method_forward(&mut self) {
        self.method = self.method.next();
        self.dirty = true;
    }

    pub fn cycle_method_backward(&mut self) {
        self.method = self.method.prev();
        self.dirty = true;
    }

    // ── URL editing ──────────────────────────────────────────────────

    pub fn url_insert_char(&mut self, c: char) {
        self.url.insert(self.url_cursor, c);
        self.url_cursor += c.len_utf8();
        self.dirty = true;
    }

    pub fn url_backspace(&mut self) {
        if self.url_cursor > 0 {
            let prev = self.url[..self.url_cursor]
                .chars()
                .last()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.url_cursor -= prev;
            self.url.remove(self.url_cursor);
            self.dirty = true;
        }
    }

    pub fn url_delete(&mut self) {
        if self.url_cursor < self.url.len() {
            self.url.remove(self.url_cursor);
            self.dirty = true;
        }
    }

    pub fn url_cursor_left(&mut self) {
        if self.url_cursor > 0 {
            let prev = self.url[..self.url_cursor]
                .chars()
                .last()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.url_cursor -= prev;
        }
    }

    pub fn url_cursor_right(&mut self) {
        if self.url_cursor < self.url.len() {
            let next = self.url[self.url_cursor..]
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.url_cursor += next;
        }
    }

    pub fn url_cursor_home(&mut self) {
        self.url_cursor = 0;
    }

    pub fn url_cursor_end(&mut self) {
        self.url_cursor = self.url.len();
    }

    // ── Key-value (headers / params) editing ─────────────────────────

    /// Access the active kv row mutably.
    fn kv_selected_row_mut(&mut self) -> Option<&mut KvRow> {
        match self.focused_section {
            Section::Headers => self.headers.get_mut(self.headers_selected),
            Section::Params => self.query_params.get_mut(self.params_selected),
            _ => None,
        }
    }

    fn kv_selected_mut(&mut self) -> &mut usize {
        match self.focused_section {
            Section::Headers => &mut self.headers_selected,
            Section::Params => &mut self.params_selected,
            _ => &mut self.headers_selected,
        }
    }

    fn kv_list_len(&self) -> usize {
        match self.focused_section {
            Section::Headers => self.headers.len(),
            Section::Params => self.query_params.len(),
            _ => 0,
        }
    }

    pub fn kv_move_down(&mut self) {
        let len = self.kv_list_len();
        let sel = self.kv_selected_mut();
        if len > 0 && *sel < len - 1 {
            *sel += 1;
        }
    }

    pub fn kv_move_up(&mut self) {
        let sel = self.kv_selected_mut();
        if *sel > 0 {
            *sel -= 1;
        }
    }

    pub fn kv_goto_top(&mut self) {
        *self.kv_selected_mut() = 0;
    }

    pub fn kv_goto_bottom(&mut self) {
        let len = self.kv_list_len();
        if len > 0 {
            *self.kv_selected_mut() = len - 1;
        }
    }

    pub fn kv_add_row(&mut self) {
        let sel = match self.focused_section {
            Section::Headers => {
                let idx = if self.headers.is_empty() {
                    0
                } else {
                    self.headers_selected + 1
                };
                self.headers.insert(idx, KvRow::new_empty());
                self.headers_selected = idx;
                idx
            }
            Section::Params => {
                let idx = if self.query_params.is_empty() {
                    0
                } else {
                    self.params_selected + 1
                };
                self.query_params.insert(idx, KvRow::new_empty());
                self.params_selected = idx;
                idx
            }
            _ => return,
        };
        let _ = sel;
        self.dirty = true;
    }

    pub fn kv_delete_row(&mut self) {
        match self.focused_section {
            Section::Headers => {
                if !self.headers.is_empty() {
                    self.headers.remove(self.headers_selected);
                    if self.headers_selected >= self.headers.len() && !self.headers.is_empty() {
                        self.headers_selected = self.headers.len() - 1;
                    }
                    self.dirty = true;
                }
            }
            Section::Params => {
                if !self.query_params.is_empty() {
                    self.query_params.remove(self.params_selected);
                    if self.params_selected >= self.query_params.len()
                        && !self.query_params.is_empty()
                    {
                        self.params_selected = self.query_params.len() - 1;
                    }
                    self.dirty = true;
                }
            }
            _ => {}
        }
    }

    pub fn kv_toggle_enabled(&mut self) {
        if let Some(row) = self.kv_selected_row_mut() {
            row.enabled = !row.enabled;
            self.dirty = true;
        }
    }

    /// Start editing the selected KV row.
    pub fn kv_start_edit(&mut self) {
        let len = self.kv_list_len();
        if len == 0 {
            return;
        }
        let editing_field = self.editing_field;
        if let Some(row) = self.kv_selected_row_mut() {
            row.cursor = match editing_field {
                KvField::Key => row.key.len(),
                KvField::Value => row.value.len(),
            };
        }
        self.editing = true;
    }

    /// Insert a char in the current kv field being edited.
    pub fn kv_insert_char(&mut self, c: char) {
        let editing_field = self.editing_field;
        if let Some(row) = self.kv_selected_row_mut() {
            let field = match editing_field {
                KvField::Key => &mut row.key,
                KvField::Value => &mut row.value,
            };
            field.insert(row.cursor, c);
            row.cursor += c.len_utf8();
            self.dirty = true;
        }
    }

    pub fn kv_backspace(&mut self) {
        let editing_field = self.editing_field;
        if let Some(row) = self.kv_selected_row_mut() {
            if row.cursor > 0 {
                let field = match editing_field {
                    KvField::Key => &mut row.key,
                    KvField::Value => &mut row.value,
                };
                let prev = field[..row.cursor]
                    .chars()
                    .last()
                    .map(|c| c.len_utf8())
                    .unwrap_or(0);
                row.cursor -= prev;
                field.remove(row.cursor);
                self.dirty = true;
            }
        }
    }

    pub fn kv_cursor_left(&mut self) {
        let editing_field = self.editing_field;
        if let Some(row) = self.kv_selected_row_mut() {
            if row.cursor > 0 {
                let field_str = match editing_field {
                    KvField::Key => &row.key,
                    KvField::Value => &row.value,
                };
                let prev = field_str[..row.cursor]
                    .chars()
                    .last()
                    .map(|c| c.len_utf8())
                    .unwrap_or(0);
                row.cursor -= prev;
            }
        }
    }

    pub fn kv_cursor_right(&mut self) {
        let editing_field = self.editing_field;
        if let Some(row) = self.kv_selected_row_mut() {
            let field_len = match editing_field {
                KvField::Key => row.key.len(),
                KvField::Value => row.value.len(),
            };
            if row.cursor < field_len {
                let field_str = match editing_field {
                    KvField::Key => &row.key,
                    KvField::Value => &row.value,
                };
                let next = field_str[row.cursor..]
                    .chars()
                    .next()
                    .map(|c| c.len_utf8())
                    .unwrap_or(0);
                row.cursor += next;
            }
        }
    }

    /// Toggle between Key and Value field editing.
    pub fn kv_toggle_field(&mut self) {
        self.editing_field = match self.editing_field {
            KvField::Key => KvField::Value,
            KvField::Value => KvField::Key,
        };
        let editing_field = self.editing_field;
        if let Some(row) = self.kv_selected_row_mut() {
            row.cursor = match editing_field {
                KvField::Key => row.key.len(),
                KvField::Value => row.value.len(),
            };
        }
    }

    /// Stop editing and commit.
    pub fn kv_stop_edit(&mut self) {
        self.editing = false;
    }

    // ── Body editing ─────────────────────────────────────────────────

    pub fn body_insert_char(&mut self, c: char) {
        if let Some(line) = self.body_lines.get_mut(self.body_cursor_row) {
            line.insert(self.body_cursor_col, c);
            self.body_cursor_col += c.len_utf8();
            self.dirty = true;
        }
    }

    /// Insert a block of text at the cursor (for bracketed paste).
    pub fn body_insert_text(&mut self, text: &str) {
        for c in text.chars() {
            if c == '\n' {
                self.body_insert_newline();
            } else if c != '\r' {
                self.body_insert_char(c);
            }
        }
    }

    pub fn body_insert_newline(&mut self) {
        let current_line = self.body_lines[self.body_cursor_row].clone();
        let (before, after) = current_line.split_at(self.body_cursor_col);
        self.body_lines[self.body_cursor_row] = before.to_string();
        self.body_lines
            .insert(self.body_cursor_row + 1, after.to_string());
        self.body_cursor_row += 1;
        self.body_cursor_col = 0;
        self.dirty = true;
    }

    pub fn body_backspace(&mut self) {
        if self.body_cursor_col > 0 {
            let line = &self.body_lines[self.body_cursor_row];
            let prev = line[..self.body_cursor_col]
                .chars()
                .last()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.body_cursor_col -= prev;
            self.body_lines[self.body_cursor_row].remove(self.body_cursor_col);
            self.dirty = true;
        } else if self.body_cursor_row > 0 {
            // Merge with previous line
            let current = self.body_lines.remove(self.body_cursor_row);
            self.body_cursor_row -= 1;
            self.body_cursor_col = self.body_lines[self.body_cursor_row].len();
            self.body_lines[self.body_cursor_row].push_str(&current);
            self.dirty = true;
        }
    }

    pub fn body_delete(&mut self) {
        let line_len = self.body_lines[self.body_cursor_row].len();
        if self.body_cursor_col < line_len {
            self.body_lines[self.body_cursor_row].remove(self.body_cursor_col);
            self.dirty = true;
        } else if self.body_cursor_row < self.body_lines.len() - 1 {
            // Merge next line into current
            let next = self.body_lines.remove(self.body_cursor_row + 1);
            self.body_lines[self.body_cursor_row].push_str(&next);
            self.dirty = true;
        }
    }

    pub fn body_cursor_left(&mut self) {
        if self.body_cursor_col > 0 {
            let line = &self.body_lines[self.body_cursor_row];
            let prev = line[..self.body_cursor_col]
                .chars()
                .last()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.body_cursor_col -= prev;
        }
    }

    pub fn body_cursor_right(&mut self) {
        let line_len = self.body_lines[self.body_cursor_row].len();
        if self.body_cursor_col < line_len {
            let line = &self.body_lines[self.body_cursor_row];
            let next = line[self.body_cursor_col..]
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.body_cursor_col += next;
        }
    }

    pub fn body_cursor_up(&mut self) {
        if self.body_cursor_row > 0 {
            self.body_cursor_row -= 1;
            let line_len = self.body_lines[self.body_cursor_row].len();
            self.body_cursor_col = self.body_cursor_col.min(line_len);
        }
    }

    pub fn body_cursor_down(&mut self) {
        if self.body_cursor_row < self.body_lines.len() - 1 {
            self.body_cursor_row += 1;
            let line_len = self.body_lines[self.body_cursor_row].len();
            self.body_cursor_col = self.body_cursor_col.min(line_len);
        }
    }

    pub fn body_cursor_home(&mut self) {
        self.body_cursor_col = 0;
    }

    pub fn body_cursor_end(&mut self) {
        self.body_cursor_col = self.body_lines[self.body_cursor_row].len();
    }

    pub fn body_goto_top(&mut self) {
        self.body_cursor_row = 0;
        self.body_cursor_col = 0;
    }

    pub fn body_goto_bottom(&mut self) {
        self.body_cursor_row = self.body_lines.len() - 1;
        self.body_cursor_col = self.body_lines[self.body_cursor_row].len();
    }

    // ── Section navigation ───────────────────────────────────────────

    pub fn next_section(&mut self) {
        if self.panel_focus == PanelFocus::Response {
            // Tab within response toggles body/headers
            if let Some(ref mut resp) = self.response {
                resp.toggle_section();
            }
            return;
        }
        self.editing = false;
        self.focused_section = self.focused_section.next();
    }

    pub fn prev_section(&mut self) {
        if self.panel_focus == PanelFocus::Response {
            if let Some(ref mut resp) = self.response {
                resp.toggle_section();
            }
            return;
        }
        self.editing = false;
        self.focused_section = self.focused_section.prev();
    }

    /// Move focus to the response panel.
    pub fn focus_response(&mut self) {
        self.editing = false;
        self.panel_focus = PanelFocus::Response;
    }

    /// Move focus back to the request panel.
    pub fn focus_request(&mut self) {
        self.panel_focus = PanelFocus::Request;
    }

    /// Toggle fullscreen for the currently focused panel.
    /// Pressing `f` when already fullscreen exits back to the split view.
    pub fn toggle_fullscreen(&mut self) {
        match self.fullscreen {
            Some(_) => self.fullscreen = None,
            None => self.fullscreen = Some(self.panel_focus),
        }
    }

    // ── Spinner ──────────────────────────────────────────────────────

    pub fn tick_spinner(&mut self) {
        if self.request_in_flight {
            self.spinner_frame = (self.spinner_frame + 1) % 10;
        }
    }

    pub fn spinner_char(&self) -> char {
        const FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        FRAMES[self.spinner_frame as usize]
    }

    // ── Build request URL with params ────────────────────────────────

    /// Build the full URL with enabled query params appended.
    pub fn build_url_with_params(&self) -> String {
        let enabled_params: Vec<_> = self
            .query_params
            .iter()
            .filter(|p| p.enabled && !p.key.is_empty())
            .collect();

        if enabled_params.is_empty() {
            return self.url.clone();
        }

        let separator = if self.url.contains('?') { "&" } else { "?" };
        let params_str: Vec<String> = enabled_params
            .iter()
            .map(|p| format!("{}={}", p.key, p.value))
            .collect();

        format!("{}{}{}", self.url, separator, params_str.join("&"))
    }

    /// Collect enabled headers as (key, value) pairs.
    pub fn enabled_headers(&self) -> Vec<(String, String)> {
        self.headers
            .iter()
            .filter(|h| h.enabled && !h.key.is_empty())
            .map(|h| (h.key.clone(), h.value.clone()))
            .collect()
    }

    /// Get the body as a single string.
    pub fn body_text(&self) -> String {
        self.body_lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstools_core::db::open_memory_db;

    fn setup() -> (RequestPanel, Connection) {
        let conn = open_memory_db().unwrap();
        model::init_db(&conn).unwrap();
        (RequestPanel::new(), conn)
    }

    #[test]
    fn test_new_panel_is_inactive() {
        let panel = RequestPanel::new();
        assert!(!panel.is_active());
        assert!(!panel.dirty);
    }

    #[test]
    fn test_load_and_save() {
        let (mut panel, conn) = setup();
        let entry_id = model::add_entry(&conn, None, "test", model::EntryType::Query).unwrap();

        panel.load(entry_id, "test", &conn).unwrap();
        assert!(panel.is_active());
        assert_eq!(panel.method, HttpMethod::Get);
        assert!(!panel.dirty);

        // Modify and save
        panel.method = HttpMethod::Post;
        panel.url = "https://example.com".to_string();
        panel.dirty = true;
        panel.save(&conn).unwrap();
        assert!(!panel.dirty);

        // Reload and verify
        panel.load(entry_id, "test", &conn).unwrap();
        assert_eq!(panel.method, HttpMethod::Post);
        assert_eq!(panel.url, "https://example.com");
    }

    #[test]
    fn test_url_editing() {
        let mut panel = RequestPanel::new();
        panel.url_insert_char('h');
        panel.url_insert_char('i');
        assert_eq!(panel.url, "hi");
        assert_eq!(panel.url_cursor, 2);

        panel.url_backspace();
        assert_eq!(panel.url, "h");
        assert_eq!(panel.url_cursor, 1);

        panel.url_cursor_left();
        assert_eq!(panel.url_cursor, 0);
        panel.url_cursor_right();
        assert_eq!(panel.url_cursor, 1);
    }

    #[test]
    fn test_kv_operations() {
        let mut panel = RequestPanel::new();
        panel.active_entry_id = Some(1);
        panel.focused_section = Section::Headers;

        // Add rows
        panel.kv_add_row();
        panel.kv_add_row();
        assert_eq!(panel.headers.len(), 2);

        // Navigate
        panel.kv_move_up();
        assert_eq!(panel.headers_selected, 0);
        panel.kv_move_down();
        assert_eq!(panel.headers_selected, 1);

        // Toggle
        panel.kv_toggle_enabled();
        assert!(!panel.headers[1].enabled);

        // Delete
        panel.kv_delete_row();
        assert_eq!(panel.headers.len(), 1);
    }

    #[test]
    fn test_body_editing() {
        let mut panel = RequestPanel::new();
        panel.body_insert_char('{');
        panel.body_insert_newline();
        panel.body_insert_char('}');
        assert_eq!(panel.body_lines, vec!["{", "}"]);
        assert_eq!(panel.body_cursor_row, 1);
        assert_eq!(panel.body_cursor_col, 1);

        panel.body_backspace();
        assert_eq!(panel.body_lines, vec!["{", ""]);
        panel.body_backspace(); // merge lines
        assert_eq!(panel.body_lines, vec!["{"]);
    }

    #[test]
    fn test_section_cycling() {
        let mut panel = RequestPanel::new();
        assert_eq!(panel.focused_section, Section::Url);
        panel.next_section();
        assert_eq!(panel.focused_section, Section::Params);
        panel.next_section();
        assert_eq!(panel.focused_section, Section::Headers);
        panel.next_section();
        assert_eq!(panel.focused_section, Section::Body);
        panel.next_section();
        assert_eq!(panel.focused_section, Section::Url);

        panel.prev_section();
        assert_eq!(panel.focused_section, Section::Body);
    }

    #[test]
    fn test_build_url_with_params() {
        let mut panel = RequestPanel::new();
        panel.url = "https://api.example.com/users".to_string();

        panel.query_params.push(KvRow {
            db_id: 0,
            key: "page".to_string(),
            value: "1".to_string(),
            enabled: true,
            cursor: 0,
        });
        panel.query_params.push(KvRow {
            db_id: 0,
            key: "limit".to_string(),
            value: "10".to_string(),
            enabled: true,
            cursor: 0,
        });
        panel.query_params.push(KvRow {
            db_id: 0,
            key: "debug".to_string(),
            value: "true".to_string(),
            enabled: false,
            cursor: 0,
        });

        let url = panel.build_url_with_params();
        assert_eq!(url, "https://api.example.com/users?page=1&limit=10");
    }

    #[test]
    fn test_clear_panel() {
        let mut panel = RequestPanel::new();
        panel.url = "https://example.com".to_string();
        panel.dirty = true;
        panel.active_entry_id = Some(1);

        panel.clear();
        assert!(!panel.is_active());
        assert!(!panel.dirty);
        assert!(panel.url.is_empty());
    }

    #[test]
    fn test_kv_save_and_load_roundtrip() {
        let (mut panel, conn) = setup();
        let entry_id = model::add_entry(&conn, None, "test", model::EntryType::Query).unwrap();

        panel.load(entry_id, "test", &conn).unwrap();

        // Add headers
        panel.focused_section = Section::Headers;
        panel.kv_add_row();
        panel.headers[0].key = "Content-Type".to_string();
        panel.headers[0].value = "application/json".to_string();

        // Add params
        panel.focused_section = Section::Params;
        panel.kv_add_row();
        panel.query_params[0].key = "page".to_string();
        panel.query_params[0].value = "1".to_string();

        panel.dirty = true;
        panel.save(&conn).unwrap();

        // Reload
        let mut panel2 = RequestPanel::new();
        panel2.load(entry_id, "test", &conn).unwrap();
        assert_eq!(panel2.headers.len(), 1);
        assert_eq!(panel2.headers[0].key, "Content-Type");
        assert_eq!(panel2.query_params.len(), 1);
        assert_eq!(panel2.query_params[0].key, "page");
    }
}
