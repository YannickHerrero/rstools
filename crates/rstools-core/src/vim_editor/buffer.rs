/// A multi-line text buffer with a 2D cursor.
///
/// Lines are stored as `Vec<String>`, where each string is one line of text
/// (without trailing newline). The cursor is tracked as (row, col) where col
/// is a byte offset into the current line.
#[derive(Debug, Clone)]
pub struct TextBuffer {
    pub lines: Vec<String>,
    pub cursor_row: usize,
    pub cursor_col: usize,
    /// Desired column for vertical movement (sticky column).
    /// When moving up/down, the cursor tries to stay at this column.
    pub desired_col: usize,
    /// Whether the buffer has been modified since last save.
    pub dirty: bool,
}

impl TextBuffer {
    pub fn new() -> Self {
        Self {
            lines: vec![String::new()],
            cursor_row: 0,
            cursor_col: 0,
            desired_col: 0,
            dirty: false,
        }
    }

    /// Create a buffer from a string.
    pub fn from_text(text: &str) -> Self {
        let lines: Vec<String> = if text.is_empty() {
            vec![String::new()]
        } else {
            text.lines().map(String::from).collect()
        };
        // Ensure at least one line
        let lines = if lines.is_empty() {
            vec![String::new()]
        } else {
            lines
        };
        Self {
            lines,
            cursor_row: 0,
            cursor_col: 0,
            desired_col: 0,
            dirty: false,
        }
    }

    /// Get the full text as a single string with newlines.
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// Set the buffer content from a string, resetting cursor.
    pub fn set_text(&mut self, text: &str) {
        self.lines = if text.is_empty() {
            vec![String::new()]
        } else {
            text.lines().map(String::from).collect()
        };
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.desired_col = 0;
        self.dirty = false;
    }

    /// Number of lines in the buffer.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Get the current line.
    pub fn current_line(&self) -> &str {
        &self.lines[self.cursor_row]
    }

    /// Get the length (in bytes) of the current line.
    pub fn current_line_len(&self) -> usize {
        self.lines[self.cursor_row].len()
    }

