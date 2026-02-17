pub mod model;
pub mod ui;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    widgets::ListState,
    Frame,
};
use rusqlite::Connection;

use rstools_core::{
    keybinds::{process_normal_key, Action, InputMode, KeyState},
    telescope::TelescopeItem,
    tool::Tool,
    which_key::WhichKeyEntry,
};

use model::Todo;

/// The editing context when in Insert mode.
#[derive(Debug, Clone)]
enum EditContext {
    /// Adding a new todo.
    Adding,
    /// Editing an existing todo (by index in the filtered list).
    Editing(usize),
    /// Search/filter input.
    Filtering,
}

/// The Todo tool — a minimalist todo list with vim-style navigation.
pub struct TodoTool {
    /// All todos loaded from the database.
    todos: Vec<Todo>,
    /// Indices into `todos` after filtering.
    filtered: Vec<usize>,
    /// List selection state.
    list_state: ListState,
    /// Current input mode.
    mode: InputMode,
    /// Key state for multi-key sequences.
    key_state: KeyState,
    /// Text input buffer (for add/edit/filter).
    input: String,
    /// Cursor position in the input buffer.
    input_cursor: usize,
    /// What we're editing, if in Insert mode.
    edit_context: Option<EditContext>,
    /// Current filter string.
    filter: Option<String>,
    /// Database connection.
    conn: Connection,
}

impl TodoTool {
    pub fn new(conn: Connection) -> anyhow::Result<Self> {
        let mut tool = Self {
            todos: Vec::new(),
            filtered: Vec::new(),
            list_state: ListState::default(),
            mode: InputMode::Normal,
            key_state: KeyState::default(),
            input: String::new(),
            input_cursor: 0,
            edit_context: None,
            filter: None,
            conn,
        };
        model::init_db(&tool.conn)?;
        tool.reload()?;
        Ok(tool)
    }

    /// Reload todos from the database.
    fn reload(&mut self) -> anyhow::Result<()> {
        self.todos = model::list_todos(&self.conn)?;
        self.apply_filter();
        Ok(())
    }

    /// Apply the current filter to the todo list.
    fn apply_filter(&mut self) {
        if let Some(ref filter) = self.filter {
            let f = filter.to_lowercase();
            self.filtered = self
                .todos
                .iter()
                .enumerate()
                .filter(|(_, t)| t.title.to_lowercase().contains(&f))
                .map(|(i, _)| i)
                .collect();
        } else {
            self.filtered = (0..self.todos.len()).collect();
        }

        // Keep selection in bounds
        if self.filtered.is_empty() {
            self.list_state.select(None);
        } else {
            let sel = self.list_state.selected().unwrap_or(0);
            if sel >= self.filtered.len() {
                self.list_state
                    .select(Some(self.filtered.len().saturating_sub(1)));
            } else if self.list_state.selected().is_none() {
                self.list_state.select(Some(0));
            }
        }
    }

    /// Get the currently selected todo (if any).
    fn selected_todo(&self) -> Option<&Todo> {
        let sel = self.list_state.selected()?;
        let idx = *self.filtered.get(sel)?;
        self.todos.get(idx)
    }

    /// Get the currently selected todo's id.
    fn selected_todo_id(&self) -> Option<i64> {
        self.selected_todo().map(|t| t.id)
    }

    /// Start adding a new todo.
    fn start_add(&mut self) {
        self.mode = InputMode::Insert;
        self.input.clear();
        self.input_cursor = 0;
        self.edit_context = Some(EditContext::Adding);
    }

    /// Start editing the selected todo.
    fn start_edit(&mut self) {
        let sel = self.list_state.selected().unwrap_or(0);
        let title = self.selected_todo().map(|t| t.title.clone());
        if let Some(title) = title {
            self.mode = InputMode::Insert;
            self.input_cursor = title.len();
            self.input = title;
            self.edit_context = Some(EditContext::Editing(sel));
        }
    }

    /// Start search/filter mode.
    fn start_filter(&mut self) {
        self.mode = InputMode::Insert;
        self.input = self.filter.clone().unwrap_or_default();
        self.input_cursor = self.input.len();
        self.edit_context = Some(EditContext::Filtering);
    }

