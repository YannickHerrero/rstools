use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use unicode_width::UnicodeWidthStr;

// ── TreeEntry trait ──────────────────────────────────────────────────

/// Trait that any entry type must implement to be used with TreeSidebar.
pub trait TreeEntry: Clone {
    fn id(&self) -> i64;
    fn parent_id(&self) -> Option<i64>;
    fn name(&self) -> &str;
    fn is_folder(&self) -> bool;
    fn is_expanded(&self) -> bool;
}

// ── TreeNode ─────────────────────────────────────────────────────────

/// A node in the in-memory tree representation.
#[derive(Debug, Clone)]
pub struct TreeNode<T: TreeEntry> {
    pub entry: T,
    pub children: Vec<TreeNode<T>>,
    pub expanded: bool,
}

// ── FlatEntry ────────────────────────────────────────────────────────

/// A flattened entry for rendering — one visible line in the sidebar.
#[derive(Debug, Clone)]
pub struct FlatEntry {
    pub entry_id: i64,
    pub depth: usize,
    pub name: String,
    pub is_folder: bool,
    pub is_expanded: bool,
    pub has_children: bool,
    /// For each depth level 0..depth, whether a vertical guide line (│) should
    /// be drawn. True when the ancestor at that depth has more siblings below.
    pub guide_depths: Vec<bool>,
}

// ── Clipboard ────────────────────────────────────────────────────────

/// Clipboard operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardMode {
    Copy,
    Cut,
}

/// Item stored in the clipboard.
#[derive(Debug, Clone)]
pub struct ClipboardItem {
    pub entry_id: i64,
    pub mode: ClipboardMode,
}

// ── SidebarInput ─────────────────────────────────────────────────────

/// What kind of input the sidebar is currently waiting for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidebarInput {
    /// No input active.
    None,
    /// Adding a new entry (path string).
    Adding,
    /// Renaming the selected entry.
    Renaming,
    /// Confirming deletion of an entry.
    ConfirmDelete,
}

// ── TreeSidebar ──────────────────────────────────────────────────────

/// Generic tree sidebar state, parameterized over the entry type.
pub struct TreeSidebar<T: TreeEntry> {
    /// Tree roots (top-level entries).
    pub roots: Vec<TreeNode<T>>,
    /// Flattened visible entries for rendering and navigation.
    pub flat_view: Vec<FlatEntry>,
    /// Currently selected index into flat_view.
    pub selected: usize,
    /// Clipboard for copy/cut/paste.
    pub clipboard: Option<ClipboardItem>,
    /// Current input mode for the sidebar.
    pub input_mode: SidebarInput,
    /// Text input buffer.
    pub input_buffer: String,
    /// Cursor position in the input buffer.
    pub input_cursor: usize,
    /// Whether the sidebar is visible.
    pub visible: bool,
}

impl<T: TreeEntry> TreeSidebar<T> {
    pub fn new() -> Self {
        Self {
            roots: Vec::new(),
            flat_view: Vec::new(),
            selected: 0,
            clipboard: None,
            input_mode: SidebarInput::None,
            input_buffer: String::new(),
            input_cursor: 0,
            visible: true,
        }
    }

    /// Reload the tree from a flat list of entries (fetched from DB by the caller).
    pub fn reload_from_entries(&mut self, entries: &[T]) {
        self.roots = build_tree(entries, None);
        sort_tree(&mut self.roots);
        self.rebuild_flat_view();
    }

    /// Rebuild the flat_view from the current tree state, preserving selection if possible.
    pub fn rebuild_flat_view(&mut self) {
        let old_id = self.selected_entry_id();
        self.flat_view.clear();
        flatten_tree(&self.roots, 0, &[], &mut self.flat_view);

        // Try to restore selection by entry ID
        if let Some(id) = old_id {
            if let Some(pos) = self.flat_view.iter().position(|e| e.entry_id == id) {
                self.selected = pos;
                return;
            }
        }

        // Clamp selection: allow selected == flat_view.len() (the blank root line)
        if self.selected > self.max_selectable() {
            self.selected = self.max_selectable();
        }
    }

