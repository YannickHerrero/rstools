//! In-memory representation of an opened KeePass vault.
//!
//! Converts the keepass crate's `Database` into our own tree structure
//! for navigation and rendering. The Recycle Bin group is hidden.

use std::path::Path;

use keepass::{Database, DatabaseKey};
use zeroize::Zeroize;

// ── Tree node types ──────────────────────────────────────────────────

/// The type of a node in the vault tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    Group,
    Entry,
}

/// A single entry's details extracted from the KeePass database.
#[derive(Debug, Clone)]
pub struct EntryDetails {
    pub title: String,
    pub username: String,
    pub password: String,
    pub url: String,
    pub notes: String,
    pub tags: Vec<String>,
    /// Custom string fields (key, value, is_protected).
    pub custom_fields: Vec<(String, String, bool)>,
}

impl Default for EntryDetails {
    fn default() -> Self {
        Self {
            title: String::new(),
            username: String::new(),
            password: String::new(),
            url: String::new(),
            notes: String::new(),
            tags: Vec::new(),
            custom_fields: Vec::new(),
        }
    }
}

/// A node in our in-memory vault tree.
#[derive(Debug, Clone)]
pub struct VaultNode {
    /// Display name (group name or entry title).
    pub name: String,
    /// Whether this is a group or entry.
    pub node_type: NodeType,
    /// Children (only for groups).
    pub children: Vec<VaultNode>,
    /// Entry details (only for entries).
    pub details: Option<EntryDetails>,
    /// Whether this group is expanded in the tree view.
    pub expanded: bool,
}

/// A flattened entry for rendering — one visible line in the tree.
#[derive(Debug, Clone)]
pub struct FlatNode {
    /// Index path into the VaultNode tree (e.g., [0, 2, 1] means root.children[0].children[2].children[1]).
    pub path: Vec<usize>,
    /// Display name.
    pub name: String,
    /// Node type.
    pub node_type: NodeType,
    /// Depth in tree (for indentation).
    pub depth: usize,
    /// Whether this node is expanded (groups only).
    pub is_expanded: bool,
    /// Whether this node has children.
    pub has_children: bool,
    /// For each depth level, whether a vertical guide line should be drawn.
    pub guide_depths: Vec<bool>,
}

/// The full vault state for an opened KeePass database.
pub struct VaultState {
    /// Root nodes of the vault tree.
    pub roots: Vec<VaultNode>,
    /// Flattened visible nodes for rendering and navigation.
    pub flat_view: Vec<FlatNode>,
    /// Currently selected index into flat_view.
    pub selected: usize,
    /// Path of the opened .kdbx file.
    pub file_path: String,
    /// Display name of the vault.
    pub vault_name: String,
}

impl VaultState {
    /// Open and parse a KeePass database file.
    pub fn open(file_path: &str, password: &str) -> anyhow::Result<Self> {
        let path = Path::new(file_path);
        if !path.exists() {
            anyhow::bail!("File not found: {file_path}");
        }

        let mut password_owned = password.to_string();
        let db_key = DatabaseKey::new().with_password(&password_owned);
        let db = Database::open(&mut std::fs::File::open(path)?, db_key)?;
        password_owned.zeroize();

        // Get the recycle bin UUID to filter it out.
        // We store it as a 128-bit value to avoid needing the uuid crate as a dependency.
        let recycle_bin_u128 = db.meta.recyclebin_uuid.map(|u| u.as_u128());

        // Convert the database tree to our VaultNode structure
        let vault_name = db.meta.database_name.clone().unwrap_or_else(|| {
            path.file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "Vault".to_string())
        });

        let roots = convert_group(&db.root, recycle_bin_u128);