    /// Submit the current input.
    fn submit_input(&mut self) {
        let input = self.input.trim().to_string();
        match self.edit_context.take() {
            Some(EditContext::Adding) => {
                if !input.is_empty() {
                    let _ = model::add_todo(&self.conn, &input, None);
                    let _ = self.reload();
                }
            }
            Some(EditContext::Editing(idx)) => {
                if !input.is_empty() {
                    if let Some(&todo_idx) = self.filtered.get(idx) {
                        if let Some(todo) = self.todos.get(todo_idx) {
                            let _ = model::update_todo(
                                &self.conn,
                                todo.id,
                                &input,
                                todo.description.as_deref(),
                            );
                            let _ = self.reload();
                        }
                    }
                }
            }
            Some(EditContext::Filtering) => {
                if input.is_empty() {
                    self.filter = None;
                } else {
                    self.filter = Some(input);
                }
                self.apply_filter();
            }
            None => {}
        }
        self.mode = InputMode::Normal;
        self.input.clear();
        self.input_cursor = 0;
    }

    /// Cancel the current input.
    fn cancel_input(&mut self) {
        self.edit_context = None;
        self.mode = InputMode::Normal;
        self.input.clear();
        self.input_cursor = 0;
    }

    /// Handle key events in Insert mode (text input).
    fn handle_insert_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc => {
                self.cancel_input();
                Action::SetMode(InputMode::Normal)
            }
            KeyCode::Enter => {
                self.submit_input();
                Action::SetMode(InputMode::Normal)
            }
            KeyCode::Char(c) => {
                self.input.insert(self.input_cursor, c);
                self.input_cursor += c.len_utf8();
                // Live filter update
                if matches!(self.edit_context, Some(EditContext::Filtering)) {
                    let input = self.input.clone();
                    if input.is_empty() {
                        self.filter = None;
                    } else {
                        self.filter = Some(input);
                    }
                    self.apply_filter();
                }
                Action::None
            }
            KeyCode::Backspace => {
                if self.input_cursor > 0 {
                    let prev = self.input[..self.input_cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    self.input.drain(prev..self.input_cursor);
                    self.input_cursor = prev;
                    // Live filter update
                    if matches!(self.edit_context, Some(EditContext::Filtering)) {
                        let input = self.input.clone();
                        if input.is_empty() {
                            self.filter = None;
                        } else {
                            self.filter = Some(input);
                        }
                        self.apply_filter();
                    }
                }
                Action::None
            }
            KeyCode::Left => {
                if self.input_cursor > 0 {
                    let prev = self.input[..self.input_cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    self.input_cursor = prev;
                }
                Action::None
            }
            KeyCode::Right => {
                if self.input_cursor < self.input.len() {
                    let next = self.input[self.input_cursor..]
                        .char_indices()
                        .nth(1)
                        .map(|(i, _)| self.input_cursor + i)
                        .unwrap_or(self.input.len());
                    self.input_cursor = next;
                }
                Action::None
            }
            _ => Action::None,
        }
    }

    /// Get the filtered todos for display.
    fn visible_todos(&self) -> Vec<&Todo> {
        self.filtered
            .iter()
            .filter_map(|&i| self.todos.get(i))
            .collect()
    }

    /// Current mode getter (used by the hub for status bar).
    pub fn mode(&self) -> InputMode {
        self.mode
    }
}

impl Tool for TodoTool {
    fn name(&self) -> &str {
        "Todo"
    }

    fn description(&self) -> &str {
        "Minimalist todo list"
    }

    fn mode(&self) -> InputMode {
        self.mode
    }

    fn init_db(&self, conn: &Connection) -> anyhow::Result<()> {
        model::init_db(conn)
    }

    fn which_key_entries(&self) -> Vec<WhichKeyEntry> {
        vec![
            WhichKeyEntry::action("a", "Add todo"),
            WhichKeyEntry::action("d", "Delete todo"),
        ]
    }