    /// The maximum selectable index: flat_view.len() is the blank root line.
    fn max_selectable(&self) -> usize {
        self.flat_view.len()
    }

    /// Whether the cursor is on the blank root line (one past last entry).
    pub fn is_on_blank_line(&self) -> bool {
        self.selected == self.flat_view.len() && !self.flat_view.is_empty()
    }

    /// Get the currently selected flat entry, if any.
    pub fn selected_entry(&self) -> Option<&FlatEntry> {
        self.flat_view.get(self.selected)
    }

    /// Get the entry ID of the currently selected entry.
    pub fn selected_entry_id(&self) -> Option<i64> {
        self.selected_entry().map(|e| e.entry_id)
    }

    /// Move selection down. Can go one past last entry (blank root line).
    pub fn move_down(&mut self) {
        if self.selected < self.max_selectable() {
            self.selected += 1;
        }
    }

    /// Move selection up.
    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Go to the top of the list.
    pub fn goto_top(&mut self) {
        self.selected = 0;
    }

    /// Go to the bottom of the list (the blank root line).
    pub fn goto_bottom(&mut self) {
        self.selected = self.max_selectable();
    }

    /// Half-page down.
    pub fn half_page_down(&mut self, visible_lines: usize) {
        let half = visible_lines / 2;
        self.selected = (self.selected + half).min(self.max_selectable());
    }

    /// Half-page up.
    pub fn half_page_up(&mut self, visible_lines: usize) {
        let half = visible_lines / 2;
        self.selected = self.selected.saturating_sub(half);
    }

    /// Toggle expansion of the selected folder.
    /// Returns the entry_id if the entry was a folder that was toggled, None otherwise.
    /// The caller is responsible for persisting the expansion state to DB.
    pub fn toggle_expand(&mut self) -> Option<(i64, bool)> {
        if let Some(entry) = self.selected_entry() {
            if entry.is_folder {
                let entry_id = entry.entry_id;
                if let Some(node) = find_node_mut(&mut self.roots, entry_id) {
                    node.expanded = !node.expanded;
                    let new_state = node.expanded;
                    self.rebuild_flat_view();
                    return Some((entry_id, new_state));
                }
            }
        }
        None
    }

    /// Expand the selected folder (no-op if already expanded or not a folder).
    /// Returns the entry_id if expanded, None otherwise.
    pub fn expand_selected(&mut self) -> Option<(i64, bool)> {
        if let Some(entry) = self.selected_entry() {
            if entry.is_folder && !entry.is_expanded {
                let entry_id = entry.entry_id;
                if let Some(node) = find_node_mut(&mut self.roots, entry_id) {
                    node.expanded = true;
                    self.rebuild_flat_view();
                    return Some((entry_id, true));
                }
            }
        }
        None
    }

    /// Collapse the selected folder, or move to parent if already collapsed or a leaf.
    /// Returns Some(entry_id, false) if collapsed, None if navigated to parent.
    pub fn collapse_or_parent(&mut self) -> Option<(i64, bool)> {
        if let Some(entry) = self.selected_entry() {
            let entry_id = entry.entry_id;

            // If it's an expanded folder, collapse it
            if entry.is_folder && entry.is_expanded {
                if let Some(node) = find_node_mut(&mut self.roots, entry_id) {
                    node.expanded = false;
                    self.rebuild_flat_view();
                    return Some((entry_id, false));
                }
            }

            // Otherwise, move to parent
            let parent_id = find_parent_id(&self.roots, entry_id);
            if let Some(pid) = parent_id {
                if let Some(pos) = self.flat_view.iter().position(|e| e.entry_id == pid) {
                    self.selected = pos;
                }
            }
        }
        None
    }

    /// Start the "add entry" input mode.
    pub fn start_add(&mut self) {
        self.input_mode = SidebarInput::Adding;
        self.input_buffer.clear();
        self.input_cursor = 0;
    }

