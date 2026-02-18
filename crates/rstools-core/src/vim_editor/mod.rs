pub mod buffer;
pub mod history;

use buffer::{
    char_class, find_char_backward, find_char_forward, find_till_backward, find_till_forward,
    find_word_backward, find_word_end, find_word_forward, CharClass, TextBuffer,
};
use history::History;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

// ── Vim modes ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VimMode {
    Normal,
    Insert,
    Visual,
    VisualLine,
}

// ── Register (clipboard) ─────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Register {
    pub content: String,
    pub linewise: bool,
}

impl Register {
    pub fn new() -> Self {
        Self {
            content: String::new(),
            linewise: false,
        }
    }
}

// ── Editor action result ─────────────────────────────────────────────

/// Actions that the editor can request from its parent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorAction {
    /// Key was consumed, no external action needed.
    None,
    /// Mode changed (for parent to update status bar).
    ModeChanged(VimMode),
    /// Editor wants to enter command mode (`:` was pressed).
    EnterCommandMode,
}

// ── Key parse state ──────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Operator {
    Delete,
    Change,
    Yank,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Motion {
    Left,
    Right,
    Up,
    Down,
    WordForward,
    WordBackward,
    WordEnd,
    LineStart,
    LineEnd,
    FileTop,
    FileBottom,
    HalfPageDown,
    HalfPageUp,
    FindChar(char),
    FindCharBack(char),
    TillChar(char),
    TillCharBack(char),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextObject {
    InnerWord,
    AroundWord,
    InnerQuote(char),
    AroundQuote(char),
    InnerBracket(char),
    AroundBracket(char),
    InnerParagraph,
    AroundParagraph,
}

/// Parse state for multi-key vim commands.
#[derive(Debug, Clone)]
enum ParseState {
    /// Waiting for initial input.
    Idle,
    /// Accumulating count prefix digits.
    Count(usize),
    /// An operator has been entered, waiting for motion/text-object.
    OperatorPending { op: Operator, count: usize },
    /// Operator + count, waiting for motion.
    OperatorCount {
        op: Operator,
        count1: usize,
        count2: usize,
    },
    /// Waiting for second key of two-key sequence.
    PendingG { count: usize },
    /// Waiting for char after f/F/t/T.
    PendingFind {
        count: usize,
        op: Option<Operator>,
        forward: bool,
        till: bool,
    },
    /// Waiting for char after r.
    PendingReplace { count: usize },
    /// Waiting for text object target after 'i' or 'a'.
    PendingTextObject {
        op: Option<Operator>,
        count: usize,
        inner: bool,
    },
}

// ── VimEditor ────────────────────────────────────────────────────────

pub struct VimEditor {
    pub buffer: TextBuffer,
    pub mode: VimMode,
    history: History,
    register: Register,
    parse_state: ParseState,
    /// Anchor position for visual mode.
    visual_anchor_row: usize,
    visual_anchor_col: usize,
    /// Visible height (updated each render for half-page calculations).
    visible_height: usize,
}

impl VimEditor {
    pub fn new() -> Self {
        Self {
            buffer: TextBuffer::new(),
            mode: VimMode::Normal,
            history: History::new(200),
            register: Register::new(),
            parse_state: ParseState::Idle,
            visual_anchor_row: 0,
            visual_anchor_col: 0,
            visible_height: 20,
        }
    }

    pub fn from_text(text: &str) -> Self {
        let mut editor = Self::new();
        editor.buffer = TextBuffer::from_text(text);
        editor
    }

    pub fn text(&self) -> String {
        self.buffer.text()
    }

    pub fn set_text(&mut self, text: &str) {
        self.buffer.set_text(text);
        self.history.clear();
        self.mode = VimMode::Normal;
        self.parse_state = ParseState::Idle;
    }

    pub fn is_dirty(&self) -> bool {
        self.buffer.dirty
    }

    pub fn mark_clean(&mut self) {
        self.buffer.dirty = false;
    }

    /// Save snapshot before a modification for undo.
    fn save_undo(&mut self) {
        let snapshot = self.buffer.snapshot();
        self.history.push(snapshot);
    }

    /// Reset parse state.
    fn reset_parse(&mut self) {
        self.parse_state = ParseState::Idle;
    }

    // ── Key handling ─────────────────────────────────────────────────

    pub fn handle_key(&mut self, key: KeyEvent) -> EditorAction {
        match self.mode {
            VimMode::Normal => self.handle_normal_key(key),
            VimMode::Insert => self.handle_insert_key(key),
            VimMode::Visual | VimMode::VisualLine => self.handle_visual_key(key),
        }
    }

    fn handle_normal_key(&mut self, key: KeyEvent) -> EditorAction {
        // Process based on current parse state
        match self.parse_state.clone() {
            ParseState::Idle => self.handle_normal_idle(key),
            ParseState::Count(n) => self.handle_normal_count(key, n),
            ParseState::OperatorPending { op, count } => {
                self.handle_operator_pending(key, op, count)
            }
            ParseState::OperatorCount { op, count1, count2 } => {
                self.handle_operator_count(key, op, count1, count2)
            }
            ParseState::PendingG { count } => self.handle_pending_g(key, count),
            ParseState::PendingFind {
                count,
                op,
                forward,
                till,
            } => self.handle_pending_find(key, count, op, forward, till),
            ParseState::PendingReplace { count } => self.handle_pending_replace(key, count),
            ParseState::PendingTextObject { op, count, inner } => {
                self.handle_pending_text_object(key, op, count, inner)
            }
        }
    }

    fn handle_normal_idle(&mut self, key: KeyEvent) -> EditorAction {
        match key.code {
            // Count prefix
            KeyCode::Char(c @ '1'..='9') => {
                self.parse_state = ParseState::Count((c as u8 - b'0') as usize);
                EditorAction::None
            }
            // Motions
            KeyCode::Char('h') | KeyCode::Left => {
                self.execute_motion(Motion::Left, 1);
                EditorAction::None
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.execute_motion(Motion::Right, 1);
                EditorAction::None
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.execute_motion(Motion::Down, 1);
                EditorAction::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.execute_motion(Motion::Up, 1);
                EditorAction::None
            }
            KeyCode::Char('w') => {
                self.execute_motion(Motion::WordForward, 1);
                EditorAction::None
            }
            KeyCode::Char('b') => {
                self.execute_motion(Motion::WordBackward, 1);
                EditorAction::None
            }
            KeyCode::Char('e') => {
                self.execute_motion(Motion::WordEnd, 1);
                EditorAction::None
            }
            KeyCode::Char('0') => {
                self.execute_motion(Motion::LineStart, 1);
                EditorAction::None
            }
            KeyCode::Char('$') => {
                self.execute_motion(Motion::LineEnd, 1);
                EditorAction::None
            }
            KeyCode::Char('G') => {
                self.execute_motion(Motion::FileBottom, 1);
                EditorAction::None
            }
            KeyCode::Char('g') => {
                self.parse_state = ParseState::PendingG { count: 1 };
                EditorAction::None
            }
            KeyCode::Char('d') if key.modifiers == KeyModifiers::CONTROL => {
                self.execute_motion(Motion::HalfPageDown, 1);
                EditorAction::None
            }
            KeyCode::Char('u') if key.modifiers == KeyModifiers::CONTROL => {
                self.execute_motion(Motion::HalfPageUp, 1);
                EditorAction::None
            }
            KeyCode::Char('f') => {
                self.parse_state = ParseState::PendingFind {
                    count: 1,
                    op: None,
                    forward: true,
                    till: false,
                };
                EditorAction::None
            }
            KeyCode::Char('F') => {
                self.parse_state = ParseState::PendingFind {
                    count: 1,
                    op: None,
                    forward: false,
                    till: false,
                };
                EditorAction::None
            }
            KeyCode::Char('t') => {
                self.parse_state = ParseState::PendingFind {
                    count: 1,
                    op: None,
                    forward: true,
                    till: true,
                };
                EditorAction::None
            }
            KeyCode::Char('T') => {
                self.parse_state = ParseState::PendingFind {
                    count: 1,
                    op: None,
                    forward: false,
                    till: true,
                };
                EditorAction::None
            }

            // Operators
            KeyCode::Char('d') => {
                self.parse_state = ParseState::OperatorPending {
                    op: Operator::Delete,
                    count: 1,
                };
                EditorAction::None
            }
            KeyCode::Char('c') => {
                self.parse_state = ParseState::OperatorPending {
                    op: Operator::Change,
                    count: 1,
                };
                EditorAction::None
            }
            KeyCode::Char('y') => {
                self.parse_state = ParseState::OperatorPending {
                    op: Operator::Yank,
                    count: 1,
                };
                EditorAction::None
            }

            // Standalone commands
            KeyCode::Char('x') => {
                self.save_undo();
                self.buffer.delete_char_at_cursor();
                self.buffer.clamp_cursor_col(false);
                EditorAction::None
            }
            KeyCode::Char('r') if key.modifiers == KeyModifiers::NONE => {
                self.parse_state = ParseState::PendingReplace { count: 1 };
                EditorAction::None
            }
            KeyCode::Char('D') => {
                self.save_undo();
                let col = self.buffer.cursor_col;
                let len = self.buffer.current_line_len();
                if col < len {
                    let deleted = self.buffer.delete_range(
                        self.buffer.cursor_row,
                        col,
                        self.buffer.cursor_row,
                        len,
                    );
                    self.register = Register {
                        content: deleted,
                        linewise: false,
                    };
                }
                self.buffer.clamp_cursor_col(false);
                EditorAction::None
            }
            KeyCode::Char('C') => {
                self.save_undo();
                let col = self.buffer.cursor_col;
                let len = self.buffer.current_line_len();
                if col < len {
                    let deleted = self.buffer.delete_range(
                        self.buffer.cursor_row,
                        col,
                        self.buffer.cursor_row,
                        len,
                    );
                    self.register = Register {
                        content: deleted,
                        linewise: false,
                    };
                }
                self.mode = VimMode::Insert;
                return EditorAction::ModeChanged(VimMode::Insert);
            }
            KeyCode::Char('Y') => {
                // Yank current line
                let line = self.buffer.current_line().to_string();
                self.register = Register {
                    content: line,
                    linewise: true,
                };
                EditorAction::None
            }
            KeyCode::Char('J') => {
                self.save_undo();
                self.buffer.join_lines();
                EditorAction::None
            }
            KeyCode::Char('p') => {
                self.save_undo();
                self.paste_after();
                EditorAction::None
            }
            KeyCode::Char('P') => {
                self.save_undo();
                self.paste_before();
                EditorAction::None
            }
            KeyCode::Char('u') if key.modifiers == KeyModifiers::NONE => {
                let current = self.buffer.snapshot();
                if let Some(snapshot) = self.history.undo(current) {
                    self.buffer.restore(&snapshot);
                }
                EditorAction::None
            }
            KeyCode::Char('r') if key.modifiers == KeyModifiers::CONTROL => {
                let current = self.buffer.snapshot();
                if let Some(snapshot) = self.history.redo(current) {
                    self.buffer.restore(&snapshot);
                }
                EditorAction::None
            }

            // Mode entry
            KeyCode::Char('i') => {
                self.mode = VimMode::Insert;
                EditorAction::ModeChanged(VimMode::Insert)
            }
            KeyCode::Char('a') => {
                self.buffer.cursor_right();
                self.mode = VimMode::Insert;
                EditorAction::ModeChanged(VimMode::Insert)
            }
            KeyCode::Char('A') => {
                self.buffer.cursor_end();
                self.mode = VimMode::Insert;
                EditorAction::ModeChanged(VimMode::Insert)
            }
            KeyCode::Char('I') => {
                self.buffer.cursor_home();
                self.mode = VimMode::Insert;
                EditorAction::ModeChanged(VimMode::Insert)
            }
            KeyCode::Char('o') => {
                self.save_undo();
                self.buffer.open_line_below();
                self.mode = VimMode::Insert;
                EditorAction::ModeChanged(VimMode::Insert)
            }
            KeyCode::Char('O') => {
                self.save_undo();
                self.buffer.open_line_above();
                self.mode = VimMode::Insert;
                EditorAction::ModeChanged(VimMode::Insert)
            }
            KeyCode::Char('v') => {
                self.visual_anchor_row = self.buffer.cursor_row;
                self.visual_anchor_col = self.buffer.cursor_col;
                self.mode = VimMode::Visual;
                EditorAction::ModeChanged(VimMode::Visual)
            }
            KeyCode::Char('V') => {
                self.visual_anchor_row = self.buffer.cursor_row;
                self.visual_anchor_col = self.buffer.cursor_col;
                self.mode = VimMode::VisualLine;
                EditorAction::ModeChanged(VimMode::VisualLine)
            }
            KeyCode::Char(':') => EditorAction::EnterCommandMode,
            KeyCode::Esc => {
                self.reset_parse();
                EditorAction::None
            }
            _ => EditorAction::None,
        }
    }

    fn handle_normal_count(&mut self, key: KeyEvent, n: usize) -> EditorAction {
        match key.code {
            KeyCode::Char(c @ '0'..='9') => {
                self.parse_state = ParseState::Count(n * 10 + (c as u8 - b'0') as usize);
                EditorAction::None
            }
            // Motion with count
            KeyCode::Char('h') | KeyCode::Left => {
                self.execute_motion(Motion::Left, n);
                self.reset_parse();
                EditorAction::None
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.execute_motion(Motion::Right, n);
                self.reset_parse();
                EditorAction::None
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.execute_motion(Motion::Down, n);
                self.reset_parse();
                EditorAction::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.execute_motion(Motion::Up, n);
                self.reset_parse();
                EditorAction::None
            }
            KeyCode::Char('w') => {
                self.execute_motion(Motion::WordForward, n);
                self.reset_parse();
                EditorAction::None
            }
            KeyCode::Char('b') => {
                self.execute_motion(Motion::WordBackward, n);
                self.reset_parse();
                EditorAction::None
            }
            KeyCode::Char('e') => {
                self.execute_motion(Motion::WordEnd, n);
                self.reset_parse();
                EditorAction::None
            }
            KeyCode::Char('G') => {
                // nG = goto line n
                let target = (n - 1).min(self.buffer.lines.len() - 1);
                self.buffer.cursor_row = target;
                self.buffer.cursor_col = 0;
                self.buffer.desired_col = 0;
                self.reset_parse();
                EditorAction::None
            }
            KeyCode::Char('g') => {
                self.parse_state = ParseState::PendingG { count: n };
                EditorAction::None
            }
            // Operator with count
            KeyCode::Char('d') => {
                self.parse_state = ParseState::OperatorPending {
                    op: Operator::Delete,
                    count: n,
                };
                EditorAction::None
            }
            KeyCode::Char('c') => {
                self.parse_state = ParseState::OperatorPending {
                    op: Operator::Change,
                    count: n,
                };
                EditorAction::None
            }
            KeyCode::Char('y') => {
                self.parse_state = ParseState::OperatorPending {
                    op: Operator::Yank,
                    count: n,
                };
                EditorAction::None
            }
            KeyCode::Char('x') => {
                self.save_undo();
                for _ in 0..n {
                    self.buffer.delete_char_at_cursor();
                }
                self.buffer.clamp_cursor_col(false);
                self.reset_parse();
                EditorAction::None
            }
            KeyCode::Char('f') => {
                self.parse_state = ParseState::PendingFind {
                    count: n,
                    op: None,
                    forward: true,
                    till: false,
                };
                EditorAction::None
            }
            KeyCode::Char('F') => {
                self.parse_state = ParseState::PendingFind {
                    count: n,
                    op: None,
                    forward: false,
                    till: false,
                };
                EditorAction::None
            }
            KeyCode::Char('t') => {
                self.parse_state = ParseState::PendingFind {
                    count: n,
                    op: None,
                    forward: true,
                    till: true,
                };
                EditorAction::None
            }
            KeyCode::Char('T') => {
                self.parse_state = ParseState::PendingFind {
                    count: n,
                    op: None,
                    forward: false,
                    till: true,
                };
                EditorAction::None
            }
            KeyCode::Char('r') => {
                self.parse_state = ParseState::PendingReplace { count: n };
                EditorAction::None
            }
            KeyCode::Esc => {
                self.reset_parse();
                EditorAction::None
            }
            _ => {
                self.reset_parse();
                EditorAction::None
            }
        }
    }

    fn handle_operator_pending(
        &mut self,
        key: KeyEvent,
        op: Operator,
        count: usize,
    ) -> EditorAction {
        match key.code {
            // Double operator = line operation (dd, yy, cc)
            KeyCode::Char('d') if op == Operator::Delete => {
                self.execute_line_op(op, count);
                self.reset_parse();
                if op == Operator::Change {
                    return EditorAction::ModeChanged(VimMode::Insert);
                }
                EditorAction::None
            }
            KeyCode::Char('y') if op == Operator::Yank => {
                self.execute_line_op(op, count);
                self.reset_parse();
                EditorAction::None
            }
            KeyCode::Char('c') if op == Operator::Change => {
                self.execute_line_op(op, count);
                self.reset_parse();
                return EditorAction::ModeChanged(VimMode::Insert);
            }
            // Count after operator
            KeyCode::Char(c @ '1'..='9') => {
                self.parse_state = ParseState::OperatorCount {
                    op,
                    count1: count,
                    count2: (c as u8 - b'0') as usize,
                };
                EditorAction::None
            }
            // Text object
            KeyCode::Char('i') => {
                self.parse_state = ParseState::PendingTextObject {
                    op: Some(op),
                    count,
                    inner: true,
                };
                EditorAction::None
            }
            KeyCode::Char('a') => {
                self.parse_state = ParseState::PendingTextObject {
                    op: Some(op),
                    count,
                    inner: false,
                };
                EditorAction::None
            }
            // Motion after operator
            KeyCode::Char('h') | KeyCode::Left => {
                self.execute_operator_motion(op, Motion::Left, count);
                self.reset_parse();
                if op == Operator::Change {
                    return EditorAction::ModeChanged(VimMode::Insert);
                }
                EditorAction::None
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.execute_operator_motion(op, Motion::Right, count);
                self.reset_parse();
                if op == Operator::Change {
                    return EditorAction::ModeChanged(VimMode::Insert);
                }
                EditorAction::None
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.execute_operator_line_motion(op, count, true);
                self.reset_parse();
                if op == Operator::Change {
                    return EditorAction::ModeChanged(VimMode::Insert);
                }
                EditorAction::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.execute_operator_line_motion(op, count, false);
                self.reset_parse();
                if op == Operator::Change {
                    return EditorAction::ModeChanged(VimMode::Insert);
                }
                EditorAction::None
            }
            KeyCode::Char('w') => {
                self.execute_operator_motion(op, Motion::WordForward, count);
                self.reset_parse();
                if op == Operator::Change {
                    return EditorAction::ModeChanged(VimMode::Insert);
                }
                EditorAction::None
            }
            KeyCode::Char('b') => {
                self.execute_operator_motion(op, Motion::WordBackward, count);
                self.reset_parse();
                if op == Operator::Change {
                    return EditorAction::ModeChanged(VimMode::Insert);
                }
                EditorAction::None
            }
            KeyCode::Char('e') => {
                self.execute_operator_motion(op, Motion::WordEnd, count);
                self.reset_parse();
                if op == Operator::Change {
                    return EditorAction::ModeChanged(VimMode::Insert);
                }
                EditorAction::None
            }
            KeyCode::Char('$') => {
                self.execute_operator_motion(op, Motion::LineEnd, count);
                self.reset_parse();
                if op == Operator::Change {
                    return EditorAction::ModeChanged(VimMode::Insert);
                }
                EditorAction::None
            }
            KeyCode::Char('0') => {
                self.execute_operator_motion(op, Motion::LineStart, count);
                self.reset_parse();
                if op == Operator::Change {
                    return EditorAction::ModeChanged(VimMode::Insert);
                }
                EditorAction::None
            }
            KeyCode::Char('G') => {
                // d/y/c to end of file (linewise)
                let end_row = self.buffer.lines.len() - 1;
                let start_row = self.buffer.cursor_row;
                self.save_undo();
                let deleted = self.buffer.delete_line_range(start_row, end_row);
                self.register = Register {
                    content: deleted,
                    linewise: true,
                };
                if op == Operator::Change {
                    self.mode = VimMode::Insert;
                    self.reset_parse();
                    return EditorAction::ModeChanged(VimMode::Insert);
                }
                self.reset_parse();
                EditorAction::None
            }
            KeyCode::Char('g') => {
                self.parse_state = ParseState::PendingG { count };
                EditorAction::None
            }
            KeyCode::Char('f') => {
                self.parse_state = ParseState::PendingFind {
                    count,
                    op: Some(op),
                    forward: true,
                    till: false,
                };
                EditorAction::None
            }
            KeyCode::Char('F') => {
                self.parse_state = ParseState::PendingFind {
                    count,
                    op: Some(op),
                    forward: false,
                    till: false,
                };
                EditorAction::None
            }
            KeyCode::Char('t') => {
                self.parse_state = ParseState::PendingFind {
                    count,
                    op: Some(op),
                    forward: true,
                    till: true,
                };
                EditorAction::None
            }
            KeyCode::Char('T') => {
                self.parse_state = ParseState::PendingFind {
                    count,
                    op: Some(op),
                    forward: false,
                    till: true,
                };
                EditorAction::None
            }
            KeyCode::Esc => {
                self.reset_parse();
                EditorAction::None
            }
            _ => {
                self.reset_parse();
                EditorAction::None
            }
        }
    }

    fn handle_operator_count(
        &mut self,
        key: KeyEvent,
        op: Operator,
        count1: usize,
        count2: usize,
    ) -> EditorAction {
        let total = count1 * count2;
        match key.code {
            KeyCode::Char(c @ '0'..='9') => {
                self.parse_state = ParseState::OperatorCount {
                    op,
                    count1,
                    count2: count2 * 10 + (c as u8 - b'0') as usize,
                };
                EditorAction::None
            }
            _ => {
                // Re-dispatch with combined count
                self.parse_state = ParseState::OperatorPending { op, count: total };
                self.handle_operator_pending(key, op, total)
            }
        }
    }

    fn handle_pending_g(&mut self, key: KeyEvent, count: usize) -> EditorAction {
        match key.code {
            KeyCode::Char('g') => {
                if count > 1 {
                    // ngg = goto line n
                    let target = (count - 1).min(self.buffer.lines.len() - 1);
                    self.buffer.cursor_row = target;
                    self.buffer.cursor_col = 0;
                    self.buffer.desired_col = 0;
                } else {
                    self.execute_motion(Motion::FileTop, 1);
                }
                self.reset_parse();
                EditorAction::None
            }
            _ => {
                self.reset_parse();
                EditorAction::None
            }
        }
    }

    fn handle_pending_find(
        &mut self,
        key: KeyEvent,
        count: usize,
        op: Option<Operator>,
        forward: bool,
        till: bool,
    ) -> EditorAction {
        match key.code {
            KeyCode::Char(c) => {
                let motion = match (forward, till) {
                    (true, false) => Motion::FindChar(c),
                    (true, true) => Motion::TillChar(c),
                    (false, false) => Motion::FindCharBack(c),
                    (false, true) => Motion::TillCharBack(c),
                };
                if let Some(op) = op {
                    self.execute_operator_motion(op, motion, count);
                    self.reset_parse();
                    if op == Operator::Change {
                        return EditorAction::ModeChanged(VimMode::Insert);
                    }
                } else {
                    self.execute_motion(motion, count);
                    self.reset_parse();
                }
                EditorAction::None
            }
            KeyCode::Esc => {
                self.reset_parse();
                EditorAction::None
            }
            _ => {
                self.reset_parse();
                EditorAction::None
            }
        }
    }

    fn handle_pending_replace(&mut self, key: KeyEvent, _count: usize) -> EditorAction {
        match key.code {
            KeyCode::Char(c) => {
                self.save_undo();
                self.buffer.replace_char(c);
                self.reset_parse();
                EditorAction::None
            }
            KeyCode::Esc => {
                self.reset_parse();
                EditorAction::None
            }
            _ => {
                self.reset_parse();
                EditorAction::None
            }
        }
    }

    fn handle_pending_text_object(
        &mut self,
        key: KeyEvent,
        op: Option<Operator>,
        count: usize,
        inner: bool,
    ) -> EditorAction {
        let text_obj = match key.code {
            KeyCode::Char('w') => {
                if inner {
                    TextObject::InnerWord
                } else {
                    TextObject::AroundWord
                }
            }
            KeyCode::Char('"') => {
                if inner {
                    TextObject::InnerQuote('"')
                } else {
                    TextObject::AroundQuote('"')
                }
            }
            KeyCode::Char('\'') => {
                if inner {
                    TextObject::InnerQuote('\'')
                } else {
                    TextObject::AroundQuote('\'')
                }
            }
            KeyCode::Char('`') => {
                if inner {
                    TextObject::InnerQuote('`')
                } else {
                    TextObject::AroundQuote('`')
                }
            }
            KeyCode::Char('(') | KeyCode::Char(')') | KeyCode::Char('b') => {
                if inner {
                    TextObject::InnerBracket('(')
                } else {
                    TextObject::AroundBracket('(')
                }
            }
            KeyCode::Char('[') | KeyCode::Char(']') => {
                if inner {
                    TextObject::InnerBracket('[')
                } else {
                    TextObject::AroundBracket('[')
                }
            }
            KeyCode::Char('{') | KeyCode::Char('}') | KeyCode::Char('B') => {
                if inner {
                    TextObject::InnerBracket('{')
                } else {
                    TextObject::AroundBracket('{')
                }
            }
            KeyCode::Char('<') | KeyCode::Char('>') => {
                if inner {
                    TextObject::InnerBracket('<')
                } else {
                    TextObject::AroundBracket('<')
                }
            }
            KeyCode::Char('p') => {
                if inner {
                    TextObject::InnerParagraph
                } else {
                    TextObject::AroundParagraph
                }
            }
            KeyCode::Esc => {
                self.reset_parse();
                return EditorAction::None;
            }
            _ => {
                self.reset_parse();
                return EditorAction::None;
            }
        };

        if let Some(op) = op {
            self.execute_text_object_op(op, text_obj, count);
            self.reset_parse();
            if op == Operator::Change {
                return EditorAction::ModeChanged(VimMode::Insert);
            }
        } else {
            // In visual mode, text objects select the range
            if let Some((sr, sc, er, ec)) = self.compute_text_object(text_obj) {
                self.visual_anchor_row = sr;
                self.visual_anchor_col = sc;
                self.buffer.cursor_row = er;
                self.buffer.cursor_col = if ec > 0 { ec - 1 } else { 0 };
            }
            self.reset_parse();
        }
        EditorAction::None
    }

    // ── Insert mode ──────────────────────────────────────────────────

    fn handle_insert_key(&mut self, key: KeyEvent) -> EditorAction {
        match key.code {
            KeyCode::Esc => {
                // Move cursor back one (vim convention)
                if self.buffer.cursor_col > 0 {
                    self.buffer.cursor_left();
                }
                self.mode = VimMode::Normal;
                EditorAction::ModeChanged(VimMode::Normal)
            }
            KeyCode::Char(c) => {
                self.save_undo();
                self.buffer.insert_char(c);
                EditorAction::None
            }
            KeyCode::Enter => {
                self.save_undo();
                self.buffer.insert_newline();
                EditorAction::None
            }
            KeyCode::Backspace => {
                self.save_undo();
                self.buffer.backspace();
                EditorAction::None
            }
            KeyCode::Delete => {
                self.save_undo();
                self.buffer.delete_char_at_cursor();
                EditorAction::None
            }
            KeyCode::Left => {
                self.buffer.cursor_left();
                EditorAction::None
            }
            KeyCode::Right => {
                self.buffer.cursor_right();
                EditorAction::None
            }
            KeyCode::Up => {
                self.buffer.cursor_up();
                EditorAction::None
            }
            KeyCode::Down => {
                self.buffer.cursor_down();
                EditorAction::None
            }
            KeyCode::Home => {
                self.buffer.cursor_home();
                EditorAction::None
            }
            KeyCode::End => {
                self.buffer.cursor_end();
                EditorAction::None
            }
            _ => EditorAction::None,
        }
    }

    // ── Visual mode ──────────────────────────────────────────────────

    fn handle_visual_key(&mut self, key: KeyEvent) -> EditorAction {
        // Handle pending parse states first (text objects, g-prefix, etc.)
        match self.parse_state.clone() {
            ParseState::PendingTextObject { op, count, inner } => {
                return self.handle_pending_text_object(key, op, count, inner);
            }
            ParseState::PendingG { count } => {
                return self.handle_pending_g(key, count);
            }
            _ => {}
        }

        match key.code {
            KeyCode::Esc => {
                self.mode = VimMode::Normal;
                self.reset_parse();
                EditorAction::ModeChanged(VimMode::Normal)
            }
            // Toggle between visual modes
            KeyCode::Char('v') => {
                if self.mode == VimMode::Visual {
                    self.mode = VimMode::Normal;
                    EditorAction::ModeChanged(VimMode::Normal)
                } else {
                    self.mode = VimMode::Visual;
                    EditorAction::ModeChanged(VimMode::Visual)
                }
            }
            KeyCode::Char('V') => {
                if self.mode == VimMode::VisualLine {
                    self.mode = VimMode::Normal;
                    EditorAction::ModeChanged(VimMode::Normal)
                } else {
                    self.mode = VimMode::VisualLine;
                    EditorAction::ModeChanged(VimMode::VisualLine)
                }
            }
            // Motions extend selection
            KeyCode::Char('h') | KeyCode::Left => {
                self.execute_motion(Motion::Left, 1);
                EditorAction::None
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.execute_motion(Motion::Right, 1);
                EditorAction::None
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.execute_motion(Motion::Down, 1);
                EditorAction::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.execute_motion(Motion::Up, 1);
                EditorAction::None
            }
            KeyCode::Char('w') => {
                self.execute_motion(Motion::WordForward, 1);
                EditorAction::None
            }
            KeyCode::Char('b') => {
                self.execute_motion(Motion::WordBackward, 1);
                EditorAction::None
            }
            KeyCode::Char('e') => {
                self.execute_motion(Motion::WordEnd, 1);
                EditorAction::None
            }
            KeyCode::Char('0') => {
                self.execute_motion(Motion::LineStart, 1);
                EditorAction::None
            }
            KeyCode::Char('$') => {
                self.execute_motion(Motion::LineEnd, 1);
                EditorAction::None
            }
            KeyCode::Char('G') => {
                self.execute_motion(Motion::FileBottom, 1);
                EditorAction::None
            }
            KeyCode::Char('g') => {
                self.parse_state = ParseState::PendingG { count: 1 };
                EditorAction::None
            }
            // Text objects in visual mode
            KeyCode::Char('i') => {
                self.parse_state = ParseState::PendingTextObject {
                    op: None,
                    count: 1,
                    inner: true,
                };
                EditorAction::None
            }
            KeyCode::Char('a') if self.mode == VimMode::Visual => {
                self.parse_state = ParseState::PendingTextObject {
                    op: None,
                    count: 1,
                    inner: false,
                };
                EditorAction::None
            }
            // Operators on selection
            KeyCode::Char('d') | KeyCode::Char('x') => {
                self.execute_visual_op(Operator::Delete);
                self.mode = VimMode::Normal;
                EditorAction::ModeChanged(VimMode::Normal)
            }
            KeyCode::Char('c') => {
                self.execute_visual_op(Operator::Change);
                self.mode = VimMode::Insert;
                EditorAction::ModeChanged(VimMode::Insert)
            }
            KeyCode::Char('y') => {
                self.execute_visual_op(Operator::Yank);
                self.mode = VimMode::Normal;
                EditorAction::ModeChanged(VimMode::Normal)
            }
            KeyCode::Char('J') => {
                self.save_undo();
                let (sr, er) = self.visual_line_range();
                self.buffer.cursor_row = sr;
                for _ in sr..er {
                    self.buffer.join_lines();
                }
                self.mode = VimMode::Normal;
                EditorAction::ModeChanged(VimMode::Normal)
            }
            _ => EditorAction::None,
        }
    }

    // ── Motion execution ─────────────────────────────────────────────

    fn execute_motion(&mut self, motion: Motion, count: usize) {
        for _ in 0..count {
            match motion {
                Motion::Left => self.buffer.cursor_left(),
                Motion::Right => self.buffer.cursor_right(),
                Motion::Up => self.buffer.cursor_up(),
                Motion::Down => self.buffer.cursor_down(),
                Motion::WordForward => {
                    let (r, c) = find_word_forward(
                        &self.buffer.lines,
                        self.buffer.cursor_row,
                        self.buffer.cursor_col,
                    );
                    self.buffer.cursor_row = r;
                    self.buffer.cursor_col = c;
                    self.buffer.desired_col = c;
                }
                Motion::WordBackward => {
                    let (r, c) = find_word_backward(
                        &self.buffer.lines,
                        self.buffer.cursor_row,
                        self.buffer.cursor_col,
                    );
                    self.buffer.cursor_row = r;
                    self.buffer.cursor_col = c;
                    self.buffer.desired_col = c;
                }
                Motion::WordEnd => {
                    let (r, c) = find_word_end(
                        &self.buffer.lines,
                        self.buffer.cursor_row,
                        self.buffer.cursor_col,
                    );
                    self.buffer.cursor_row = r;
                    self.buffer.cursor_col = c;
                    self.buffer.desired_col = c;
                }
                Motion::LineStart => self.buffer.cursor_home(),
                Motion::LineEnd => self.buffer.cursor_end(),
                Motion::FileTop => self.buffer.goto_top(),
                Motion::FileBottom => self.buffer.goto_bottom(),
                Motion::HalfPageDown => {
                    let half = self.visible_height / 2;
                    for _ in 0..half {
                        self.buffer.cursor_down();
                    }
                }
                Motion::HalfPageUp => {
                    let half = self.visible_height / 2;
                    for _ in 0..half {
                        self.buffer.cursor_up();
                    }
                }
                Motion::FindChar(c) => {
                    if let Some(pos) =
                        find_char_forward(self.buffer.current_line(), self.buffer.cursor_col, c)
                    {
                        self.buffer.cursor_col = pos;
                        self.buffer.desired_col = pos;
                    }
                }
                Motion::FindCharBack(c) => {
                    if let Some(pos) =
                        find_char_backward(self.buffer.current_line(), self.buffer.cursor_col, c)
                    {
                        self.buffer.cursor_col = pos;
                        self.buffer.desired_col = pos;
                    }
                }
                Motion::TillChar(c) => {
                    if let Some(pos) =
                        find_till_forward(self.buffer.current_line(), self.buffer.cursor_col, c)
                    {
                        self.buffer.cursor_col = pos;
                        self.buffer.desired_col = pos;
                    }
                }
                Motion::TillCharBack(c) => {
                    if let Some(pos) =
                        find_till_backward(self.buffer.current_line(), self.buffer.cursor_col, c)
                    {
                        self.buffer.cursor_col = pos;
                        self.buffer.desired_col = pos;
                    }
                }
            }
        }
    }

    /// Compute where a motion would land without moving the cursor.
    fn motion_target(&self, motion: Motion, count: usize) -> (usize, usize) {
        let mut row = self.buffer.cursor_row;
        let mut col = self.buffer.cursor_col;

        for _ in 0..count {
            match motion {
                Motion::Left => {
                    if col > 0 {
                        let line = &self.buffer.lines[row];
                        col = line[..col]
                            .char_indices()
                            .next_back()
                            .map(|(i, _)| i)
                            .unwrap_or(0);
                    }
                }
                Motion::Right => {
                    let line_len = self.buffer.lines[row].len();
                    if col < line_len {
                        let line = &self.buffer.lines[row];
                        col = line[col..]
                            .char_indices()
                            .nth(1)
                            .map(|(i, _)| col + i)
                            .unwrap_or(line_len);
                    }
                }
                Motion::Up => {
                    if row > 0 {
                        row -= 1;
                        col = col.min(self.buffer.lines[row].len());
                    }
                }
                Motion::Down => {
                    if row < self.buffer.lines.len() - 1 {
                        row += 1;
                        col = col.min(self.buffer.lines[row].len());
                    }
                }
                Motion::WordForward => {
                    let (r, c) = find_word_forward(&self.buffer.lines, row, col);
                    row = r;
                    col = c;
                }
                Motion::WordBackward => {
                    let (r, c) = find_word_backward(&self.buffer.lines, row, col);
                    row = r;
                    col = c;
                }
                Motion::WordEnd => {
                    let (r, c) = find_word_end(&self.buffer.lines, row, col);
                    row = r;
                    // For operators, we need to include the end character
                    col = c;
                }
                Motion::LineStart => {
                    col = 0;
                }
                Motion::LineEnd => {
                    col = self.buffer.lines[row].len();
                }
                Motion::FileTop => {
                    row = 0;
                    col = 0;
                }
                Motion::FileBottom => {
                    row = self.buffer.lines.len() - 1;
                    col = 0;
                }
                Motion::FindChar(ch) => {
                    if let Some(pos) = find_char_forward(&self.buffer.lines[row], col, ch) {
                        col = pos;
                    }
                }
                Motion::FindCharBack(ch) => {
                    if let Some(pos) = find_char_backward(&self.buffer.lines[row], col, ch) {
                        col = pos;
                    }
                }
                Motion::TillChar(ch) => {
                    if let Some(pos) = find_till_forward(&self.buffer.lines[row], col, ch) {
                        col = pos;
                    }
                }
                Motion::TillCharBack(ch) => {
                    if let Some(pos) = find_till_backward(&self.buffer.lines[row], col, ch) {
                        col = pos;
                    }
                }
                _ => {}
            }
        }

        (row, col)
    }

    // ── Operator execution ───────────────────────────────────────────

    fn execute_operator_motion(&mut self, op: Operator, motion: Motion, count: usize) {
        let (target_row, target_col) = self.motion_target(motion, count);
        let cur_row = self.buffer.cursor_row;
        let cur_col = self.buffer.cursor_col;

        // Determine range (ensure start < end)
        let (sr, sc, er, ec) = if (target_row, target_col) < (cur_row, cur_col) {
            (target_row, target_col, cur_row, cur_col)
        } else {
            (cur_row, cur_col, target_row, target_col)
        };

        // For word end motion, include the character at the end
        let ec = match motion {
            Motion::WordEnd | Motion::FindChar(_) | Motion::TillChar(_) => {
                // Include the character at ec
                let line = &self.buffer.lines[er];
                line[ec..]
                    .char_indices()
                    .nth(1)
                    .map(|(i, _)| ec + i)
                    .unwrap_or(line.len())
            }
            _ => ec,
        };

        self.save_undo();

        match op {
            Operator::Delete => {
                let deleted = self.buffer.delete_range(sr, sc, er, ec);
                self.register = Register {
                    content: deleted,
                    linewise: false,
                };
                self.buffer.clamp_cursor_col(false);
            }
            Operator::Change => {
                let deleted = self.buffer.delete_range(sr, sc, er, ec);
                self.register = Register {
                    content: deleted,
                    linewise: false,
                };
                self.mode = VimMode::Insert;
            }
            Operator::Yank => {
                let yanked = self.buffer.get_range(sr, sc, er, ec);
                self.register = Register {
                    content: yanked,
                    linewise: false,
                };
                // Cursor goes to start of yanked range
                self.buffer.cursor_row = sr;
                self.buffer.cursor_col = sc;
            }
        }
    }

    /// Execute operator with a line-based motion (j, k).
    fn execute_operator_line_motion(&mut self, op: Operator, count: usize, down: bool) {
        let cur_row = self.buffer.cursor_row;
        let target_row = if down {
            (cur_row + count).min(self.buffer.lines.len() - 1)
        } else {
            cur_row.saturating_sub(count)
        };
        let start = cur_row.min(target_row);
        let end = cur_row.max(target_row);

        self.save_undo();
        match op {
            Operator::Delete => {
                let deleted = self.buffer.delete_line_range(start, end);
                self.register = Register {
                    content: deleted,
                    linewise: true,
                };
            }
            Operator::Change => {
                let deleted = self.buffer.delete_line_range(start, end);
                self.register = Register {
                    content: deleted,
                    linewise: true,
                };
                // Insert a blank line for editing
                if start >= self.buffer.lines.len() {
                    self.buffer.lines.push(String::new());
                    self.buffer.cursor_row = self.buffer.lines.len() - 1;
                } else {
                    self.buffer.lines.insert(start, String::new());
                    self.buffer.cursor_row = start;
                }
                self.buffer.cursor_col = 0;
                self.mode = VimMode::Insert;
            }
            Operator::Yank => {
                let yanked = self.buffer.get_line_range(start, end);
                self.register = Register {
                    content: yanked,
                    linewise: true,
                };
                self.buffer.cursor_row = start;
                self.buffer.cursor_col = 0;
            }
        }
    }

    /// Execute a line-wise operation (dd, yy, cc).
    fn execute_line_op(&mut self, op: Operator, count: usize) {
        let cur_row = self.buffer.cursor_row;
        let end_row = (cur_row + count - 1).min(self.buffer.lines.len() - 1);

        self.save_undo();
        match op {
            Operator::Delete => {
                let deleted = self.buffer.delete_line_range(cur_row, end_row);
                self.register = Register {
                    content: deleted,
                    linewise: true,
                };
            }
            Operator::Yank => {
                let yanked = self.buffer.get_line_range(cur_row, end_row);
                self.register = Register {
                    content: yanked,
                    linewise: true,
                };
            }
            Operator::Change => {
                let deleted = self.buffer.delete_line_range(cur_row, end_row);
                self.register = Register {
                    content: deleted,
                    linewise: true,
                };
                // Insert blank line for editing
                if cur_row >= self.buffer.lines.len() {
                    self.buffer.lines.push(String::new());
                    self.buffer.cursor_row = self.buffer.lines.len() - 1;
                } else {
                    self.buffer.lines.insert(cur_row, String::new());
                    self.buffer.cursor_row = cur_row;
                }
                self.buffer.cursor_col = 0;
                self.mode = VimMode::Insert;
            }
        }
    }

    // ── Text objects ─────────────────────────────────────────────────

    /// Compute the range (start_row, start_col, end_row, end_col) of a text object.
    /// end_col is exclusive.
    fn compute_text_object(&self, obj: TextObject) -> Option<(usize, usize, usize, usize)> {
        let row = self.buffer.cursor_row;
        let col = self.buffer.cursor_col;
        let line = &self.buffer.lines[row];

        match obj {
            TextObject::InnerWord | TextObject::AroundWord => {
                if line.is_empty() {
                    return None;
                }
                let chars: Vec<(usize, char)> = line.char_indices().collect();
                let pos = chars
                    .iter()
                    .position(|(i, _)| *i >= col)
                    .unwrap_or(chars.len() - 1);
                let current_class = char_class(chars[pos].1);

                // Find start of word
                let mut start = pos;
                while start > 0 && char_class(chars[start - 1].1) == current_class {
                    start -= 1;
                }

                // Find end of word
                let mut end = pos;
                while end + 1 < chars.len() && char_class(chars[end + 1].1) == current_class {
                    end += 1;
                }

                let start_col = chars[start].0;
                let end_col = if end + 1 < chars.len() {
                    chars[end + 1].0
                } else {
                    line.len()
                };

                if matches!(obj, TextObject::AroundWord) {
                    // Include trailing whitespace
                    let mut ae = end + 1;
                    while ae < chars.len() && char_class(chars[ae].1) == CharClass::Whitespace {
                        ae += 1;
                    }
                    let around_end = if ae < chars.len() {
                        chars[ae].0
                    } else {
                        line.len()
                    };
                    if around_end > end_col {
                        return Some((row, start_col, row, around_end));
                    }
                    // Or include leading whitespace if no trailing
                    let mut as_ = start;
                    while as_ > 0 && char_class(chars[as_ - 1].1) == CharClass::Whitespace {
                        as_ -= 1;
                    }
                    let around_start = chars[as_].0;
                    return Some((row, around_start, row, end_col));
                }

                Some((row, start_col, row, end_col))
            }
            TextObject::InnerQuote(q) | TextObject::AroundQuote(q) => {
                // Find matching quotes on the current line
                let chars: Vec<(usize, char)> = line.char_indices().collect();
                let mut quote_positions = Vec::new();
                for &(i, ch) in &chars {
                    if ch == q {
                        quote_positions.push(i);
                    }
                }
                // Find the pair surrounding the cursor
                for pair in quote_positions.chunks(2) {
                    if pair.len() == 2 && pair[0] <= col && col <= pair[1] {
                        if matches!(obj, TextObject::InnerQuote(_)) {
                            // One char after opening quote to closing quote
                            let inner_start = pair[0]
                                + chars
                                    .iter()
                                    .find(|(i, _)| *i == pair[0])
                                    .unwrap()
                                    .1
                                    .len_utf8();
                            return Some((row, inner_start, row, pair[1]));
                        } else {
                            let after_close = pair[1]
                                + chars
                                    .iter()
                                    .find(|(i, _)| *i == pair[1])
                                    .unwrap()
                                    .1
                                    .len_utf8();
                            return Some((row, pair[0], row, after_close));
                        }
                    }
                }
                None
            }
            TextObject::InnerBracket(open) | TextObject::AroundBracket(open) => {
                let close = match open {
                    '(' => ')',
                    '[' => ']',
                    '{' => '}',
                    '<' => '>',
                    _ => return None,
                };
                // Search for matching brackets, handling nesting
                // First find the opening bracket before/at cursor
                let full_text = self.buffer.text();
                let cursor_offset = self.buffer.lines[..row]
                    .iter()
                    .map(|l| l.len() + 1)
                    .sum::<usize>()
                    + col;

                let chars: Vec<(usize, char)> = full_text.char_indices().collect();

                // Find the opening bracket
                let mut depth = 0i32;
                let mut open_offset = None;
                for &(i, ch) in chars.iter().rev() {
                    if i > cursor_offset {
                        continue;
                    }
                    if ch == close {
                        depth += 1;
                    } else if ch == open {
                        if depth == 0 {
                            open_offset = Some(i);
                            break;
                        }
                        depth -= 1;
                    }
                }

                let open_offset = open_offset?;

                // Find matching close bracket
                let mut depth = 0i32;
                let mut close_offset = None;
                for &(i, ch) in &chars {
                    if i <= open_offset {
                        continue;
                    }
                    if ch == open {
                        depth += 1;
                    } else if ch == close {
                        if depth == 0 {
                            close_offset = Some(i);
                            break;
                        }
                        depth -= 1;
                    }
                }

                let close_offset = close_offset?;

                // Convert offsets back to (row, col)
                let (sr, sc) = offset_to_pos(&self.buffer.lines, open_offset);
                let (er, ec) = offset_to_pos(&self.buffer.lines, close_offset);

                if matches!(obj, TextObject::InnerBracket(_)) {
                    // Inside the brackets (exclusive)
                    let inner_start = open_offset + open.len_utf8();
                    let (isr, isc) = offset_to_pos(&self.buffer.lines, inner_start);
                    Some((isr, isc, er, ec))
                } else {
                    // Include the brackets
                    let after_close = close_offset + close.len_utf8();
                    let (aer, aec) = offset_to_pos(&self.buffer.lines, after_close);
                    Some((sr, sc, aer, aec))
                }
            }
            TextObject::InnerParagraph | TextObject::AroundParagraph => {
                // A paragraph is a block of non-empty lines
                let mut start = row;
                while start > 0 && !self.buffer.lines[start - 1].is_empty() {
                    start -= 1;
                }
                let mut end = row;
                while end + 1 < self.buffer.lines.len() && !self.buffer.lines[end + 1].is_empty() {
                    end += 1;
                }

                if matches!(obj, TextObject::AroundParagraph) {
                    // Include trailing blank lines
                    while end + 1 < self.buffer.lines.len() && self.buffer.lines[end + 1].is_empty()
                    {
                        end += 1;
                    }
                }

                Some((start, 0, end, self.buffer.lines[end].len()))
            }
        }
    }

    fn execute_text_object_op(&mut self, op: Operator, obj: TextObject, _count: usize) {
        let Some((sr, sc, er, ec)) = self.compute_text_object(obj) else {
            return;
        };

        self.save_undo();

        match op {
            Operator::Delete => {
                let deleted = self.buffer.delete_range(sr, sc, er, ec);
                self.register = Register {
                    content: deleted,
                    linewise: false,
                };
                self.buffer.clamp_cursor_col(false);
            }
            Operator::Change => {
                let deleted = self.buffer.delete_range(sr, sc, er, ec);
                self.register = Register {
                    content: deleted,
                    linewise: false,
                };
                self.mode = VimMode::Insert;
            }
            Operator::Yank => {
                let yanked = self.buffer.get_range(sr, sc, er, ec);
                self.register = Register {
                    content: yanked,
                    linewise: false,
                };
                self.buffer.cursor_row = sr;
                self.buffer.cursor_col = sc;
            }
        }
    }

    // ── Visual mode operations ───────────────────────────────────────

    /// Get the visual selection range for char-wise visual mode.
    fn visual_char_range(&self) -> (usize, usize, usize, usize) {
        let ar = self.visual_anchor_row;
        let ac = self.visual_anchor_col;
        let cr = self.buffer.cursor_row;
        let cc = self.buffer.cursor_col;

        if (ar, ac) <= (cr, cc) {
            // Include the character at the cursor
            let end_col = {
                let line = &self.buffer.lines[cr];
                line[cc..]
                    .char_indices()
                    .nth(1)
                    .map(|(i, _)| cc + i)
                    .unwrap_or(line.len())
            };
            (ar, ac, cr, end_col)
        } else {
            let end_col = {
                let line = &self.buffer.lines[ar];
                line[ac..]
                    .char_indices()
                    .nth(1)
                    .map(|(i, _)| ac + i)
                    .unwrap_or(line.len())
            };
            (cr, cc, ar, end_col)
        }
    }

    /// Get the visual selection range for line-wise visual mode.
    fn visual_line_range(&self) -> (usize, usize) {
        let ar = self.visual_anchor_row;
        let cr = self.buffer.cursor_row;
        (ar.min(cr), ar.max(cr))
    }

    fn execute_visual_op(&mut self, op: Operator) {
        self.save_undo();

        if self.mode == VimMode::VisualLine {
            let (start, end) = self.visual_line_range();
            match op {
                Operator::Delete | Operator::Change => {
                    let deleted = self.buffer.delete_line_range(start, end);
                    self.register = Register {
                        content: deleted,
                        linewise: true,
                    };
                    if op == Operator::Change {
                        if start >= self.buffer.lines.len() {
                            self.buffer.lines.push(String::new());
                            self.buffer.cursor_row = self.buffer.lines.len() - 1;
                        } else {
                            self.buffer.lines.insert(start, String::new());
                            self.buffer.cursor_row = start;
                        }
                        self.buffer.cursor_col = 0;
                    }
                }
                Operator::Yank => {
                    let yanked = self.buffer.get_line_range(start, end);
                    self.register = Register {
                        content: yanked,
                        linewise: true,
                    };
                    self.buffer.cursor_row = start;
                    self.buffer.cursor_col = 0;
                }
            }
        } else {
            let (sr, sc, er, ec) = self.visual_char_range();
            match op {
                Operator::Delete | Operator::Change => {
                    let deleted = self.buffer.delete_range(sr, sc, er, ec);
                    self.register = Register {
                        content: deleted,
                        linewise: false,
                    };
                    self.buffer.clamp_cursor_col(op == Operator::Change);
                }
                Operator::Yank => {
                    let yanked = self.buffer.get_range(sr, sc, er, ec);
                    self.register = Register {
                        content: yanked,
                        linewise: false,
                    };
                    self.buffer.cursor_row = sr;
                    self.buffer.cursor_col = sc;
                }
            }
        }
    }

    // ── Paste ────────────────────────────────────────────────────────

    fn paste_after(&mut self) {
        if self.register.content.is_empty() {
            return;
        }
        if self.register.linewise {
            self.buffer.insert_lines_below(&self.register.content);
        } else {
            // Paste after cursor position
            self.buffer.cursor_right();
            self.buffer.insert_text(&self.register.content);
            if self.buffer.cursor_col > 0 {
                self.buffer.cursor_left();
            }
        }
    }

    fn paste_before(&mut self) {
        if self.register.content.is_empty() {
            return;
        }
        if self.register.linewise {
            self.buffer.insert_lines_above(&self.register.content);
        } else {
            self.buffer.insert_text(&self.register.content);
            if self.buffer.cursor_col > 0 {
                self.buffer.cursor_left();
            }
        }
    }

    // ── Rendering ────────────────────────────────────────────────────

    pub fn render(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        self.visible_height = area.height as usize;

        // Line number gutter width (relative line numbers)
        let max_line_num = self.buffer.line_count();
        let gutter_width: u16 = format!("{}", max_line_num).len() as u16 + 2; // " N "
        let text_area = Rect {
            x: area.x + gutter_width,
            width: area.width.saturating_sub(gutter_width),
            ..area
        };
        let gutter_area = Rect {
            width: gutter_width,
            ..area
        };

        let visible_lines = area.height as usize;

        // Scrolling: keep cursor in view
        let scroll_offset = if self.buffer.cursor_row >= visible_lines {
            self.buffer.cursor_row - visible_lines + 1
        } else {
            0
        };

        // Visual selection range
        let visual_range = match self.mode {
            VimMode::Visual => Some(self.visual_char_range()),
            VimMode::VisualLine => {
                let (sr, er) = self.visual_line_range();
                Some((sr, 0, er, usize::MAX))
            }
            _ => None,
        };

        let mut gutter_lines: Vec<Line> = Vec::new();
        let mut text_lines: Vec<Line> = Vec::new();

        for i in scroll_offset..self.buffer.line_count().min(scroll_offset + visible_lines) {
            let is_current = i == self.buffer.cursor_row;

            // Relative line numbers
            let line_num_display = if is_current {
                format!("{:>width$} ", i + 1, width = gutter_width as usize - 2)
            } else {
                let rel = if i > self.buffer.cursor_row {
                    i - self.buffer.cursor_row
                } else {
                    self.buffer.cursor_row - i
                };
                format!("{:>width$} ", rel, width = gutter_width as usize - 2)
            };

            let gutter_style = if is_current && focused {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            gutter_lines.push(Line::from(Span::styled(line_num_display, gutter_style)));

            // Text content with visual selection highlighting
            let line_text = &self.buffer.lines[i];
            if let Some((vsr, vsc, ver, vec_)) = visual_range {
                let line = render_line_with_selection(
                    line_text, i, vsr, vsc, ver, vec_, is_current, focused,
                );
                text_lines.push(line);
            } else {
                let text_style = if is_current && focused {
                    Style::default().fg(Color::White)
                } else {
                    Style::default().fg(Color::Gray)
                };
                text_lines.push(Line::from(Span::styled(line_text.clone(), text_style)));
            }
        }

        // Fill remaining lines with ~ (like vim)
        for _ in self.buffer.line_count().saturating_sub(scroll_offset)..visible_lines {
            gutter_lines.push(Line::from(Span::styled(
                format!("{:>width$} ", "~", width = gutter_width as usize - 2),
                Style::default().fg(Color::DarkGray),
            )));
            text_lines.push(Line::from(""));
        }

        frame.render_widget(Paragraph::new(gutter_lines), gutter_area);
        frame.render_widget(Paragraph::new(text_lines), text_area);

        // Show cursor
        if focused {
            let visible_row = self.buffer.cursor_row.saturating_sub(scroll_offset);
            let cursor_x = text_area.x + self.buffer.cursor_col as u16;
            let cursor_y = text_area.y + visible_row as u16;
            if cursor_x < text_area.x + text_area.width && cursor_y < text_area.y + text_area.height
            {
                frame.set_cursor_position((cursor_x, cursor_y));
            }
        }
    }
}

// ── Helper functions ─────────────────────────────────────────────────

/// Convert a byte offset in the full text to (row, col).
fn offset_to_pos(lines: &[String], offset: usize) -> (usize, usize) {
    let mut remaining = offset;
    for (row, line) in lines.iter().enumerate() {
        if remaining <= line.len() {
            return (row, remaining);
        }
        remaining -= line.len() + 1; // +1 for newline
    }
    let last = lines.len() - 1;
    (last, lines[last].len())
}

/// Render a line with visual selection highlighting.
fn render_line_with_selection(
    line_text: &str,
    line_row: usize,
    sel_start_row: usize,
    sel_start_col: usize,
    sel_end_row: usize,
    sel_end_col: usize,
    is_current: bool,
    focused: bool,
) -> Line<'static> {
    let normal_style = if is_current && focused {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(Color::Gray)
    };
    let selected_style = Style::default()
        .bg(Color::Rgb(68, 68, 120))
        .fg(Color::White);

    // Determine selection range on this line
    let (sel_start, sel_end) = if line_row < sel_start_row || line_row > sel_end_row {
        // Not in selection
        return Line::from(Span::styled(line_text.to_string(), normal_style));
    } else {
        let start = if line_row == sel_start_row {
            sel_start_col.min(line_text.len())
        } else {
            0
        };
        let end = if line_row == sel_end_row {
            sel_end_col.min(line_text.len())
        } else {
            line_text.len()
        };
        (start, end)
    };

    if sel_start >= sel_end && sel_start >= line_text.len() {
        return Line::from(Span::styled(line_text.to_string(), normal_style));
    }

    let mut spans = Vec::new();
    if sel_start > 0 {
        spans.push(Span::styled(
            line_text[..sel_start].to_string(),
            normal_style,
        ));
    }
    let end = sel_end.min(line_text.len());
    if sel_start < end {
        spans.push(Span::styled(
            line_text[sel_start..end].to_string(),
            selected_style,
        ));
    }
    if end < line_text.len() {
        spans.push(Span::styled(line_text[end..].to_string(), normal_style));
    }

    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn esc() -> KeyEvent {
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
    }

    fn enter() -> KeyEvent {
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
    }

    #[test]
    fn test_basic_insert() {
        let mut ed = VimEditor::new();
        ed.handle_key(key('i')); // enter insert mode
        ed.handle_key(key('h'));
        ed.handle_key(key('e'));
        ed.handle_key(key('l'));
        ed.handle_key(key('l'));
        ed.handle_key(key('o'));
        assert_eq!(ed.text(), "hello");
    }

    #[test]
    fn test_navigation() {
        let mut ed = VimEditor::from_text("hello world");
        assert_eq!(ed.buffer.cursor_col, 0);
        ed.handle_key(key('l'));
        assert_eq!(ed.buffer.cursor_col, 1);
        ed.handle_key(key('h'));
        assert_eq!(ed.buffer.cursor_col, 0);
        ed.handle_key(key('$'));
        assert_eq!(ed.buffer.cursor_col, 11);
        ed.handle_key(key('0'));
        assert_eq!(ed.buffer.cursor_col, 0);
    }

    #[test]
    fn test_word_motions() {
        let mut ed = VimEditor::from_text("hello world foo");
        ed.handle_key(key('w'));
        assert_eq!(ed.buffer.cursor_col, 6); // "world"
        ed.handle_key(key('w'));
        assert_eq!(ed.buffer.cursor_col, 12); // "foo"
        ed.handle_key(key('b'));
        assert_eq!(ed.buffer.cursor_col, 6); // back to "world"
        ed.handle_key(key('e'));
        assert_eq!(ed.buffer.cursor_col, 10); // end of "world"
    }

    #[test]
    fn test_delete_word() {
        let mut ed = VimEditor::from_text("hello world");
        ed.handle_key(key('d'));
        ed.handle_key(key('w'));
        assert_eq!(ed.text(), "world");
    }

    #[test]
    fn test_delete_inner_word() {
        let mut ed = VimEditor::from_text("hello world");
        ed.buffer.cursor_col = 6; // on 'w' of "world"
        ed.handle_key(key('d'));
        ed.handle_key(key('i'));
        ed.handle_key(key('w'));
        assert_eq!(ed.text(), "hello ");
    }

    #[test]
    fn test_yank_and_paste() {
        let mut ed = VimEditor::from_text("hello\nworld");
        // yy (yank line)
        ed.handle_key(key('y'));
        ed.handle_key(key('y'));
        assert!(ed.register.linewise);
        assert_eq!(ed.register.content, "hello");
        // p (paste below)
        ed.handle_key(key('p'));
        assert_eq!(ed.text(), "hello\nhello\nworld");
    }

    #[test]
    fn test_dd() {
        let mut ed = VimEditor::from_text("hello\nworld\nfoo");
        ed.handle_key(key('d'));
        ed.handle_key(key('d'));
        assert_eq!(ed.text(), "world\nfoo");
    }

    #[test]
    fn test_count_prefix() {
        let mut ed = VimEditor::from_text("hello world foo bar baz");
        ed.handle_key(key('2'));
        ed.handle_key(key('w'));
        assert_eq!(ed.buffer.cursor_col, 12); // "foo"
    }

    #[test]
    fn test_undo_redo() {
        let mut ed = VimEditor::from_text("hello");
        ed.handle_key(key('d'));
        ed.handle_key(key('d'));
        assert_eq!(ed.text(), "");

        // Undo
        ed.handle_key(key('u'));
        assert_eq!(ed.text(), "hello");

        // Redo
        ed.handle_key(ctrl('r'));
        assert_eq!(ed.text(), "");
    }

    #[test]
    fn test_visual_mode_delete() {
        let mut ed = VimEditor::from_text("hello world");
        ed.handle_key(key('v'));
        assert_eq!(ed.mode, VimMode::Visual);
        // Select "hello"
        ed.handle_key(key('e'));
        ed.handle_key(key('d'));
        assert_eq!(ed.text(), " world");
        assert_eq!(ed.mode, VimMode::Normal);
    }

    #[test]
    fn test_visual_line_yank() {
        let mut ed = VimEditor::from_text("hello\nworld\nfoo");
        ed.handle_key(key('V'));
        assert_eq!(ed.mode, VimMode::VisualLine);
        ed.handle_key(key('j'));
        ed.handle_key(key('y'));
        assert!(ed.register.linewise);
        assert_eq!(ed.register.content, "hello\nworld");
        assert_eq!(ed.mode, VimMode::Normal);
    }

    #[test]
    fn test_visual_inner_word_yank() {
        let mut ed = VimEditor::from_text("hello world");
        ed.buffer.cursor_col = 0; // on 'h'
                                  // v i w y  — visual inner word yank
        ed.handle_key(key('v'));
        ed.handle_key(key('i'));
        ed.handle_key(key('w'));
        ed.handle_key(key('y'));
        assert_eq!(ed.register.content, "hello");
        assert_eq!(ed.mode, VimMode::Normal);
    }

    #[test]
    fn test_x_deletes_char() {
        let mut ed = VimEditor::from_text("hello");
        ed.handle_key(key('x'));
        assert_eq!(ed.text(), "ello");
    }

    #[test]
    fn test_replace_char() {
        let mut ed = VimEditor::from_text("hello");
        ed.handle_key(key('r'));
        ed.handle_key(key('H'));
        assert_eq!(ed.text(), "Hello");
    }

    #[test]
    fn test_change_word() {
        let mut ed = VimEditor::from_text("hello world");
        ed.handle_key(key('c'));
        ed.handle_key(key('w'));
        assert_eq!(ed.mode, VimMode::Insert);
        assert_eq!(ed.text(), "world");
    }

    #[test]
    fn test_o_and_O() {
        let mut ed = VimEditor::from_text("hello\nworld");
        ed.handle_key(key('o')); // open line below
        assert_eq!(ed.mode, VimMode::Insert);
        assert_eq!(ed.buffer.cursor_row, 1);
        assert_eq!(ed.text(), "hello\n\nworld");

        ed.handle_key(esc());
        ed.buffer.cursor_row = 2; // on "world"
        ed.handle_key(key('O')); // open line above
        assert_eq!(ed.buffer.cursor_row, 2);
        assert_eq!(ed.text(), "hello\n\n\nworld");
    }

    #[test]
    fn test_join_lines() {
        let mut ed = VimEditor::from_text("hello\n  world");
        ed.handle_key(key('J'));
        assert_eq!(ed.text(), "hello world");
    }

    #[test]
    fn test_d_dollar() {
        let mut ed = VimEditor::from_text("hello world");
        ed.buffer.cursor_col = 5;
        ed.handle_key(key('D'));
        assert_eq!(ed.text(), "hello");
    }

    #[test]
    fn test_cc() {
        let mut ed = VimEditor::from_text("hello\nworld");
        ed.handle_key(key('c'));
        ed.handle_key(key('c'));
        assert_eq!(ed.mode, VimMode::Insert);
        // The line should be replaced with empty
        assert_eq!(ed.buffer.lines.len(), 2); // blank + "world"
    }

    #[test]
    fn test_find_char_motion() {
        let mut ed = VimEditor::from_text("hello world");
        ed.handle_key(key('f'));
        ed.handle_key(key('w'));
        assert_eq!(ed.buffer.cursor_col, 6);
    }

    #[test]
    fn test_2dd() {
        let mut ed = VimEditor::from_text("one\ntwo\nthree");
        ed.handle_key(key('2'));
        ed.handle_key(key('d'));
        ed.handle_key(key('d'));
        assert_eq!(ed.text(), "three");
    }

    #[test]
    fn test_delete_inner_quotes() {
        let mut ed = VimEditor::from_text("say \"hello world\"");
        ed.buffer.cursor_col = 6; // inside the quotes
        ed.handle_key(key('d'));
        ed.handle_key(key('i'));
        ed.handle_key(key('"'));
        assert_eq!(ed.text(), "say \"\"");
    }

    #[test]
    fn test_gg_and_G() {
        let mut ed = VimEditor::from_text("one\ntwo\nthree");
        ed.handle_key(key('G'));
        assert_eq!(ed.buffer.cursor_row, 2);
        ed.handle_key(key('g'));
        ed.handle_key(key('g'));
        assert_eq!(ed.buffer.cursor_row, 0);
    }

    #[test]
    fn test_paste_charwise() {
        let mut ed = VimEditor::from_text("hello world");
        ed.handle_key(key('d'));
        ed.handle_key(key('w'));
        // Register has "hello "
        ed.handle_key(key('$'));
        ed.handle_key(key('p'));
        // Should paste after end of line
        let text = ed.text();
        assert!(text.contains("hello"));
    }

    #[test]
    fn test_3j() {
        let mut ed = VimEditor::from_text("a\nb\nc\nd\ne");
        ed.handle_key(key('3'));
        ed.handle_key(key('j'));
        assert_eq!(ed.buffer.cursor_row, 3);
    }
}
