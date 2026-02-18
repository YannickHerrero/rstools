use super::buffer::BufferSnapshot;

/// Simple linear undo/redo history.
///
/// Stores buffer snapshots before each edit. `u` pops from undo stack
/// and pushes current to redo stack. `Ctrl-r` pops from redo and pushes
/// current to undo.
pub struct History {
    undo_stack: Vec<BufferSnapshot>,
    redo_stack: Vec<BufferSnapshot>,
    max_depth: usize,
}

impl History {
    pub fn new(max_depth: usize) -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            max_depth,
        }
    }

    /// Save a snapshot before an edit operation.
    /// Clears the redo stack (new edit branch).
    pub fn push(&mut self, snapshot: BufferSnapshot) {
        self.redo_stack.clear();
        self.undo_stack.push(snapshot);
        if self.undo_stack.len() > self.max_depth {
            self.undo_stack.remove(0);
        }
    }

    /// Undo: returns the previous snapshot if available.
    /// The caller should pass the current state to save for redo.
    pub fn undo(&mut self, current: BufferSnapshot) -> Option<BufferSnapshot> {
        if let Some(snapshot) = self.undo_stack.pop() {
            self.redo_stack.push(current);
            Some(snapshot)
        } else {
            None
        }
    }

    /// Redo: returns the next snapshot if available.
    /// The caller should pass the current state to save for undo.
    pub fn redo(&mut self, current: BufferSnapshot) -> Option<BufferSnapshot> {
        if let Some(snapshot) = self.redo_stack.pop() {
            self.undo_stack.push(current);
            Some(snapshot)
        } else {
            None
        }
    }

    /// Clear all history.
    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(text: &str) -> BufferSnapshot {
        BufferSnapshot {
            lines: text.lines().map(String::from).collect(),
            cursor_row: 0,
            cursor_col: 0,
            desired_col: 0,
        }
    }

    #[test]
    fn test_undo_redo() {
        let mut h = History::new(100);
        let s0 = snapshot("hello");
        let s1 = snapshot("hello world");

        h.push(s0.clone());
        // Now buffer is at "hello world", undo should give us "hello"
        let undone = h.undo(s1.clone());
        assert!(undone.is_some());
        assert_eq!(undone.unwrap().lines, vec!["hello"]);

        // Redo should give us "hello world"
        let redone = h.redo(s0.clone());
        assert!(redone.is_some());
        assert_eq!(redone.unwrap().lines, vec!["hello world"]);
    }

    #[test]
    fn test_new_edit_clears_redo() {
        let mut h = History::new(100);
        h.push(snapshot("a"));
        h.push(snapshot("b"));

        // Undo once
        let _ = h.undo(snapshot("c"));
        assert!(h.can_redo());

        // New edit clears redo
        h.push(snapshot("d"));
        assert!(!h.can_redo());
    }

    #[test]
    fn test_max_depth() {
        let mut h = History::new(3);
        h.push(snapshot("a"));
        h.push(snapshot("b"));
        h.push(snapshot("c"));
        h.push(snapshot("d")); // "a" should be dropped

        assert_eq!(h.undo_stack.len(), 3);
    }
}
