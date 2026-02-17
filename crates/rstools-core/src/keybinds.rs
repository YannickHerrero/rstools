use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Input modes, modeled after vim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    /// Default mode. Navigation and actions via keybinds.
    Normal,
    /// Text input mode. Entered with `i`, `a`, `o`, etc. Exited with `Esc`.
    Insert,
    /// Command-line mode. Entered with `:`. Supports `:q`, `:w`, etc.
    Command,
}

impl Default for InputMode {
    fn default() -> Self {
        Self::Normal
    }
}

impl InputMode {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Insert => "INSERT",
            Self::Command => "COMMAND",
        }
    }
}

/// Actions that can result from processing a key event.
/// Tools and the hub return these to signal what should happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// No-op — the key was consumed but nothing happens.
    None,
    /// Quit the current tool or the hub entirely.
    Quit,
    /// Switch to a specific input mode.
    SetMode(InputMode),
    /// Move selection down by N items.
    MoveDown(usize),
    /// Move selection up by N items.
    MoveUp(usize),
    /// Jump to top of list.
    GotoTop,
    /// Jump to bottom of list.
    GotoBottom,
    /// Half-page down.
    HalfPageDown,
    /// Half-page up.
    HalfPageUp,
    /// Confirm/select/toggle the current item.
    Confirm,
    /// Delete the current item.
    Delete,
    /// Begin adding a new item.
    Add,
    /// Begin adding a new item below current.
    AddBelow,
    /// Begin editing the current item.
    Edit,
    /// Enter search/filter mode.
    Search,
    /// Open which-key leader menu.
    LeaderKey,
    /// A leader key sequence was completed with this key.
    LeaderSequence(char),
    /// Switch to tool by index (0-based).
    SwitchTool(usize),
    /// Next tool tab.
    NextTool,
    /// Previous tool tab.
    PrevTool,
    /// Tool picker (telescope over tools).
    ToolPicker,
    /// Show help.
    Help,
    /// Open telescope fuzzy finder.
    Telescope,
    /// Submit text in Insert/Command mode (Enter was pressed).
    Submit(String),
    /// Text input changed in Insert mode.
    TextInput(String),
}

/// Pending key state for multi-key sequences like `gg`, `dd`, `gt`, `gT`.
#[derive(Debug, Default, Clone)]
pub struct KeyState {
    /// Whether the leader key (Space) was just pressed.
    pub leader_active: bool,
    /// Pending first key of a two-key sequence (e.g., 'g' for gg/gt/gT, 'd' for dd).
    pub pending_key: Option<char>,
}

impl KeyState {
    pub fn reset(&mut self) {
        self.leader_active = false;
        self.pending_key = None;
    }
}

/// Process a key event in Normal mode, accounting for multi-key sequences.
/// Returns an Action and whether the key_state was consumed/reset.
pub fn process_normal_key(key: KeyEvent, state: &mut KeyState) -> Action {
    // If leader is active, process leader sequences
    if state.leader_active {
        state.leader_active = false;
        return match key.code {
            KeyCode::Char(' ') => Action::ToolPicker,
            KeyCode::Char('f') => {
                // Start of <Space>f sequence — next key matters
                // For now, treat <Space>f as telescope trigger
                // (sub-sequences like ff, fg will be handled later)
                Action::Telescope
            }
            KeyCode::Char(c @ '1'..='9') => {
                let idx = (c as u8 - b'1') as usize;
                Action::SwitchTool(idx)
            }
            KeyCode::Char('q') => Action::Quit,
            KeyCode::Char(c) => Action::LeaderSequence(c),
            KeyCode::Esc => Action::None,
            _ => Action::None,
        };
    }

    // If there's a pending key, handle two-key sequences
    if let Some(pending) = state.pending_key.take() {
        return match (pending, key.code) {
            ('g', KeyCode::Char('g')) => Action::GotoTop,
            ('g', KeyCode::Char('t')) => Action::NextTool,
            ('g', KeyCode::Char('T')) => Action::PrevTool,
            ('d', KeyCode::Char('d')) => Action::Delete,
            _ => Action::None, // Invalid sequence, ignore
        };
    }

    // Single key processing
    match key.code {
        KeyCode::Char(' ') => {
            state.leader_active = true;
            Action::LeaderKey
        }
        KeyCode::Char('j') => Action::MoveDown(1),
        KeyCode::Char('k') => Action::MoveUp(1),
        KeyCode::Char('G') => Action::GotoBottom,
        KeyCode::Char('g') => {
            state.pending_key = Some('g');
            Action::None
        }
        KeyCode::Char('d') if key.modifiers == KeyModifiers::CONTROL => Action::HalfPageDown,
        KeyCode::Char('u') if key.modifiers == KeyModifiers::CONTROL => Action::HalfPageUp,
        KeyCode::Char('d') => {
            state.pending_key = Some('d');
            Action::None
        }
        KeyCode::Enter => Action::Confirm,
        KeyCode::Char('/') => Action::Search,
        KeyCode::Char('a') => Action::Add,
        KeyCode::Char('o') => Action::AddBelow,
        KeyCode::Char('e') => Action::Edit,
        KeyCode::Char('i') => Action::SetMode(InputMode::Insert),
        KeyCode::Char(':') => Action::SetMode(InputMode::Command),
        KeyCode::Char('?') => Action::Help,
        KeyCode::Char('q') => Action::Quit,
        _ => Action::None,
    }
}