    /// Clamp cursor_col to be within valid range for the current line.
    /// In Normal mode, the cursor can't go past the last character.
    /// In Insert mode, it can go one past (to append).
    pub fn clamp_cursor_col(&mut self, allow_past_end: bool) {
        let len = self.current_line_len();
        let max = if allow_past_end || len == 0 {
            len
        } else {
            // Find the start of the last character
            self.lines[self.cursor_row]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0)
        };
        if self.cursor_col > max {
            self.cursor_col = max;
        }
    }

    // ── Basic cursor movement ────────────────────────────────────────

    /// Move cursor left by one character.
    pub fn cursor_left(&mut self) {
        if self.cursor_col > 0 {
            let line = &self.lines[self.cursor_row];
            self.cursor_col = line[..self.cursor_col]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.desired_col = self.cursor_col;
        }
    }

    /// Move cursor right by one character.
    pub fn cursor_right(&mut self) {
        let line_len = self.current_line_len();
        if self.cursor_col < line_len {
            let line = &self.lines[self.cursor_row];
            self.cursor_col = line[self.cursor_col..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor_col + i)
                .unwrap_or(line_len);
            self.desired_col = self.cursor_col;
        }
    }

    /// Move cursor up by one line, preserving desired column.
    pub fn cursor_up(&mut self) {
        if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.cursor_col = snap_to_char_boundary(&self.lines[self.cursor_row], self.desired_col);
        }
    }

    /// Move cursor down by one line, preserving desired column.
    pub fn cursor_down(&mut self) {
        if self.cursor_row < self.lines.len() - 1 {
            self.cursor_row += 1;
            self.cursor_col = snap_to_char_boundary(&self.lines[self.cursor_row], self.desired_col);
        }
    }

    /// Move cursor to the beginning of the current line.
    pub fn cursor_home(&mut self) {
        self.cursor_col = 0;
        self.desired_col = 0;
    }

    /// Move cursor to the end of the current line.
    pub fn cursor_end(&mut self) {
        self.cursor_col = self.current_line_len();
        self.desired_col = self.cursor_col;
    }

    /// Move to the first line, column 0.
    pub fn goto_top(&mut self) {
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.desired_col = 0;
    }

    /// Move to the last line, column 0.
    pub fn goto_bottom(&mut self) {
        self.cursor_row = self.lines.len() - 1;
        self.cursor_col = 0;
        self.desired_col = 0;
    }

    // ── Insert operations ────────────────────────────────────────────

    /// Insert a character at the cursor position.
    pub fn insert_char(&mut self, c: char) {
        self.lines[self.cursor_row].insert(self.cursor_col, c);
        self.cursor_col += c.len_utf8();
        self.desired_col = self.cursor_col;
        self.dirty = true;
    }

    /// Insert a newline at the cursor position, splitting the current line.
    pub fn insert_newline(&mut self) {
        let current_line = self.lines[self.cursor_row].clone();
        let (before, after) = current_line.split_at(self.cursor_col);
        self.lines[self.cursor_row] = before.to_string();
        self.lines.insert(self.cursor_row + 1, after.to_string());
        self.cursor_row += 1;
        self.cursor_col = 0;
        self.desired_col = 0;
        self.dirty = true;
    }

    /// Delete the character before the cursor (backspace).
    pub fn backspace(&mut self) {
        if self.cursor_col > 0 {
            let line = &self.lines[self.cursor_row];
            let prev_len = line[..self.cursor_col]
                .chars()
                .last()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.cursor_col -= prev_len;
            self.lines[self.cursor_row].remove(self.cursor_col);
            self.desired_col = self.cursor_col;
            self.dirty = true;
        } else if self.cursor_row > 0 {
            // Merge with previous line
            let current = self.lines.remove(self.cursor_row);
            self.cursor_row -= 1;
            self.cursor_col = self.lines[self.cursor_row].len();
            self.lines[self.cursor_row].push_str(&current);
            self.desired_col = self.cursor_col;
            self.dirty = true;
        }
    }

    /// Delete the character at the cursor (delete key / 'x' in normal mode).
    pub fn delete_char_at_cursor(&mut self) {
        let line_len = self.current_line_len();
        if self.cursor_col < line_len {
            self.lines[self.cursor_row].remove(self.cursor_col);
            self.dirty = true;
        } else if self.cursor_row < self.lines.len() - 1 {
            // Merge next line into current
            let next = self.lines.remove(self.cursor_row + 1);
            self.lines[self.cursor_row].push_str(&next);
            self.dirty = true;
        }
    }

    // ── Line operations ──────────────────────────────────────────────

    /// Delete the current line. Returns the deleted line content.
    pub fn delete_line(&mut self) -> String {
        let deleted = self.lines.remove(self.cursor_row);
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        if self.cursor_row >= self.lines.len() {
            self.cursor_row = self.lines.len() - 1;
        }
        self.cursor_col = snap_to_char_boundary(&self.lines[self.cursor_row], self.desired_col);
        self.dirty = true;
        deleted
    }

    /// Delete N lines starting from the current line. Returns the deleted text
    /// (joined with newlines).
    pub fn delete_lines(&mut self, count: usize) -> String {
        let end = (self.cursor_row + count).min(self.lines.len());
        let deleted: Vec<String> = self.lines.drain(self.cursor_row..end).collect();
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        if self.cursor_row >= self.lines.len() {
            self.cursor_row = self.lines.len() - 1;
        }
        self.cursor_col = snap_to_char_boundary(&self.lines[self.cursor_row], self.desired_col);
        self.dirty = true;
        deleted.join("\n")
    }

    /// Insert a line below the current line and move cursor there.
    pub fn open_line_below(&mut self) {
        self.lines.insert(self.cursor_row + 1, String::new());
        self.cursor_row += 1;
        self.cursor_col = 0;
        self.desired_col = 0;
        self.dirty = true;
    }

    /// Insert a line above the current line and move cursor there.
    pub fn open_line_above(&mut self) {
        self.lines.insert(self.cursor_row, String::new());
        self.cursor_col = 0;
        self.desired_col = 0;
        self.dirty = true;
    }

    /// Join the current line with the next line (vim 'J').
    pub fn join_lines(&mut self) {
        if self.cursor_row < self.lines.len() - 1 {
            let next = self.lines.remove(self.cursor_row + 1);
            let trimmed = next.trim_start();
            let join_col = self.lines[self.cursor_row].len();
            if !self.lines[self.cursor_row].is_empty() && !trimmed.is_empty() {
                self.lines[self.cursor_row].push(' ');
            }
            self.lines[self.cursor_row].push_str(trimmed);
            self.cursor_col = join_col;
            self.desired_col = self.cursor_col;
            self.dirty = true;
        }
    }

    // ── Range operations ─────────────────────────────────────────────

    /// Delete text in a range of (row, col) positions.
    /// Returns the deleted text. `start` is inclusive, `end` is exclusive.
    pub fn delete_range(
        &mut self,
        start_row: usize,
        start_col: usize,
        end_row: usize,
        end_col: usize,
    ) -> String {
        if start_row == end_row {
            // Same line
            let line = &mut self.lines[start_row];
            let s = start_col.min(line.len());
            let e = end_col.min(line.len());
            let deleted: String = line.drain(s..e).collect();
            self.cursor_row = start_row;
            self.cursor_col = s;
            self.desired_col = self.cursor_col;
            self.dirty = true;
            deleted
        } else {
            // Multi-line
            let mut result = String::new();

            // Get the text from start_col to end of start_row
            let first_line = &self.lines[start_row];
            let s = start_col.min(first_line.len());
            result.push_str(&first_line[s..]);

            // Get complete middle lines
            for row in (start_row + 1)..end_row {
                result.push('\n');
                result.push_str(&self.lines[row]);
            }

            // Get text from beginning of end_row to end_col
            if end_row < self.lines.len() {
                result.push('\n');
                let last_line = &self.lines[end_row];
                let e = end_col.min(last_line.len());
                result.push_str(&last_line[..e]);
            }

            // Now actually remove the text
            let end_remainder = if end_row < self.lines.len() {
                let e = end_col.min(self.lines[end_row].len());
                self.lines[end_row][e..].to_string()
            } else {
                String::new()
            };

            // Truncate the start line and append the remainder
            self.lines[start_row].truncate(s);
            self.lines[start_row].push_str(&end_remainder);

            // Remove the lines in between
            let remove_count = end_row.min(self.lines.len()) - start_row;
            if remove_count > 0 {
                let drain_start = start_row + 1;
                let drain_end = (start_row + 1 + remove_count).min(self.lines.len());
                if drain_start < drain_end {
                    self.lines.drain(drain_start..drain_end);
                }
            }

            if self.lines.is_empty() {
                self.lines.push(String::new());
            }

            self.cursor_row = start_row.min(self.lines.len() - 1);
            self.cursor_col = s.min(self.lines[self.cursor_row].len());
            self.desired_col = self.cursor_col;
            self.dirty = true;
            result
        }
    }

    /// Delete lines in a range (inclusive). Returns the deleted text.
    pub fn delete_line_range(&mut self, start_row: usize, end_row: usize) -> String {
        let s = start_row.min(self.lines.len() - 1);
        let e = (end_row + 1).min(self.lines.len());
        let deleted: Vec<String> = self.lines.drain(s..e).collect();
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        if self.cursor_row >= self.lines.len() {
            self.cursor_row = self.lines.len() - 1;
        }
        self.cursor_col = snap_to_char_boundary(&self.lines[self.cursor_row], self.desired_col);
        self.dirty = true;
        deleted.join("\n")
    }

    /// Get text in a range without deleting.
    pub fn get_range(
        &self,
        start_row: usize,
        start_col: usize,
        end_row: usize,
        end_col: usize,
    ) -> String {
        if start_row == end_row {
            let line = &self.lines[start_row];
            let s = start_col.min(line.len());
            let e = end_col.min(line.len());
            line[s..e].to_string()
        } else {
            let mut result = String::new();
            let first_line = &self.lines[start_row];
            let s = start_col.min(first_line.len());
            result.push_str(&first_line[s..]);

            for row in (start_row + 1)..end_row {
                result.push('\n');
                result.push_str(&self.lines[row]);
            }

            if end_row < self.lines.len() {
                result.push('\n');
                let last_line = &self.lines[end_row];
                let e = end_col.min(last_line.len());
                result.push_str(&last_line[..e]);
            }
            result
        }
    }

    /// Get text for a line range (inclusive) without deleting.
    pub fn get_line_range(&self, start_row: usize, end_row: usize) -> String {
        let s = start_row.min(self.lines.len() - 1);
        let e = (end_row + 1).min(self.lines.len());
        self.lines[s..e].join("\n")
    }

    /// Insert text at the cursor position (may contain newlines).
    pub fn insert_text(&mut self, text: &str) {
        for c in text.chars() {
            if c == '\n' {
                self.insert_newline();
            } else {
                self.insert_char(c);
            }
        }
    }

    /// Insert lines below the current line (for pasting line-wise content).
    pub fn insert_lines_below(&mut self, text: &str) {
        let new_lines: Vec<String> = text.lines().map(String::from).collect();
        if new_lines.is_empty() {
            return;
        }
        let insert_at = self.cursor_row + 1;
        for (i, line) in new_lines.iter().enumerate() {
            self.lines.insert(insert_at + i, line.clone());
        }
        self.cursor_row = insert_at;
        self.cursor_col = 0;
        self.desired_col = 0;
        self.dirty = true;
    }

    /// Insert lines above the current line (for pasting line-wise content with P).
    pub fn insert_lines_above(&mut self, text: &str) {
        let new_lines: Vec<String> = text.lines().map(String::from).collect();
        if new_lines.is_empty() {
            return;
        }
        for (i, line) in new_lines.iter().enumerate() {
            self.lines.insert(self.cursor_row + i, line.clone());
        }
        // Cursor stays at first inserted line
        self.cursor_col = 0;
        self.desired_col = 0;
        self.dirty = true;
    }

    /// Replace a character at the cursor position.
    pub fn replace_char(&mut self, c: char) {
        let line_len = self.current_line_len();
        if self.cursor_col < line_len {
            // Remove the char at cursor, insert the new one
            let old_char_len = self.lines[self.cursor_row][self.cursor_col..]
                .chars()
                .next()
                .map(|ch| ch.len_utf8())
                .unwrap_or(0);
            self.lines[self.cursor_row].drain(self.cursor_col..self.cursor_col + old_char_len);
            self.lines[self.cursor_row].insert(self.cursor_col, c);
            self.dirty = true;
        }
    }

    /// Create a snapshot of the buffer state for undo.
    pub fn snapshot(&self) -> BufferSnapshot {
        BufferSnapshot {
            lines: self.lines.clone(),
            cursor_row: self.cursor_row,
            cursor_col: self.cursor_col,
            desired_col: self.desired_col,
        }
    }

    /// Restore from a snapshot.
    pub fn restore(&mut self, snapshot: &BufferSnapshot) {
        self.lines = snapshot.lines.clone();
        self.cursor_row = snapshot.cursor_row;
        self.cursor_col = snapshot.cursor_col;
        self.desired_col = snapshot.desired_col;
    }
}