        let mut state = Self {
            roots,
            flat_view: Vec::new(),
            selected: 0,
            file_path: file_path.to_string(),
            vault_name,
        };
        state.rebuild_flat_view();
        Ok(state)
    }

    /// Rebuild the flat view from the current tree state.
    pub fn rebuild_flat_view(&mut self) {
        let old_path = self.selected_path();
        self.flat_view.clear();
        flatten_nodes(&self.roots, 0, &[], &mut self.flat_view, &[]);

        // Try to restore selection by path
        if let Some(old) = old_path {
            if let Some(idx) = self.flat_view.iter().position(|n| n.path == old) {
                self.selected = idx;
                return;
            }
        }
        // Keep selection in bounds
        if !self.flat_view.is_empty() && self.selected >= self.flat_view.len() {
            self.selected = self.flat_view.len() - 1;
        }
    }

    /// Get the path of the currently selected node.
    fn selected_path(&self) -> Option<Vec<usize>> {
        self.flat_view.get(self.selected).map(|n| n.path.clone())
    }

    /// Get the currently selected flat node.
    pub fn selected_node(&self) -> Option<&FlatNode> {
        self.flat_view.get(self.selected)
    }

    /// Get the VaultNode at a given path.
    pub fn node_at_path(&self, path: &[usize]) -> Option<&VaultNode> {
        if path.is_empty() {
            return None;
        }
        let mut current = self.roots.get(path[0])?;
        for &idx in &path[1..] {
            current = current.children.get(idx)?;
        }
        Some(current)
    }

    /// Get the entry details for the selected node.
    pub fn selected_details(&self) -> Option<&EntryDetails> {
        let flat = self.selected_node()?;
        let node = self.node_at_path(&flat.path)?;
        node.details.as_ref()
    }

    /// Toggle expand/collapse for the selected node.
    pub fn toggle_expand(&mut self) {
        if let Some(flat) = self.flat_view.get(self.selected) {
            if flat.node_type == NodeType::Group {
                let path = flat.path.clone();
                if let Some(node) = self.node_at_path_mut(&path) {
                    node.expanded = !node.expanded;
                }
                self.rebuild_flat_view();
            }
        }
    }

    /// Expand the selected group.
    pub fn expand_selected(&mut self) {
        if let Some(flat) = self.flat_view.get(self.selected) {
            if flat.node_type == NodeType::Group && !flat.is_expanded {
                let path = flat.path.clone();
                if let Some(node) = self.node_at_path_mut(&path) {
                    node.expanded = true;
                }
                self.rebuild_flat_view();
            }
        }
    }

    /// Collapse the selected group, or move to parent if already collapsed / is an entry.
    pub fn collapse_or_parent(&mut self) {
        if let Some(flat) = self.flat_view.get(self.selected) {
            let path = flat.path.clone();
            if flat.node_type == NodeType::Group && flat.is_expanded {
                // Collapse this group
                if let Some(node) = self.node_at_path_mut(&path) {
                    node.expanded = false;
                }
                self.rebuild_flat_view();
            } else if path.len() > 1 {
                // Move to parent
                let parent_path: Vec<usize> = path[..path.len() - 1].to_vec();
                if let Some(idx) = self.flat_view.iter().position(|n| n.path == parent_path) {
                    self.selected = idx;
                }
            }
        }
    }

    /// Get a mutable reference to the node at a given path (public version).
    pub fn node_at_path_mut_public(&mut self, path: &[usize]) -> Option<&mut VaultNode> {
        self.node_at_path_mut(path)
    }

    /// Get a mutable reference to the node at a given path.
    fn node_at_path_mut(&mut self, path: &[usize]) -> Option<&mut VaultNode> {
        if path.is_empty() {
            return None;
        }
        let mut current = self.roots.get_mut(path[0])?;
        for &idx in &path[1..] {
            current = current.children.get_mut(idx)?;
        }
        Some(current)
    }

    // ── Navigation ───────────────────────────────────────────────────

    pub fn move_down(&mut self) {
        if !self.flat_view.is_empty() && self.selected < self.flat_view.len() - 1 {
            self.selected += 1;
        }
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn goto_top(&mut self) {
        self.selected = 0;
    }

    pub fn goto_bottom(&mut self) {
        if !self.flat_view.is_empty() {
            self.selected = self.flat_view.len() - 1;
        }
    }

    pub fn half_page_down(&mut self, visible_lines: usize) {
        let half = visible_lines / 2;
        self.selected = (self.selected + half).min(self.flat_view.len().saturating_sub(1));
    }

    pub fn half_page_up(&mut self, visible_lines: usize) {
        let half = visible_lines / 2;
        self.selected = self.selected.saturating_sub(half);
    }

    /// Collect all entry titles for telescope search.
    pub fn collect_searchable_entries(&self) -> Vec<SearchableEntry> {
        let mut entries = Vec::new();
        collect_entries_recursive(&self.roots, &[], &mut entries);
        entries
    }
}

/// A searchable entry for telescope integration.
#[derive(Debug, Clone)]
pub struct SearchableEntry {
    /// The entry title.
    pub title: String,
    /// The full group path (e.g., "Internet/Email").
    pub group_path: String,
    /// The username, if available.
    pub username: String,
    /// Path into the tree for navigation.
    pub tree_path: Vec<usize>,
}

fn collect_entries_recursive(
    nodes: &[VaultNode],
    current_path: &[usize],
    out: &mut Vec<SearchableEntry>,
) {
    for (i, node) in nodes.iter().enumerate() {
        let mut path = current_path.to_vec();
        path.push(i);

        match node.node_type {
            NodeType::Entry => {
                if let Some(ref details) = node.details {
                    out.push(SearchableEntry {
                        title: details.title.clone(),
                        group_path: String::new(), // Will be set by caller context
                        username: details.username.clone(),
                        tree_path: path,
                    });
                }
            }
            NodeType::Group => {
                collect_entries_recursive(&node.children, &path, out);
                // Set group_path for children we just added
                let group_name = &node.name;
                for entry in out.iter_mut().rev() {
                    if entry.tree_path.starts_with(&path) && entry.group_path.is_empty() {
                        entry.group_path = group_name.clone();
                    } else if entry.tree_path.starts_with(&path) {
                        entry.group_path = format!("{}/{}", group_name, entry.group_path);
                    } else {
                        break;
                    }
                }
            }
        }
    }
}

