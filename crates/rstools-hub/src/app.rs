use anyhow::Result;
use crossterm::cursor::SetCursorStyle;
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::{layout::Rect, Frame};
use rusqlite::Connection;

use rstools_core::{
    help_popup::{self, HelpPopup},
    keybinds::{Action, InputMode, KeyState},
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
    /// Help popup state.
    help_popup: HelpPopup,
    /// Telescope overlay state.
    telescope: Telescope,
    /// Command-line input buffer.
    command_input: String,
    /// Command-line cursor position.
    command_cursor: usize,
    /// Key state for dashboard (persistent so gg/dd work).
    key_state: KeyState,
    /// Cached layout areas from the last render (for mouse hit-testing).
    last_tab_area: Rect,
    last_content_area: Rect,
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
            help_popup: HelpPopup::new(),
            telescope: Telescope::new(),
            command_input: String::new(),
            command_cursor: 0,
            key_state: KeyState::default(),
            last_tab_area: Rect::default(),
            last_content_area: Rect::default(),
        }
    }

    /// Reset all pending key state (hub + active tool).
    /// Called when the hub takes over input for overlays.
    fn reset_all_key_state(&mut self) {
        self.key_state.reset();
        if let Some(idx) = self.active_tool {
            self.tools[idx].reset_key_state();
        }
    }

    /// Initialize all tool databases.
    pub fn init_db(&self, conn: &Connection) -> Result<()> {
        for tool in &self.tools {
            tool.init_db(conn)?;
        }
        Ok(())
    }

    /// Tick the active tool (called every ~50ms for async polling, animations, etc.).
    pub fn tick(&mut self) {
        if let Some(idx) = self.active_tool {
            self.tools[idx].tick();
        }
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

            // Handle help popup if active
            if self.help_popup.visible {
                self.handle_help_key(key);
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
        } else if let Event::Paste(text) = event {
            self.handle_paste_event(&text);
        } else if let Event::Mouse(mouse) = event {
            self.handle_mouse_event(mouse);
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
            Action::Help => {
                self.show_help();
            }
            _ => {}
        }
    }

    /// Show the leader key which-key menu.
    fn show_leader_menu(&mut self) {
        self.reset_all_key_state();
        let mut entries = which_key::hub_leader_entries();
        // Add context-specific entries based on active tool
        if let Some(idx) = self.active_tool {
            let tool_entries = self.tools[idx].which_key_entries();
            for entry in tool_entries.into_iter().rev() {
                entries.insert(0, entry);
            }
        }
        self.which_key.show("Leader", entries);
    }

    /// Handle a key press after the leader key.
    fn handle_leader_sequence(&mut self, c: char) {
        // First try to delegate to the active tool
        if let Some(idx) = self.active_tool {
            if let Some(action) = self.tools[idx].handle_leader_action(c) {
                self.process_action(action);
                return;
            }
        }

        // Hub-level leader sequences
        match c {
            't' => {
                // Switch to Todo tool
                if let Some(idx) = self.tools.iter().position(|t| t.name() == "Todo") {
                    self.switch_to_tool(idx);
                }
            }
            'h' => {
                // Switch to HTTP tool
                if let Some(idx) = self.tools.iter().position(|t| t.name() == "HTTP") {
                    self.switch_to_tool(idx);
                }
            }
            'k' => {
                // Switch to KeePass tool
                if let Some(idx) = self.tools.iter().position(|t| t.name() == "KeePass") {
                    self.switch_to_tool(idx);
                }
            }
            'n' => {
                // Switch to Notes tool
                if let Some(idx) = self.tools.iter().position(|t| t.name() == "Notes") {
                    self.switch_to_tool(idx);
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
                self.reset_all_key_state();
            }
            KeyCode::Char(c) => {
                self.which_key.hide();
                self.reset_all_key_state();

                // Process the which-key selection (top-level leader menu)
                // First try to delegate to the active tool
                let mut handled = false;
                if let Some(idx) = self.active_tool {
                    if let Some(action) = self.tools[idx].handle_leader_action(c) {
                        self.process_action(action);
                        handled = true;
                    }
                }

                if !handled {
                    match c {
                        'q' => {
                            self.process_action(Action::Quit);
                        }
                        'f' => {
                            self.open_telescope();
                        }
                        'h' => {
                            // Switch to HTTP tool
                            if let Some(idx) = self.tools.iter().position(|t| t.name() == "HTTP") {
                                self.switch_to_tool(idx);
                            }
                        }
                        't' => {
                            // Switch to todo tool
                            if let Some(idx) = self.tools.iter().position(|t| t.name() == "Todo") {
                                self.switch_to_tool(idx);
                            }
                        }
                        'k' => {
                            // Switch to KeePass tool
                            if let Some(idx) = self.tools.iter().position(|t| t.name() == "KeePass")
                            {
                                self.switch_to_tool(idx);
                            }
                        }
                        'n' => {
                            // Switch to Notes tool
                            if let Some(idx) = self.tools.iter().position(|t| t.name() == "Notes") {
                                self.switch_to_tool(idx);
                            }
                        }
                        '?' => {
                            self.show_help();
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
            }
            _ => {
                self.which_key.hide();
                self.reset_all_key_state();
            }
        }
    }

    /// Handle telescope key events.
    fn handle_telescope_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.telescope.close();
                self.reset_all_key_state();
            }
            KeyCode::Enter => {
                if let Some(id) = self.telescope.selected_id() {
                    let id = id.to_string();
                    self.telescope.close();
                    self.reset_all_key_state();
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
        if let Some(tool_name) = id.strip_prefix("tool:") {
            if let Some(idx) = self.tools.iter().position(|t| t.name() == tool_name) {
                self.switch_to_tool(idx);
            }
            return;
        }

        if let Some(idx) = self
            .tools
            .iter_mut()
            .position(|tool| tool.handle_telescope_selection(id))
        {
            self.switch_to_tool(idx);
        }
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
        let cmd = cmd.trim();

        // First, let the active tool try to handle it
        if let Some(idx) = self.active_tool {
            if self.tools[idx].handle_command(cmd) {
                return;
            }
        }

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
            "wq" | "x" => {
                if let Some(idx) = self.active_tool {
                    self.tools[idx].handle_command("w");
                    self.tools[idx].on_blur();
                    self.active_tool = None;
                } else {
                    self.should_quit = true;
                }
            }
            "wqa" | "wqa!" | "xa" | "xa!" => {
                if let Some(idx) = self.active_tool {
                    self.tools[idx].handle_command("w");
                }
                self.should_quit = true;
            }
            _ => {
                // Unknown command — could show an error message in the future
            }
        }
    }

    /// Handle keys when on the dashboard (no tool active).
    fn handle_dashboard_key(&mut self, key: KeyEvent) {
        use rstools_core::keybinds::process_normal_key;

        let action = process_normal_key(key, &mut self.key_state);
        self.process_action(action);
    }

    /// Handle a mouse event.
    /// Handle a bracketed paste event from the terminal.
    fn handle_paste_event(&mut self, text: &str) {
        // If telescope is active, insert into the telescope input
        if self.telescope.visible {
            for c in text.chars() {
                if c != '\n' && c != '\r' {
                    self.telescope.insert_char(c);
                }
            }
            return;
        }

        // If command mode, insert into the command input
        if self.mode == InputMode::Command {
            for c in text.chars() {
                if c != '\n' && c != '\r' {
                    self.command_input.insert(self.command_cursor, c);
                    self.command_cursor += c.len_utf8();
                }
            }
            return;
        }

        // Delegate to active tool
        if let Some(idx) = self.active_tool {
            let action = self.tools[idx].handle_paste(text);
            self.process_action(action);
        }
    }

    fn handle_mouse_event(&mut self, mouse: MouseEvent) {
        let col = mouse.column;
        let row = mouse.row;

        // Overlays intercept mouse events when visible
        if self.telescope.visible {
            self.handle_telescope_mouse(mouse);
            return;
        }
        if self.which_key.visible {
            self.handle_which_key_mouse(mouse);
            return;
        }
        if self.help_popup.visible {
            self.handle_help_mouse(mouse);
            return;
        }

        // Tab bar: click to switch tools
        if self.last_tab_area.height > 0
            && row >= self.last_tab_area.y
            && row < self.last_tab_area.y + self.last_tab_area.height
            && col >= self.last_tab_area.x
            && col < self.last_tab_area.x + self.last_tab_area.width
        {
            if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                if let Some(idx) = self.tab_index_at(col) {
                    self.switch_to_tool(idx);
                }
                return;
            }
        }

        // Content area: delegate to active tool
        if self.last_content_area.height > 0
            && row >= self.last_content_area.y
            && row < self.last_content_area.y + self.last_content_area.height
            && col >= self.last_content_area.x
            && col < self.last_content_area.x + self.last_content_area.width
        {
            if let Some(idx) = self.active_tool {
                let action = self.tools[idx].handle_mouse(mouse, self.last_content_area);
                self.process_action(action);
            }
        }
    }

    /// Determine which tab index was clicked based on column position.
    fn tab_index_at(&self, col: u16) -> Option<usize> {
        if self.tools.is_empty() {
            return None;
        }
        // Tabs are rendered as: "Name1 | Name2 | Name3"
        // Each tab: name + divider " | " (3 chars), except the last.
        let mut x = self.last_tab_area.x;
        for (i, tool) in self.tools.iter().enumerate() {
            let name_len = tool.name().len() as u16;
            if col >= x && col < x + name_len {
                return Some(i);
            }
            x += name_len;
            // Divider " | " = 3 chars
            if i < self.tools.len() - 1 {
                x += 3;
            }
        }
        None
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

    /// Show the help popup with global + tool-specific keybinds.
    fn show_help(&mut self) {
        self.reset_all_key_state();
        let mut entries = Vec::new();

        // Add tool-specific entries first (if a tool is active)
        if let Some(idx) = self.active_tool {
            let tool_entries = self.tools[idx].help_entries();
            if !tool_entries.is_empty() {
                entries.extend(tool_entries);
            }
        }

        // Add global entries
        entries.extend(help_popup::global_help_entries());

        let title = match self.active_tool {
            Some(idx) => format!("{} Help", self.tools[idx].name()),
            None => "Help".to_string(),
        };

        self.help_popup.show(title, entries);
    }

    /// Handle mouse events while telescope is visible.
    fn handle_telescope_mouse(&mut self, mouse: MouseEvent) {
        // Overlays render over the full terminal area.
        // Reconstruct it from the stored layout: tab_area starts at y=0,
        // status bar is 1 line below content_area.
        let full_area = Rect {
            x: 0,
            y: 0,
            width: self.last_content_area.width.max(self.last_tab_area.width),
            height: self.last_content_area.y + self.last_content_area.height + 1,
        };
        let popup_area = self.telescope_popup_rect(full_area);

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if !rect_contains(popup_area, mouse.column, mouse.row) {
                    self.telescope.close();
                    self.reset_all_key_state();
                } else {
                    // Click inside: check if it's on a result item
                    let results_y_start = popup_area.y + 3; // input area is 3 lines
                    let results_y_end = popup_area.y + popup_area.height - 1; // -1 for bottom border
                    if mouse.row >= results_y_start
                        && mouse.row < results_y_end
                        && mouse.column > popup_area.x
                        && mouse.column < popup_area.x + popup_area.width - 1
                    {
                        let clicked_idx = (mouse.row - results_y_start) as usize;
                        if clicked_idx < self.telescope.filtered.len() {
                            self.telescope.list_state.select(Some(clicked_idx));
                        }
                    }
                }
            }
            MouseEventKind::ScrollDown => {
                self.telescope.move_down();
            }
            MouseEventKind::ScrollUp => {
                self.telescope.move_up();
            }
            _ => {}
        }
    }

    /// Compute the telescope popup rect (same logic as Telescope::render).
    fn telescope_popup_rect(&self, area: Rect) -> Rect {
        use ratatui::layout::{Constraint, Flex, Layout};
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
        popup_area
    }

    /// Handle mouse events while which-key is visible.
    fn handle_which_key_mouse(&mut self, mouse: MouseEvent) {
        // Click anywhere outside closes which-key, click inside is ignored
        if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
            self.which_key.hide();
            self.reset_all_key_state();
        }
    }

    /// Handle mouse events while help popup is visible.
    fn handle_help_mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                // Click anywhere closes the help popup
                self.help_popup.hide();
                self.reset_all_key_state();
            }
            MouseEventKind::ScrollDown => {
                self.help_popup.scroll_down();
            }
            MouseEventKind::ScrollUp => {
                self.help_popup.scroll_up();
            }
            _ => {}
        }
    }

    /// Handle key events while the help popup is visible.
    fn handle_help_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => {
                self.help_popup.hide();
                self.reset_all_key_state();
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.help_popup.scroll_down();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.help_popup.scroll_up();
            }
            KeyCode::Char('d') if key.modifiers == KeyModifiers::CONTROL => {
                for _ in 0..10 {
                    self.help_popup.scroll_down();
                }
            }
            KeyCode::Char('u') if key.modifiers == KeyModifiers::CONTROL => {
                for _ in 0..10 {
                    self.help_popup.scroll_up();
                }
            }
            _ => {}
        }
    }

    /// Render the entire application.
    pub fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let (tab_area, content_area, status_area) = ui::standard_layout(area);

        // Store layout areas for mouse hit-testing
        self.last_tab_area = tab_area;
        self.last_content_area = content_area;

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
        self.help_popup.render(frame, area);
        self.telescope.render(frame, area);
    }

    /// Returns the cursor style appropriate for the current mode.
    /// Insert mode uses a line/bar cursor; Normal/Command use block.
    pub fn cursor_style(&self) -> SetCursorStyle {
        let mode = match self.active_tool {
            Some(idx) => self.tools[idx].mode(),
            None => self.mode,
        };

        match mode {
            InputMode::Insert => SetCursorStyle::SteadyBar,
            _ => SetCursorStyle::SteadyBlock,
        }
    }

    /// Render the dashboard when no tool is active.
    fn render_dashboard(&self, frame: &mut Frame, area: Rect) {
        use ratatui::{
            layout::{Alignment, Constraint, Layout},
            style::{Modifier, Style},
            text::{Line, Span},
            widgets::Paragraph,
        };

        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "rstools",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "A vim-centric terminal toolset",
                Style::default().add_modifier(Modifier::DIM),
            )),
            Line::from(""),
            Line::from(""),
            Line::from(vec![
                Span::styled("  <Space> ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw("  Open leader menu"),
            ]),
            Line::from(vec![
                Span::styled(
                    "  <Space><Space> ",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw("  Tool picker"),
            ]),
            Line::from(vec![
                Span::styled("  <Space>f ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw("  Find (telescope)"),
            ]),
            Line::from(vec![
                Span::styled("  :q ", Style::default().add_modifier(Modifier::BOLD)),
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
                Style::default().add_modifier(Modifier::BOLD),
            )));
            all_lines.push(Line::from(""));
            for (i, tool) in self.tools.iter().enumerate() {
                all_lines.push(Line::from(vec![
                    Span::styled(
                        format!("  {} ", i + 1),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        tool.name(),
                        Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                    ),
                    Span::styled(
                        format!("  {}", tool.description()),
                        Style::default().add_modifier(Modifier::DIM),
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

/// Check if a point (col, row) is inside a Rect.
fn rect_contains(rect: Rect, col: u16, row: u16) -> bool {
    col >= rect.x && col < rect.x + rect.width && row >= rect.y && row < rect.y + rect.height
}