/// A snapshot of the buffer state for undo/redo.
#[derive(Debug, Clone)]
pub struct BufferSnapshot {
    pub lines: Vec<String>,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub desired_col: usize,
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Snap a byte offset to the nearest valid char boundary in a string.
fn snap_to_char_boundary(s: &str, target: usize) -> usize {
    if target >= s.len() {
        return s.len();
    }
    // Find the char boundary at or before target
    s.char_indices()
        .take_while(|(i, _)| *i <= target)
        .last()
        .map(|(i, _)| i)
        .unwrap_or(0)
}

// ── Word boundary helpers ────────────────────────────────────────────

/// Classify a character for word movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharClass {
    Whitespace,
    Word,
    Punctuation,
}

pub fn char_class(c: char) -> CharClass {
    if c.is_whitespace() {
        CharClass::Whitespace
    } else if c.is_alphanumeric() || c == '_' {
        CharClass::Word
    } else {
        CharClass::Punctuation
    }
}

/// Find the start of the next word (vim 'w' motion).
/// Returns (row, col).
pub fn find_word_forward(lines: &[String], row: usize, col: usize) -> (usize, usize) {
    let mut r = row;
    let c = col;

    if r >= lines.len() {
        return (r, c);
    }

    let line = &lines[r];

    // If we're at a character, skip over characters of the same class
    if c < line.len() {
        let chars: Vec<(usize, char)> = line.char_indices().collect();
        let pos = chars
            .iter()
            .position(|(i, _)| *i >= c)
            .unwrap_or(chars.len());
        if pos < chars.len() {
            let start_class = char_class(chars[pos].1);
            let mut p = pos;
            // Skip same class
            while p < chars.len() && char_class(chars[p].1) == start_class {
                p += 1;
            }
            // Skip whitespace
            while p < chars.len() && char_class(chars[p].1) == CharClass::Whitespace {
                p += 1;
            }
            if p < chars.len() {
                return (r, chars[p].0);
            }
        }
    }

    // Move to next line
    r += 1;
    while r < lines.len() {
        let line = &lines[r];
        if line.is_empty() {
            return (r, 0);
        }
        // Find first non-whitespace char
        let chars: Vec<(usize, char)> = line.char_indices().collect();
        for &(i, ch) in &chars {
            if !ch.is_whitespace() {
                return (r, i);
            }
        }
        r += 1;
    }

    // End of file
    let last_row = lines.len() - 1;
    (last_row, lines[last_row].len())
}