    /// Start the "rename entry" input mode, pre-filled with current name.
    pub fn start_rename(&mut self) {
        let name = self.selected_entry().map(|e| e.name.clone());
        if let Some(name) = name {
            self.input_mode = SidebarInput::Renaming;
            self.input_cursor = name.len();
            self.input_buffer = name;
        }
    }

    /// Start the delete confirmation.
    pub fn start_delete(&mut self) {
        if self.selected_entry().is_some() {
            self.input_mode = SidebarInput::ConfirmDelete;
            self.input_buffer.clear();
            self.input_cursor = 0;
        }
    }

    /// Cancel any active input.
    pub fn cancel_input(&mut self) {
        self.input_mode = SidebarInput::None;
        self.input_buffer.clear();
        self.input_cursor = 0;
    }

    /// Copy the selected entry to clipboard.
    pub fn copy_selected(&mut self) {
        if let Some(entry) = self.selected_entry() {
            self.clipboard = Some(ClipboardItem {
                entry_id: entry.entry_id,
                mode: ClipboardMode::Copy,
            });
        }
    }

    /// Cut the selected entry to clipboard.
    pub fn cut_selected(&mut self) {
        if let Some(entry) = self.selected_entry() {
            self.clipboard = Some(ClipboardItem {
                entry_id: entry.entry_id,
                mode: ClipboardMode::Cut,
            });
        }
    }

    /// Get the parent_id for pasting: if selected entry is a folder, paste inside it;
    /// otherwise paste in the same parent as the selected entry.
    pub fn paste_target_parent_id(&self) -> Option<i64> {
        if let Some(entry) = self.selected_entry() {
            if entry.is_folder {
                Some(entry.entry_id)
            } else {
                // Find the parent of the selected entry
                find_parent_id(&self.roots, entry.entry_id)
            }
        } else {
            None // Root level
        }
    }

    /// Insert a character into the input buffer at the cursor position.
    pub fn input_insert_char(&mut self, c: char) {
        self.input_buffer.insert(self.input_cursor, c);
        self.input_cursor += c.len_utf8();
    }