// ── Conversion from keepass types ────────────────────────────────────

/// Convert a keepass Group into our VaultNode children.
/// Filters out the Recycle Bin group.
fn convert_group(group: &keepass::db::Group, recycle_bin_u128: Option<u128>) -> Vec<VaultNode> {
    let mut nodes = Vec::new();

    for node_ref in &group.children {
        match node_ref {
            keepass::db::Node::Group(child_group) => {
                // Skip the Recycle Bin
                if let Some(rb) = recycle_bin_u128 {
                    if child_group.uuid.as_u128() == rb {
                        continue;
                    }
                }

                let children = convert_group(child_group, recycle_bin_u128);
                nodes.push(VaultNode {
                    name: child_group.name.clone(),
                    node_type: NodeType::Group,
                    children,
                    details: None,
                    expanded: false,
                });
            }
            keepass::db::Node::Entry(entry) => {
                let details = extract_entry_details(entry);
                let name = details.title.clone();
                nodes.push(VaultNode {
                    name: if name.is_empty() {
                        "(untitled)".to_string()
                    } else {
                        name
                    },
                    node_type: NodeType::Entry,
                    children: Vec::new(),
                    details: Some(details),
                    expanded: false,
                });
            }
        }
    }

    // Sort: groups first, then entries, alphabetically within each category
    nodes.sort_by(|a, b| {
        let type_ord = match (&a.node_type, &b.node_type) {
            (NodeType::Group, NodeType::Entry) => std::cmp::Ordering::Less,
            (NodeType::Entry, NodeType::Group) => std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        };
        type_ord.then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    nodes
}

/// Extract entry details from a keepass Entry.
fn extract_entry_details(entry: &keepass::db::Entry) -> EntryDetails {
    // Entry::get() returns Option<&str> — it auto-unprotects protected values
    let get_str = |key: &str| -> String { entry.get(key).unwrap_or("").to_string() };

    let title = get_str("Title");
    let username = get_str("UserName");
    let password = get_str("Password");
    let url = get_str("URL");
    let notes = get_str("Notes");

    // Extract tags
    let tags: Vec<String> = entry
        .tags
        .iter()
        .filter(|t| !t.is_empty())
        .cloned()
        .collect();

    // Extract custom fields (anything not in the standard set)
    let standard_keys: &[&str] = &["Title", "UserName", "Password", "URL", "Notes"];
    let custom_fields: Vec<(String, String, bool)> = entry
        .fields
        .iter()
        .filter(|(k, _)| !standard_keys.contains(&k.as_str()))
        .map(|(k, v)| {
            let (value, is_protected) = match v {
                keepass::db::Value::Unprotected(s) => (s.clone(), false),
                keepass::db::Value::Protected(sec) => (
                    std::str::from_utf8(sec.unsecure())
                        .unwrap_or("")
                        .to_string(),
                    true,
                ),
                keepass::db::Value::Bytes(_) => (String::new(), false),
            };
            (k.clone(), value, is_protected)
        })
        .collect();

    EntryDetails {
        title,
        username,
        password,
        url,
        notes,
        tags,
        custom_fields,
    }
}

// ── Flattening ───────────────────────────────────────────────────────

fn flatten_nodes(
    nodes: &[VaultNode],
    depth: usize,
    parent_path: &[usize],
    out: &mut Vec<FlatNode>,
    parent_guides: &[bool],
) {
    let count = nodes.len();
    for (i, node) in nodes.iter().enumerate() {
        let is_last = i == count - 1;
        let mut path = parent_path.to_vec();
        path.push(i);

        let mut guide_depths = parent_guides.to_vec();
        if depth > 0 {
            // The guide at this depth level: true if there are more siblings below
            if guide_depths.len() < depth {
                guide_depths.resize(depth, false);
            }
        }

        out.push(FlatNode {
            path: path.clone(),
            name: node.name.clone(),
            node_type: node.node_type,
            depth,
            is_expanded: node.expanded,
            has_children: !node.children.is_empty(),
            guide_depths: guide_depths.clone(),
        });

        if node.expanded && !node.children.is_empty() {
            let mut child_guides = guide_depths.clone();
            child_guides.push(!is_last);
            flatten_nodes(&node.children, depth + 1, &path, out, &child_guides);
        }
    }
}