/// Find the start of the previous word (vim 'b' motion).
pub fn find_word_backward(lines: &[String], row: usize, col: usize) -> (usize, usize) {
    let mut r = row;
    let c = col;

    if r >= lines.len() {
        return (r, c);
    }

    let line = &lines[r];
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let mut pos = chars
        .iter()
        .position(|(i, _)| *i >= c)
        .unwrap_or(chars.len());

    // Move back one position if we're at the start of a word
    if pos > 0 {
        pos -= 1;
        // Skip whitespace backward
        while pos > 0 && char_class(chars[pos].1) == CharClass::Whitespace {
            pos -= 1;
        }
        if pos > 0 || char_class(chars[pos].1) != CharClass::Whitespace {
            // Find the start of this word
            let word_class = char_class(chars[pos].1);
            while pos > 0 && char_class(chars[pos - 1].1) == word_class {
                pos -= 1;
            }
            return (r, chars[pos].0);
        }
        // pos == 0 and it might be the start
        if char_class(chars[0].1) != CharClass::Whitespace {
            return (r, 0);
        }
    }

    // Move to previous line
    if r == 0 {
        return (0, 0);
    }
    r -= 1;

    loop {
        let line = &lines[r];
        if line.is_empty() {
            return (r, 0);
        }
        let chars: Vec<(usize, char)> = line.char_indices().collect();
        let mut p = chars.len() - 1;
        // Skip trailing whitespace
        while p > 0 && char_class(chars[p].1) == CharClass::Whitespace {
            p -= 1;
        }
        // Find start of word
        let word_class = char_class(chars[p].1);
        while p > 0 && char_class(chars[p - 1].1) == word_class {
            p -= 1;
        }
        return (r, chars[p].0);
    }
}