    /// Delete the character before the cursor in the input buffer.
    pub fn input_backspace(&mut self) {
        if self.input_cursor > 0 {
            let prev = self.input_buffer[..self.input_cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.input_buffer.drain(prev..self.input_cursor);
            self.input_cursor = prev;
        }
    }

    /// Move cursor left in the input buffer.
    pub fn input_cursor_left(&mut self) {
        if self.input_cursor > 0 {
            self.input_cursor = self.input_buffer[..self.input_cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
    }

    /// Move cursor right in the input buffer.
    pub fn input_cursor_right(&mut self) {
        if self.input_cursor < self.input_buffer.len() {
            self.input_cursor = self.input_buffer[self.input_cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.input_cursor + i)
                .unwrap_or(self.input_buffer.len());
        }
    }

    /// Expand the chain of parents leading to a specific entry, so that it
    /// becomes visible in the flat view. Returns a list of (id, expanded)
    /// pairs that changed so the caller can persist them.
    pub fn expand_to_entry(&mut self, target_id: i64) -> Vec<(i64, bool)> {
        let mut changed = Vec::new();
        let ancestors = collect_ancestors(&self.roots, target_id);
        for ancestor_id in ancestors {
            if let Some(node) = find_node_mut(&mut self.roots, ancestor_id) {
                if !node.expanded {
                    node.expanded = true;
                    changed.push((ancestor_id, true));
                }
            }
        }
        if !changed.is_empty() {
            self.rebuild_flat_view();
        }
        changed
    }

    /// Select the entry with the given ID, if it's visible.
    pub fn select_entry(&mut self, entry_id: i64) {
        if let Some(pos) = self.flat_view.iter().position(|e| e.entry_id == entry_id) {
            self.selected = pos;
        }
    }
}

// ── Tree building functions ──────────────────────────────────────────

/// Build a tree from a flat list of entries, starting from entries with the given parent_id.
fn build_tree<T: TreeEntry>(entries: &[T], parent_id: Option<i64>) -> Vec<TreeNode<T>> {
    entries
        .iter()
        .filter(|e| e.parent_id() == parent_id)
        .map(|e| {
            let children = build_tree(entries, Some(e.id()));
            TreeNode {
                expanded: e.is_expanded(),
                entry: e.clone(),
                children,
            }
        })
        .collect()
}

/// Sort tree nodes: folders first, then leaves, both alphabetically. Recursive.
fn sort_tree<T: TreeEntry>(nodes: &mut Vec<TreeNode<T>>) {
    nodes.sort_by(|a, b| {
        let type_ord = match (a.entry.is_folder(), b.entry.is_folder()) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        };
        type_ord.then_with(|| {
            a.entry
                .name()
                .to_lowercase()
                .cmp(&b.entry.name().to_lowercase())
        })
    });
    for node in nodes.iter_mut() {
        sort_tree(&mut node.children);
    }
}

/// Flatten visible tree nodes into a list for rendering.
fn flatten_tree<T: TreeEntry>(
    nodes: &[TreeNode<T>],
    depth: usize,
    parent_guides: &[bool],
    out: &mut Vec<FlatEntry>,
) {
    for node in nodes.iter() {
        let guide_depths = parent_guides.to_vec();

        out.push(FlatEntry {
            entry_id: node.entry.id(),
            depth,
            name: node.entry.name().to_string(),
            is_folder: node.entry.is_folder(),
            is_expanded: node.expanded,
            has_children: !node.children.is_empty(),
            guide_depths,
        });

        if node.expanded && !node.children.is_empty() {
            let mut child_guides = parent_guides.to_vec();
            child_guides.push(true);
            flatten_tree(&node.children, depth + 1, &child_guides, out);
        }
    }
}

// ── Tree traversal functions ─────────────────────────────────────────

/// Find an immutable reference to a node by entry ID.
pub fn find_node<T: TreeEntry>(nodes: &[TreeNode<T>], id: i64) -> Option<&TreeNode<T>> {
    for node in nodes {
        if node.entry.id() == id {
            return Some(node);
        }
        if let Some(found) = find_node(&node.children, id) {
            return Some(found);
        }
    }
    None
}

/// Find a mutable reference to a node by entry ID.
pub fn find_node_mut<T: TreeEntry>(
    nodes: &mut Vec<TreeNode<T>>,
    id: i64,
) -> Option<&mut TreeNode<T>> {
    for node in nodes.iter_mut() {
        if node.entry.id() == id {
            return Some(node);
        }
        if let Some(found) = find_node_mut(&mut node.children, id) {
            return Some(found);
        }
    }
    None
}

/// Find the parent ID of an entry by searching the tree.
pub fn find_parent_id<T: TreeEntry>(nodes: &[TreeNode<T>], target_id: i64) -> Option<i64> {
    for node in nodes {
        for child in &node.children {
            if child.entry.id() == target_id {
                return Some(node.entry.id());
            }
        }
        if let Some(found) = find_parent_id(&node.children, target_id) {
            return Some(found);
        }
    }
    None
}

/// Collect ancestor IDs from root down to (but not including) the target entry.
fn collect_ancestors<T: TreeEntry>(nodes: &[TreeNode<T>], target_id: i64) -> Vec<i64> {
    fn find_path<T: TreeEntry>(nodes: &[TreeNode<T>], target_id: i64, path: &mut Vec<i64>) -> bool {
        for node in nodes {
            if node.entry.id() == target_id {
                return true;
            }
            path.push(node.entry.id());
            if find_path(&node.children, target_id, path) {
                return true;
            }
            path.pop();
        }
        false
    }
    let mut path = Vec::new();
    find_path(nodes, target_id, &mut path);
    path
}

// ── Rendering ────────────────────────────────────────────────────────

const GUIDE_STYLE: Style = Style::new().fg(Color::DarkGray);
const SELECTED_BG: Color = Color::Gray;

/// Configuration for rendering a tree sidebar.
pub struct TreeSidebarRenderConfig<'a> {
    /// The title to display in the sidebar border.
    pub title: &'a str,
    /// Whether the sidebar is focused.
    pub focused: bool,
    /// The icon to use for leaf entries (non-folder items).
    /// Defaults to "● " if None.
    pub leaf_icon: Option<&'a str>,
    /// Style for leaf entries when not selected. Default: White.
    pub leaf_style: Option<Style>,
    /// Style for folder entries when not selected. Default: Blue.
    pub folder_style: Option<Style>,
}

/// Render the tree sidebar into the given area.
pub fn render_tree_sidebar<T: TreeEntry>(
    frame: &mut Frame,
    area: Rect,
    sidebar: &TreeSidebar<T>,
    config: &TreeSidebarRenderConfig<'_>,
) {
    let border_color = if config.focused {
        Color::Blue
    } else {
        Color::DarkGray
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(config.title);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let (tree_area, input_area) = if sidebar.input_mode != SidebarInput::None {
        let input_height = 1;
        if inner.height <= input_height {
            (inner, None)
        } else {
            let tree = Rect {
                height: inner.height - input_height,
                ..inner
            };
            let input = Rect {
                y: inner.y + inner.height - input_height,
                height: input_height,
                ..inner
            };
            (tree, Some(input))
        }
    } else {
        (inner, None)
    };

    render_tree_entries(frame, tree_area, sidebar, config);

    if let Some(input_area) = input_area {
        render_input_prompt(frame, input_area, sidebar);
    }
}

fn render_tree_entries<T: TreeEntry>(
    frame: &mut Frame,
    area: Rect,
    sidebar: &TreeSidebar<T>,
    config: &TreeSidebarRenderConfig<'_>,
) {
    if sidebar.flat_view.is_empty() {
        if sidebar.selected == 0 {
            let highlight = Style::default().bg(SELECTED_BG);
            let blank = Line::from(Span::styled(" ".repeat(area.width as usize), highlight));
            let widget = Paragraph::new(vec![blank]);
            frame.render_widget(widget, area);
        } else {
            let empty = Paragraph::new("  No entries yet. Press 'a' to add.")
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(empty, area);
        }
        return;
    }

    let total_items = sidebar.flat_view.len() + 1;
    let visible_lines = area.height as usize;

    let scroll_offset = if sidebar.selected >= visible_lines {
        sidebar.selected - visible_lines + 1
    } else {
        0
    };

    let mut lines: Vec<Line> = Vec::new();

    for item_idx in scroll_offset..total_items.min(scroll_offset + visible_lines) {
        if item_idx < sidebar.flat_view.len() {
            let entry = &sidebar.flat_view[item_idx];
            lines.push(render_entry_line(
                entry, item_idx, sidebar, area.width, config,
            ));
        } else {
            let is_selected = item_idx == sidebar.selected;
            if is_selected {
                let highlight = Style::default().bg(SELECTED_BG);
                lines.push(Line::from(Span::styled(
                    " ".repeat(area.width as usize),
                    highlight,
                )));
            } else {
                lines.push(Line::from(""));
            }
        }
    }

    let tree_widget = Paragraph::new(lines);
    frame.render_widget(tree_widget, area);
}

fn render_entry_line<T: TreeEntry>(
    entry: &FlatEntry,
    idx: usize,
    sidebar: &TreeSidebar<T>,
    area_width: u16,
    config: &TreeSidebarRenderConfig<'_>,
) -> Line<'static> {
    let is_selected = idx == sidebar.selected;
    let is_cut = sidebar
        .clipboard
        .as_ref()
        .map(|c| c.entry_id == entry.entry_id && c.mode == ClipboardMode::Cut)
        .unwrap_or(false);

    let folder_style = config
        .folder_style
        .unwrap_or(Style::default().fg(Color::Blue));
    let leaf_style = config
        .leaf_style
        .unwrap_or(Style::default().fg(Color::White));

    let base_style = if is_selected {
        Style::default()
            .bg(SELECTED_BG)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else if is_cut {
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::DIM)
    } else if entry.is_folder {
        folder_style
    } else {
        leaf_style
    };

    let mut spans: Vec<Span<'static>> = Vec::new();

    for d in 0..entry.depth {
        let has_guide = entry.guide_depths.get(d).copied().unwrap_or(false);
        if has_guide {
            let guide_style = if is_selected {
                GUIDE_STYLE.bg(SELECTED_BG)
            } else {
                GUIDE_STYLE
            };
            spans.push(Span::styled("\u{2502} ", guide_style));
        } else {
            spans.push(Span::styled("  ", base_style));
        }
    }

    let icon: &str = if entry.is_folder {
        if entry.is_expanded {
            "\u{25BC} "
        } else {
            "\u{25B6} "
        }
    } else {
        config.leaf_icon.unwrap_or("\u{25CF} ")
    };
    spans.push(Span::styled(icon.to_string(), base_style));
    spans.push(Span::styled(entry.name.clone(), base_style));

    if is_selected {
        let content_width: usize = spans.iter().map(|s| s.content.width()).sum();
        let remaining = (area_width as usize).saturating_sub(content_width);
        if remaining > 0 {
            spans.push(Span::styled(
                " ".repeat(remaining),
                Style::default().bg(SELECTED_BG),
            ));
        }
    }

    Line::from(spans)
}

fn render_input_prompt<T: TreeEntry>(frame: &mut Frame, area: Rect, sidebar: &TreeSidebar<T>) {
    let (label, input_text) = match &sidebar.input_mode {
        SidebarInput::Adding => ("New: ", &sidebar.input_buffer),
        SidebarInput::Renaming => ("Name: ", &sidebar.input_buffer),
        SidebarInput::ConfirmDelete => {
            let name = sidebar
                .selected_entry()
                .map(|e| e.name.as_str())
                .unwrap_or("?");
            let prompt = format!("Delete {}? (y/n)", name);
            let line = Line::from(vec![Span::styled(
                prompt,
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )]);
            let widget = Paragraph::new(line);
            frame.render_widget(widget, area);
            return;
        }
        SidebarInput::None => return,
    };

    let line = Line::from(vec![
        Span::styled(
            label,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(input_text.clone()),
    ]);

    let widget = Paragraph::new(line);
    frame.render_widget(widget, area);

    let cursor_x = area.x + label.len() as u16 + sidebar.input_cursor as u16;
    let cursor_y = area.y;
    if cursor_x < area.x + area.width {
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Simple test entry implementing TreeEntry.
    #[derive(Debug, Clone)]
    struct TestEntry {
        id: i64,
        parent_id: Option<i64>,
        name: String,
        folder: bool,
        expanded: bool,
    }

    impl TreeEntry for TestEntry {
        fn id(&self) -> i64 {
            self.id
        }
        fn parent_id(&self) -> Option<i64> {
            self.parent_id
        }
        fn name(&self) -> &str {
            &self.name
        }
        fn is_folder(&self) -> bool {
            self.folder
        }
        fn is_expanded(&self) -> bool {
            self.expanded
        }
    }

    fn entry(
        id: i64,
        parent_id: Option<i64>,
        name: &str,
        folder: bool,
        expanded: bool,
    ) -> TestEntry {
        TestEntry {
            id,
            parent_id,
            name: name.to_string(),
            folder,
            expanded,
        }
    }

    #[test]
    fn test_empty_tree() {
        let mut sidebar: TreeSidebar<TestEntry> = TreeSidebar::new();
        sidebar.reload_from_entries(&[]);
        assert!(sidebar.flat_view.is_empty());
        assert!(sidebar.roots.is_empty());
    }

    #[test]
    fn test_flat_entries() {
        let mut sidebar: TreeSidebar<TestEntry> = TreeSidebar::new();
        let entries = vec![
            entry(1, None, "folder-a", true, false),
            entry(2, None, "query-b", false, false),
        ];
        sidebar.reload_from_entries(&entries);

        assert_eq!(sidebar.flat_view.len(), 2);
        assert_eq!(sidebar.flat_view[0].name, "folder-a");
        assert_eq!(sidebar.flat_view[1].name, "query-b");
    }

    #[test]
    fn test_expand_collapse() {
        let mut sidebar: TreeSidebar<TestEntry> = TreeSidebar::new();
        let entries = vec![
            entry(1, None, "api", true, false),
            entry(2, Some(1), "get-users", false, false),
        ];
        sidebar.reload_from_entries(&entries);

        // Initially collapsed — only the folder is visible
        assert_eq!(sidebar.flat_view.len(), 1);

        // Expand
        let result = sidebar.toggle_expand();
        assert!(result.is_some());
        assert_eq!(sidebar.flat_view.len(), 2);
        assert_eq!(sidebar.flat_view[1].name, "get-users");

        // Collapse
        let result = sidebar.toggle_expand();
        assert!(result.is_some());
        assert_eq!(sidebar.flat_view.len(), 1);
    }

    #[test]
    fn test_navigation() {
        let mut sidebar: TreeSidebar<TestEntry> = TreeSidebar::new();
        let entries = vec![
            entry(1, None, "a", true, false),
            entry(2, None, "b", true, false),
            entry(3, None, "c", false, false),
        ];
        sidebar.reload_from_entries(&entries);

        assert_eq!(sidebar.selected, 0);
        sidebar.move_down();
        assert_eq!(sidebar.selected, 1);
        sidebar.move_down();
        assert_eq!(sidebar.selected, 2);
        sidebar.move_down(); // moves to blank root line
        assert_eq!(sidebar.selected, 3);
        assert!(sidebar.is_on_blank_line());
        assert!(sidebar.selected_entry().is_none());
        sidebar.move_down(); // should not go past blank line
        assert_eq!(sidebar.selected, 3);

        sidebar.goto_top();
        assert_eq!(sidebar.selected, 0);

        sidebar.goto_bottom(); // goes to blank root line
        assert_eq!(sidebar.selected, 3);
        assert!(sidebar.is_on_blank_line());

        sidebar.move_up();
        assert_eq!(sidebar.selected, 2);
        assert!(!sidebar.is_on_blank_line());
        assert!(sidebar.selected_entry().is_some());
    }

    #[test]
    fn test_sort_folders_first() {
        let mut sidebar: TreeSidebar<TestEntry> = TreeSidebar::new();
        let entries = vec![
            entry(1, None, "z-query", false, false),
            entry(2, None, "a-folder", true, false),
            entry(3, None, "b-query", false, false),
            entry(4, None, "c-folder", true, false),
        ];
        sidebar.reload_from_entries(&entries);

        assert_eq!(sidebar.flat_view[0].name, "a-folder");
        assert!(sidebar.flat_view[0].is_folder);
        assert_eq!(sidebar.flat_view[1].name, "c-folder");
        assert!(sidebar.flat_view[1].is_folder);
        assert_eq!(sidebar.flat_view[2].name, "b-query");
        assert!(!sidebar.flat_view[2].is_folder);
        assert_eq!(sidebar.flat_view[3].name, "z-query");
        assert!(!sidebar.flat_view[3].is_folder);
    }

    #[test]
    fn test_input_buffer_operations() {
        let mut sidebar: TreeSidebar<TestEntry> = TreeSidebar::new();

        sidebar.input_insert_char('h');
        sidebar.input_insert_char('e');
        sidebar.input_insert_char('l');
        sidebar.input_insert_char('l');
        sidebar.input_insert_char('o');
        assert_eq!(sidebar.input_buffer, "hello");
        assert_eq!(sidebar.input_cursor, 5);

        sidebar.input_backspace();
        assert_eq!(sidebar.input_buffer, "hell");
        assert_eq!(sidebar.input_cursor, 4);

        sidebar.input_cursor_left();
        assert_eq!(sidebar.input_cursor, 3);

        sidebar.input_insert_char('X');
        assert_eq!(sidebar.input_buffer, "helXl");
        assert_eq!(sidebar.input_cursor, 4);

        sidebar.input_cursor_right();
        assert_eq!(sidebar.input_cursor, 5);
    }

    #[test]
    fn test_guide_depths() {
        let mut sidebar: TreeSidebar<TestEntry> = TreeSidebar::new();
        let entries = vec![
            entry(1, None, "a-folder", true, true),
            entry(2, Some(1), "sub", true, true),
            entry(3, Some(2), "query-1", false, false),
            entry(4, Some(2), "query-2", false, false),
            entry(5, Some(1), "query-3", false, false),
            entry(6, None, "b-query", false, false),
        ];
        sidebar.reload_from_entries(&entries);

        assert_eq!(sidebar.flat_view.len(), 6);
        assert_eq!(sidebar.flat_view[0].guide_depths, Vec::<bool>::new());
        assert_eq!(sidebar.flat_view[1].guide_depths, vec![true]);
        assert_eq!(sidebar.flat_view[2].guide_depths, vec![true, true]);
        assert_eq!(sidebar.flat_view[3].guide_depths, vec![true, true]);
        assert_eq!(sidebar.flat_view[4].guide_depths, vec![true]);
        assert_eq!(sidebar.flat_view[5].guide_depths, Vec::<bool>::new());
    }

    #[test]
    fn test_blank_line_empty_tree() {
        let mut sidebar: TreeSidebar<TestEntry> = TreeSidebar::new();
        sidebar.reload_from_entries(&[]);

        assert_eq!(sidebar.selected, 0);
        assert!(sidebar.flat_view.is_empty());
        assert!(sidebar.selected_entry().is_none());
        assert!(!sidebar.is_on_blank_line());
    }

    #[test]
    fn test_blank_line_with_entries() {
        let mut sidebar: TreeSidebar<TestEntry> = TreeSidebar::new();
        let entries = vec![entry(1, None, "item", false, false)];
        sidebar.reload_from_entries(&entries);

        assert_eq!(sidebar.flat_view.len(), 1);

        sidebar.move_down();
        assert_eq!(sidebar.selected, 1);
        assert!(sidebar.is_on_blank_line());
        assert!(sidebar.selected_entry().is_none());

        sidebar.move_up();
        assert_eq!(sidebar.selected, 0);
        assert!(!sidebar.is_on_blank_line());
        assert!(sidebar.selected_entry().is_some());
    }

    #[test]
    fn test_clipboard_operations() {
        let mut sidebar: TreeSidebar<TestEntry> = TreeSidebar::new();
        let entries = vec![
            entry(1, None, "item-a", false, false),
            entry(2, None, "item-b", false, false),
        ];
        sidebar.reload_from_entries(&entries);

        // Copy
        sidebar.copy_selected();
        assert!(sidebar.clipboard.is_some());
        assert_eq!(sidebar.clipboard.as_ref().unwrap().entry_id, 1);
        assert_eq!(
            sidebar.clipboard.as_ref().unwrap().mode,
            ClipboardMode::Copy
        );

        // Cut
        sidebar.move_down();
        sidebar.cut_selected();
        assert_eq!(sidebar.clipboard.as_ref().unwrap().entry_id, 2);
        assert_eq!(sidebar.clipboard.as_ref().unwrap().mode, ClipboardMode::Cut);
    }

    #[test]
    fn test_expand_to_entry() {
        let mut sidebar: TreeSidebar<TestEntry> = TreeSidebar::new();
        let entries = vec![
            entry(1, None, "root", true, false),
            entry(2, Some(1), "child", true, false),
            entry(3, Some(2), "grandchild", false, false),
        ];
        sidebar.reload_from_entries(&entries);

        // Only root is visible (collapsed)
        assert_eq!(sidebar.flat_view.len(), 1);

        // Expand to grandchild
        let changed = sidebar.expand_to_entry(3);
        assert_eq!(changed.len(), 2); // root and child were expanded
        assert_eq!(sidebar.flat_view.len(), 3);
        assert_eq!(sidebar.flat_view[2].name, "grandchild");
    }
}