    fn telescope_items(&self) -> Vec<TelescopeItem> {
        self.todos
            .iter()
            .map(|t| TelescopeItem {
                label: t.title.clone(),
                description: if t.completed {
                    "done".to_string()
                } else {
                    String::new()
                },
                id: format!("todo:{}", t.id),
            })
            .collect()
    }

    fn handle_key(&mut self, key: KeyEvent) -> Action {
        match self.mode {
            InputMode::Insert => self.handle_insert_key(key),
            InputMode::Normal => {
                let action = process_normal_key(key, &mut self.key_state);
                match action {
                    Action::MoveDown(n) => {
                        if !self.filtered.is_empty() {
                            let sel = self.list_state.selected().unwrap_or(0);
                            let next = (sel + n).min(self.filtered.len() - 1);
                            self.list_state.select(Some(next));
                        }
                        Action::None
                    }
                    Action::MoveUp(n) => {
                        if !self.filtered.is_empty() {
                            let sel = self.list_state.selected().unwrap_or(0);
                            let next = sel.saturating_sub(n);
                            self.list_state.select(Some(next));
                        }
                        Action::None
                    }
                    Action::GotoTop => {
                        if !self.filtered.is_empty() {
                            self.list_state.select(Some(0));
                        }
                        Action::None
                    }
                    Action::GotoBottom => {
                        if !self.filtered.is_empty() {
                            self.list_state
                                .select(Some(self.filtered.len().saturating_sub(1)));
                        }
                        Action::None
                    }
                    Action::HalfPageDown => {
                        if !self.filtered.is_empty() {
                            let sel = self.list_state.selected().unwrap_or(0);
                            let next = (sel + 10).min(self.filtered.len() - 1);
                            self.list_state.select(Some(next));
                        }
                        Action::None
                    }
                    Action::HalfPageUp => {
                        if !self.filtered.is_empty() {
                            let sel = self.list_state.selected().unwrap_or(0);
                            let next = sel.saturating_sub(10);
                            self.list_state.select(Some(next));
                        }
                        Action::None
                    }
                    Action::Confirm => {
                        if let Some(id) = self.selected_todo_id() {
                            let _ = model::toggle_todo(&self.conn, id);
                            let _ = self.reload();
                        }
                        Action::None
                    }
                    Action::Delete => {
                        if let Some(id) = self.selected_todo_id() {
                            let _ = model::delete_todo(&self.conn, id);
                            let _ = self.reload();
                        }
                        Action::None
                    }
                    Action::Add | Action::AddBelow => {
                        self.start_add();
                        Action::None
                    }
                    Action::Edit => {
                        self.start_edit();
                        Action::None
                    }
                    Action::Search => {
                        self.start_filter();
                        Action::None
                    }
                    // These actions bubble up to the hub
                    Action::Quit
                    | Action::LeaderKey
                    | Action::LeaderSequence(_)
                    | Action::SwitchTool(_)
                    | Action::NextTool
                    | Action::PrevTool
                    | Action::ToolPicker
                    | Action::Telescope
                    | Action::Help => action,
                    _ => Action::None,
                }
            }
            InputMode::Command => {
                // Command mode is handled by the hub
                Action::None
            }
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let visible: Vec<Todo> = self.visible_todos().into_iter().cloned().collect();

        if self.mode == InputMode::Insert && self.edit_context.is_some() {
            // Split area: list + input at bottom
            let [list_area, input_area] =
                Layout::vertical([Constraint::Min(1), Constraint::Length(3)]).areas(area);

            let mut state = self.list_state.clone();
            ui::render_todo_list(
                frame,
                list_area,
                &visible,
                &mut state,
                self.filter.as_deref(),
            );

            let prompt = match &self.edit_context {
                Some(EditContext::Adding) => "New Todo",
                Some(EditContext::Editing(_)) => "Edit Todo",
                Some(EditContext::Filtering) => "Filter",
                None => "",
            };
            ui::render_todo_input(frame, input_area, prompt, &self.input, self.input_cursor);
        } else {
            let mut state = self.list_state.clone();
            ui::render_todo_list(frame, area, &visible, &mut state, self.filter.as_deref());
        }
    }

    fn on_focus(&mut self) {
        let _ = self.reload();
    }
}