/// Find the end of the current/next word (vim 'e' motion).
pub fn find_word_end(lines: &[String], row: usize, col: usize) -> (usize, usize) {
    let mut r = row;
    let c = col;

    if r >= lines.len() {
        return (r, c);
    }

    let line = &lines[r];
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let mut pos = chars
        .iter()
        .position(|(i, _)| *i >= c)
        .unwrap_or(chars.len());

    // Move forward one position first
    if pos < chars.len() {
        pos += 1;
    }

    // Skip whitespace
    while pos < chars.len() && char_class(chars[pos].1) == CharClass::Whitespace {
        pos += 1;
    }

    if pos < chars.len() {
        // Find end of word
        let word_class = char_class(chars[pos].1);
        while pos + 1 < chars.len() && char_class(chars[pos + 1].1) == word_class {
            pos += 1;
        }
        return (r, chars[pos].0);
    }

    // Move to next line
    r += 1;
    while r < lines.len() {
        let line = &lines[r];
        if line.is_empty() {
            r += 1;
            continue;
        }
        let chars: Vec<(usize, char)> = line.char_indices().collect();
        let mut p = 0;
        // Skip leading whitespace
        while p < chars.len() && char_class(chars[p].1) == CharClass::Whitespace {
            p += 1;
        }
        if p < chars.len() {
            let word_class = char_class(chars[p].1);
            while p + 1 < chars.len() && char_class(chars[p + 1].1) == word_class {
                p += 1;
            }
            return (r, chars[p].0);
        }
        r += 1;
    }

    // End of file
    let last_row = lines.len() - 1;
    let last_col = if lines[last_row].is_empty() {
        0
    } else {
        lines[last_row]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0)
    };
    (last_row, last_col)
}

