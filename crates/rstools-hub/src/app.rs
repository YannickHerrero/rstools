use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{layout::Rect, Frame};
use rusqlite::Connection;

use rstools_core::{
    keybinds::{Action, InputMode},
    telescope::{Telescope, TelescopeItem},
    tool::Tool,
    ui,
    which_key::{self, WhichKey},
};

/// The main application state.
pub struct App {
    /// Registry of all available tools.
    tools: Vec<Box<dyn Tool>>,
    /// Index of the currently active tool (None = dashboard).
    active_tool: Option<usize>,
    /// Whether the app should quit.
    pub should_quit: bool,
    /// Current global input mode.
    mode: InputMode,
    /// Which-key popup state.
    which_key: WhichKey,
    /// Telescope overlay state.
    telescope: Telescope,
    /// Command-line input buffer.
    command_input: String,
    /// Command-line cursor position.
    command_cursor: usize,
}

impl App {
    /// Create a new App with the given tools.
    pub fn new(tools: Vec<Box<dyn Tool>>) -> Self {
        Self {
            tools,
            active_tool: None,
            should_quit: false,
            mode: InputMode::Normal,
            which_key: WhichKey::new(),
            telescope: Telescope::new(),
            command_input: String::new(),
            command_cursor: 0,
        }
    }

    /// Initialize all tool databases.
    pub fn init_db(&self, conn: &Connection) -> Result<()> {
        for tool in &self.tools {
            tool.init_db(conn)?;
        }
        Ok(())
    }

    /// Handle a terminal event.
    pub fn handle_event(&mut self, event: Event) {
        if let Event::Key(key) = event {
            // Ctrl-c always quits
            if key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL {
                self.should_quit = true;
                return;
            }

            // Handle telescope if active
            if self.telescope.visible {
                self.handle_telescope_key(key);
                return;
            }

            // Handle which-key if active
            if self.which_key.visible {
                self.handle_which_key_input(key);
                return;
            }

            // Handle command mode
            if self.mode == InputMode::Command {
                self.handle_command_key(key);
                return;
            }

            // Delegate to active tool or handle globally
            if let Some(idx) = self.active_tool {
                let action = self.tools[idx].handle_key(key);
                self.process_action(action);
            } else {
                // Dashboard mode — handle global keys
                self.handle_dashboard_key(key);
            }
        }
    }

    /// Process an action returned by a tool or global key handler.
    fn process_action(&mut self, action: Action) {
        match action {
            Action::Quit => {
                if self.active_tool.is_some() {
                    // Close current tool, go back to dashboard
                    if let Some(idx) = self.active_tool {
                        self.tools[idx].on_blur();
                    }
                    self.active_tool = None;
                } else {
                    self.should_quit = true;
                }
            }
            Action::LeaderKey => {
                self.show_leader_menu();
            }
            Action::LeaderSequence(c) => {
                self.handle_leader_sequence(c);
            }
            Action::SetMode(mode) => {
                self.mode = mode;
                if mode == InputMode::Command {
                    self.command_input.clear();
                    self.command_cursor = 0;
                }
            }
            Action::SwitchTool(idx) => {
                self.switch_to_tool(idx);
            }
            Action::NextTool => {
                if !self.tools.is_empty() {
                    let current = self.active_tool.unwrap_or(0);
                    let next = (current + 1) % self.tools.len();
                    self.switch_to_tool(next);
                }
            }
            Action::PrevTool => {
                if !self.tools.is_empty() {
                    let current = self.active_tool.unwrap_or(0);
                    let prev = if current == 0 {
                        self.tools.len() - 1
                    } else {
                        current - 1
                    };
                    self.switch_to_tool(prev);
                }
            }
            Action::ToolPicker => {
                self.open_tool_picker();
            }
            Action::Telescope => {
                self.open_telescope();
            }
            _ => {}
        }
    }

    /// Show the leader key which-key menu.
    fn show_leader_menu(&mut self) {
        let entries = which_key::hub_leader_entries();
        // Add tool-specific groups
        for (i, tool) in self.tools.iter().enumerate() {
            // Tools are already represented in hub_leader_entries
            // This is where we'd add dynamic entries
            let _ = (i, tool);
        }
        self.which_key.show("Leader", entries);
    }

