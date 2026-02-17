pub mod model;
pub mod sidebar;
pub mod ui;

use rstools_core::help_popup::HelpEntry;
use rstools_core::keybinds::{Action, InputMode, KeyState, process_normal_key};
use rstools_core::telescope::TelescopeItem;
use rstools_core::tool::Tool;
use rstools_core::which_key::WhichKeyEntry;

use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};
use rusqlite::Connection;

use sidebar::SidebarState;

pub struct HttpTool {
    sidebar: SidebarState,
    mode: InputMode,
    key_state: KeyState,
    conn: Connection,
}

impl HttpTool {
    pub fn new(conn: Connection) -> anyhow::Result<Self> {
        model::init_db(&conn)?;
        let mut sidebar = SidebarState::new();
        sidebar.reload(&conn)?;
        Ok(Self {
            sidebar,
            mode: InputMode::Normal,
            key_state: KeyState::default(),
            conn,
        })
    }
}

impl Tool for HttpTool {
    fn name(&self) -> &str {
        "HTTP"
    }

    fn description(&self) -> &str {
        "HTTP client & API explorer"
    }

    fn mode(&self) -> InputMode {
        self.mode
    }

    fn init_db(&self, conn: &Connection) -> anyhow::Result<()> {
        model::init_db(conn)
    }

    fn which_key_entries(&self) -> Vec<WhichKeyEntry> {
        vec![
            WhichKeyEntry::action("e", "Toggle explorer"),
            WhichKeyEntry::action("a", "Add entry"),
            WhichKeyEntry::action("d", "Delete entry"),
        ]
    }

    fn telescope_items(&self) -> Vec<TelescopeItem> {
        Vec::new() // TODO: return queries as searchable items
    }

    fn help_entries(&self) -> Vec<HelpEntry> {
        Vec::new() // TODO: add help entries
    }

    fn handle_key(&mut self, key: KeyEvent) -> Action {
        let _ = key;
        Action::None // TODO: implement key handling
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        ui::render_http_tool(frame, area, &self.sidebar);
    }

    fn reset_key_state(&mut self) {
        self.key_state.reset();
    }

    fn on_focus(&mut self) {
        let _ = self.sidebar.reload(&self.conn);
    }
}