/// Find the char on the current line (vim 'f' motion).
/// Returns the byte offset of the character if found.
pub fn find_char_forward(line: &str, col: usize, target: char) -> Option<usize> {
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let start = chars.iter().position(|(i, _)| *i > col)?;
    for &(i, ch) in &chars[start..] {
        if ch == target {
            return Some(i);
        }
    }
    None
}

/// Find the char on the current line backward (vim 'F' motion).
pub fn find_char_backward(line: &str, col: usize, target: char) -> Option<usize> {
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let end = chars
        .iter()
        .position(|(i, _)| *i >= col)
        .unwrap_or(chars.len());
    for &(i, ch) in chars[..end].iter().rev() {
        if ch == target {
            return Some(i);
        }
    }
    None
}

/// Find till char (vim 't' motion) - one position before the target.
pub fn find_till_forward(line: &str, col: usize, target: char) -> Option<usize> {
    let pos = find_char_forward(line, col, target)?;
    // One char before the found position
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let idx = chars.iter().position(|(i, _)| *i == pos)?;
    if idx > 0 {
        let prev_idx = idx - 1;
        if chars[prev_idx].0 > col {
            Some(chars[prev_idx].0)
        } else {
            Some(pos)
        }
    } else {
        Some(pos)
    }
}

/// Find till char backward (vim 'T' motion) - one position after the target.
pub fn find_till_backward(line: &str, col: usize, target: char) -> Option<usize> {
    let pos = find_char_backward(line, col, target)?;
    // One char after the found position
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let idx = chars.iter().position(|(i, _)| *i == pos)?;
    if idx + 1 < chars.len() && chars[idx + 1].0 < col {
        Some(chars[idx + 1].0)
    } else {
        Some(pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_buffer() {
        let buf = TextBuffer::new();
        assert_eq!(buf.lines, vec![""]);
        assert_eq!(buf.cursor_row, 0);
        assert_eq!(buf.cursor_col, 0);
    }

    #[test]
    fn test_from_text() {
        let buf = TextBuffer::from_text("hello\nworld");
        assert_eq!(buf.lines, vec!["hello", "world"]);
    }

    #[test]
    fn test_insert_and_text() {
        let mut buf = TextBuffer::new();
        buf.insert_char('h');
        buf.insert_char('i');
        assert_eq!(buf.text(), "hi");
    }

    #[test]
    fn test_newline() {
        let mut buf = TextBuffer::from_text("hello");
        buf.cursor_col = 3;
        buf.insert_newline();
        assert_eq!(buf.lines, vec!["hel", "lo"]);
        assert_eq!(buf.cursor_row, 1);
        assert_eq!(buf.cursor_col, 0);
    }

    #[test]
    fn test_backspace() {
        let mut buf = TextBuffer::from_text("hello");
        buf.cursor_col = 5;
        buf.backspace();
        assert_eq!(buf.text(), "hell");
    }

    #[test]
    fn test_backspace_merge_lines() {
        let mut buf = TextBuffer::from_text("hello\nworld");
        buf.cursor_row = 1;
        buf.cursor_col = 0;
        buf.backspace();
        assert_eq!(buf.lines, vec!["helloworld"]);
        assert_eq!(buf.cursor_row, 0);
        assert_eq!(buf.cursor_col, 5);
    }

    #[test]
    fn test_delete_char() {
        let mut buf = TextBuffer::from_text("hello");
        buf.cursor_col = 0;
        buf.delete_char_at_cursor();
        assert_eq!(buf.text(), "ello");
    }

    #[test]
    fn test_cursor_movement() {
        let mut buf = TextBuffer::from_text("hello\nworld");
        buf.cursor_right();
        assert_eq!(buf.cursor_col, 1);
        buf.cursor_down();
        assert_eq!(buf.cursor_row, 1);
        assert_eq!(buf.cursor_col, 1);
        buf.cursor_up();
        assert_eq!(buf.cursor_row, 0);
        buf.cursor_left();
        assert_eq!(buf.cursor_col, 0);
    }

    #[test]
    fn test_delete_line() {
        let mut buf = TextBuffer::from_text("hello\nworld\nfoo");
        buf.cursor_row = 1;
        let deleted = buf.delete_line();
        assert_eq!(deleted, "world");
        assert_eq!(buf.lines, vec!["hello", "foo"]);
    }

    #[test]
    fn test_word_forward() {
        let lines = vec!["hello world foo".to_string()];
        assert_eq!(find_word_forward(&lines, 0, 0), (0, 6));
        assert_eq!(find_word_forward(&lines, 0, 6), (0, 12));
    }

    #[test]
    fn test_word_backward() {
        let lines = vec!["hello world foo".to_string()];
        assert_eq!(find_word_backward(&lines, 0, 12), (0, 6));
        assert_eq!(find_word_backward(&lines, 0, 6), (0, 0));
    }

    #[test]
    fn test_word_end() {
        let lines = vec!["hello world".to_string()];
        assert_eq!(find_word_end(&lines, 0, 0), (0, 4));
        assert_eq!(find_word_end(&lines, 0, 4), (0, 10));
    }

    #[test]
    fn test_find_char() {
        let line = "hello world";
        assert_eq!(find_char_forward(line, 0, 'o'), Some(4));
        assert_eq!(find_char_forward(line, 4, 'o'), Some(7));
        assert_eq!(find_char_backward(line, 7, 'l'), Some(3));
    }

    #[test]
    fn test_delete_range() {
        let mut buf = TextBuffer::from_text("hello world");
        let deleted = buf.delete_range(0, 0, 0, 5);
        assert_eq!(deleted, "hello");
        assert_eq!(buf.text(), " world");
    }

    #[test]
    fn test_replace_char() {
        let mut buf = TextBuffer::from_text("hello");
        buf.cursor_col = 0;
        buf.replace_char('H');
        assert_eq!(buf.text(), "Hello");
    }

    #[test]
    fn test_join_lines() {
        let mut buf = TextBuffer::from_text("hello\n  world");
        buf.cursor_row = 0;
        buf.join_lines();
        assert_eq!(buf.text(), "hello world");
    }

    #[test]
    fn test_open_line_below() {
        let mut buf = TextBuffer::from_text("hello\nworld");
        buf.cursor_row = 0;
        buf.open_line_below();
        assert_eq!(buf.lines, vec!["hello", "", "world"]);
        assert_eq!(buf.cursor_row, 1);
    }

    #[test]
    fn test_open_line_above() {
        let mut buf = TextBuffer::from_text("hello\nworld");
        buf.cursor_row = 1;
        buf.open_line_above();
        assert_eq!(buf.lines, vec!["hello", "", "world"]);
        assert_eq!(buf.cursor_row, 1);
    }
}