    /// Handle a key press after the leader key.
    fn handle_leader_sequence(&mut self, c: char) {
        match c {
            't' => {
                // Show todo sub-menu or switch to todo
                if let Some(idx) = self.tools.iter().position(|t| t.name() == "Todo") {
                    let entries = self.tools[idx].which_key_entries();
                    if entries.is_empty() {
                        self.switch_to_tool(idx);
                    } else {
                        self.which_key.show("Todo", entries);
                    }
                }
            }
            _ => {}
        }
    }

    /// Handle input while which-key is visible.
    fn handle_which_key_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.which_key.hide();
            }
            KeyCode::Char(c) => {
                self.which_key.hide();
                // Process the which-key selection
                match c {
                    'q' => {
                        self.process_action(Action::Quit);
                    }
                    'f' => {
                        self.open_telescope();
                    }
                    't' => {
                        // Switch to todo tool
                        if let Some(idx) = self.tools.iter().position(|t| t.name() == "Todo") {
                            self.switch_to_tool(idx);
                        }
                    }
                    ' ' => {
                        self.open_tool_picker();
                    }
                    c @ '1'..='9' => {
                        let idx = (c as u8 - b'1') as usize;
                        self.switch_to_tool(idx);
                    }
                    _ => {}
                }
            }
            _ => {
                self.which_key.hide();
            }
        }
    }

    /// Handle telescope key events.
    fn handle_telescope_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.telescope.close();
            }
            KeyCode::Enter => {
                if let Some(id) = self.telescope.selected_id() {
                    let id = id.to_string();
                    self.telescope.close();
                    self.handle_telescope_selection(&id);
                }
            }
            KeyCode::Up | KeyCode::BackTab => {
                self.telescope.move_up();
            }
            KeyCode::Down | KeyCode::Tab => {
                self.telescope.move_down();
            }
            KeyCode::Char(c) => {
                self.telescope.insert_char(c);
            }
            KeyCode::Backspace => {
                self.telescope.backspace();
            }
            _ => {}
        }
    }

    /// Handle a telescope selection.
    fn handle_telescope_selection(&mut self, id: &str) {
        if id.starts_with("tool:") {
            let tool_name = &id[5..];
            if let Some(idx) = self.tools.iter().position(|t| t.name() == tool_name) {
                self.switch_to_tool(idx);
            }
        }
        // Future: handle "todo:<id>" etc.
    }

    /// Handle command-mode key events.
    fn handle_command_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = InputMode::Normal;
                self.command_input.clear();
                self.command_cursor = 0;
            }
            KeyCode::Enter => {
                let cmd = self.command_input.trim().to_string();
                self.mode = InputMode::Normal;
                self.command_input.clear();
                self.command_cursor = 0;
                self.execute_command(&cmd);
            }
            KeyCode::Char(c) => {
                self.command_input.insert(self.command_cursor, c);
                self.command_cursor += c.len_utf8();
            }
            KeyCode::Backspace => {
                if self.command_cursor > 0 {
                    let prev = self.command_input[..self.command_cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    self.command_input.drain(prev..self.command_cursor);
                    self.command_cursor = prev;
                }
            }
            KeyCode::Left => {
                if self.command_cursor > 0 {
                    let prev = self.command_input[..self.command_cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    self.command_cursor = prev;
                }
            }
            KeyCode::Right => {
                if self.command_cursor < self.command_input.len() {
                    let next = self.command_input[self.command_cursor..]
                        .char_indices()
                        .nth(1)
                        .map(|(i, _)| self.command_cursor + i)
                        .unwrap_or(self.command_input.len());
                    self.command_cursor = next;
                }
            }
            _ => {}
        }
    }

    /// Execute a command-mode command.
    fn execute_command(&mut self, cmd: &str) {
        match cmd {
            "q" | "quit" => {
                if self.active_tool.is_some() {
                    if let Some(idx) = self.active_tool {
                        self.tools[idx].on_blur();
                    }
                    self.active_tool = None;
                } else {
                    self.should_quit = true;
                }
            }
            "qa" | "qa!" => {
                self.should_quit = true;
            }
            _ => {
                // Unknown command — could show an error message in the future
            }
        }
    }

    /// Handle keys when on the dashboard (no tool active).
    fn handle_dashboard_key(&mut self, key: KeyEvent) {
        use rstools_core::keybinds::{process_normal_key, KeyState};

        // Use a temporary key state for dashboard
        let mut key_state = KeyState::default();
        let action = process_normal_key(key, &mut key_state);
        self.process_action(action);
    }

    /// Switch to a tool by index.
    fn switch_to_tool(&mut self, idx: usize) {
        if idx < self.tools.len() {
            if let Some(old) = self.active_tool {
                self.tools[old].on_blur();
            }
            self.active_tool = Some(idx);
            self.tools[idx].on_focus();
            self.mode = InputMode::Normal;
        }
    }

    /// Open the tool picker telescope.
    fn open_tool_picker(&mut self) {
        let items: Vec<TelescopeItem> = self
            .tools
            .iter()
            .map(|t| TelescopeItem {
                label: t.name().to_string(),
                description: t.description().to_string(),
                id: format!("tool:{}", t.name()),
            })
            .collect();
        self.telescope.open("Tool Picker", items);
    }

    /// Open the general telescope (search across all tools).
    fn open_telescope(&mut self) {
        let mut items: Vec<TelescopeItem> = Vec::new();

        // Add tools themselves
        for tool in &self.tools {
            items.push(TelescopeItem {
                label: tool.name().to_string(),
                description: tool.description().to_string(),
                id: format!("tool:{}", tool.name()),
            });
        }

        // Add items from each tool
        for tool in &self.tools {
            items.extend(tool.telescope_items());
        }

        self.telescope.open("Find", items);
    }

    /// Render the entire application.
    pub fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let (tab_area, content_area, status_area) = ui::standard_layout(area);

        // Tab bar
        let tool_names: Vec<&str> = self.tools.iter().map(|t| t.name()).collect();
        let active_tab = self.active_tool.unwrap_or(0);
        if !tool_names.is_empty() {
            ui::render_tab_bar(frame, tab_area, &tool_names, active_tab);
        }

        // Main content
        if let Some(idx) = self.active_tool {
            self.tools[idx].render(frame, content_area);
        } else {
            self.render_dashboard(frame, content_area);
        }

        // Status bar or command line
        if self.mode == InputMode::Command {
            ui::render_command_line(frame, status_area, &self.command_input, self.command_cursor);
        } else {
            let tool_name = self
                .active_tool
                .map(|i| self.tools[i].name())
                .unwrap_or("Dashboard");

            // Get mode from active tool for accurate status bar
            let mode = match self.active_tool {
                Some(idx) => self.tools[idx].mode(),
                None => self.mode,
            };
            let info = match self.active_tool {
                Some(_) => "Space: leader  ?:help  :q: close",
                None => "Space: leader  ?:help  :q: quit",
            };
            ui::render_status_bar(frame, status_area, mode, tool_name, info);
        }

        // Overlays (rendered last, on top)
        self.which_key.render(frame, area);
        self.telescope.render(frame, area);
    }

    /// Render the dashboard when no tool is active.
    fn render_dashboard(&self, frame: &mut Frame, area: Rect) {
        use ratatui::{
            layout::{Alignment, Constraint, Layout},
            style::{Color, Modifier, Style},
            text::{Line, Span},
            widgets::Paragraph,
        };

        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "rstools",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "A vim-centric terminal toolset",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(""),
            Line::from(""),
            Line::from(vec![
                Span::styled("  <Space> ", Style::default().fg(Color::Yellow)),
                Span::raw("  Open leader menu"),
            ]),
            Line::from(vec![
                Span::styled("  <Space><Space> ", Style::default().fg(Color::Yellow)),
                Span::raw("  Tool picker"),
            ]),
            Line::from(vec![
                Span::styled("  <Space>f ", Style::default().fg(Color::Yellow)),
                Span::raw("  Find (telescope)"),
            ]),
            Line::from(vec![
                Span::styled("  :q ", Style::default().fg(Color::Yellow)),
                Span::raw("  Quit"),
            ]),
            Line::from(""),
            Line::from(""),
        ];

        // Add tool list
        let mut all_lines = lines;
        if !self.tools.is_empty() {
            all_lines.push(Line::from(Span::styled(
                "Available tools:",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )));
            all_lines.push(Line::from(""));
            for (i, tool) in self.tools.iter().enumerate() {
                all_lines.push(Line::from(vec![
                    Span::styled(format!("  {} ", i + 1), Style::default().fg(Color::Yellow)),
                    Span::styled(
                        tool.name(),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("  {}", tool.description()),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
            }
        }

        let paragraph = Paragraph::new(all_lines).alignment(Alignment::Center);

        // Center vertically
        let [_, centered, _] = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(20),
            Constraint::Fill(1),
        ])
        .areas(area);

        frame.render_widget(paragraph, centered);
    }
}
