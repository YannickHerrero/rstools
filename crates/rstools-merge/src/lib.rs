pub mod ui;

use crossterm::event::KeyEvent;
use ratatui::{layout::Rect, Frame};
use rstools_core::{
    help_popup::HelpEntry,
    keybinds::{Action, InputMode},
    telescope::TelescopeItem,
    tool::Tool,
    which_key::WhichKeyEntry,
};
use rusqlite::Connection;

pub struct MergeTool {
    mode: InputMode,
}

impl MergeTool {
    pub fn new(_conn: Connection) -> anyhow::Result<Self> {
        Ok(Self {
            mode: InputMode::Normal,
        })
    }
}

impl Tool for MergeTool {
    fn name(&self) -> &str {
        "Merge"
    }

    fn description(&self) -> &str {
        "Resolve git merge conflicts"
    }

    fn mode(&self) -> InputMode {
        self.mode
    }

    fn init_db(&self, _conn: &Connection) -> anyhow::Result<()> {
        Ok(())
    }

    fn which_key_entries(&self) -> Vec<WhichKeyEntry> {
        vec![WhichKeyEntry::action("e", "Focus editor")]
    }

    fn telescope_items(&self) -> Vec<TelescopeItem> {
        Vec::new()
    }

    fn help_entries(&self) -> Vec<HelpEntry> {
        vec![HelpEntry::with_section(
            "Merge",
            "<Space>m",
            "Open merge conflict tool",
        )]
    }

    fn handle_key(&mut self, _key: KeyEvent) -> Action {
        Action::None
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        ui::render_merge_tool(frame, area);
    }
}
