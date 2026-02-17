use crate::help_popup::HelpEntry;
use crate::keybinds::{Action, InputMode};
use crate::telescope::TelescopeItem;
use crate::which_key::WhichKeyEntry;
use crossterm::event::KeyEvent;
use ratatui::{layout::Rect, Frame};
use rusqlite::Connection;

/// The trait every rstools tool must implement.
/// Tools are embedded views inside the hub, like neovim buffers.
pub trait Tool {
    /// The display name of the tool (e.g., "Todo", "KeePass").
    fn name(&self) -> &str;

    /// Short description for the tool picker.
    fn description(&self) -> &str;

    /// The tool's current input mode (for status bar display).
    fn mode(&self) -> InputMode;

    /// Initialize the tool's database tables if they don't exist.
    fn init_db(&self, conn: &Connection) -> anyhow::Result<()>;

    /// Which-key entries for this tool's leader group.
    /// These appear when the user presses `<Space><tool_key>`.
    fn which_key_entries(&self) -> Vec<WhichKeyEntry>;

    /// Items this tool contributes to telescope search.
    fn telescope_items(&self) -> Vec<TelescopeItem>;

    /// Help entries specific to this tool (shown alongside global keybinds).
    /// Override this to add tool-specific keybind documentation.
    fn help_entries(&self) -> Vec<HelpEntry> {
        Vec::new()
    }

    /// Handle a key event. Returns an Action describing what happened.
    fn handle_key(&mut self, key: KeyEvent) -> Action;

    /// Render the tool's UI into the given area.
    fn render(&self, frame: &mut Frame, area: Rect);

    /// Reset any pending key state (leader, multi-key sequences).
    /// Called by the hub when it takes over input (overlays open/close).
    fn reset_key_state(&mut self) {}

    /// Called when the tool becomes the active view.
    fn on_focus(&mut self) {}

    /// Called when the tool loses focus.
    fn on_blur(&mut self) {}
}
