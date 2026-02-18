pub mod crypto;
pub mod detail;
pub mod model;
pub mod sidebar;
pub mod ui;
pub mod vault;

use std::path::{Path, PathBuf};
use std::time::Instant;

use rstools_core::help_popup::HelpEntry;
use rstools_core::keybinds::{Action, InputMode, KeyState};
use rstools_core::telescope::TelescopeItem;
use rstools_core::tool::Tool;
use rstools_core::which_key::WhichKeyEntry;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{layout::Rect, Frame};
use rusqlite::Connection;
use zeroize::Zeroize;

use detail::DetailPanel;
use sidebar::SidebarState;
use vault::{SearchableEntry, VaultState};

// ── Auto-lock timeout ────────────────────────────────────────────────

/// Auto-lock after 15 minutes of inactivity.
const AUTO_LOCK_SECS: u64 = 15 * 60;

/// Clipboard auto-clear after 30 seconds.
const CLIPBOARD_CLEAR_SECS: u64 = 30;

// ── Input prompt types ───────────────────────────────────────────────

/// The different input prompts the tool can show.
pub enum InputPrompt {
    /// Master password entry for opening a vault.
    MasterPassword {
        buffer: String,
        file_path: String,
        error: Option<String>,
    },
    /// PIN entry for unlocking a remembered vault.
    PinInput {
        buffer: String,
        file_id: i64,
        file_path: String,
        error: Option<String>,
    },
    /// Ask the user if they want to set up a PIN after successful unlock.
    PinSetup { file_id: i64, password: String },
    /// PIN creation: entering the 4-digit PIN.
    PinCreate {
        buffer: String,
        file_id: i64,
        password: String,
    },
}

// ── Focus management ─────────────────────────────────────────────────

/// Which panel is currently focused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolFocus {
    Sidebar,
    Tree,
    Detail,
}

// ── Main tool struct ─────────────────────────────────────────────────

pub struct KeePassTool {
    pub sidebar: SidebarState,
    pub vault: Option<VaultState>,
    pub detail: DetailPanel,
    pub focus: ToolFocus,
    pub mode: InputMode,
    key_state: KeyState,
    conn: Connection,
    /// Current input prompt overlay.
    pub input_prompt: Option<InputPrompt>,
    /// Whether the vault is locked (auto-lock or manual).
    pub locked: bool,
    /// Last activity timestamp for auto-lock.
    last_activity: Instant,
    /// System clipboard instance.
    clipboard: Option<arboard::Clipboard>,
    /// When the clipboard was last set (for auto-clear).
    clipboard_set_at: Option<Instant>,
    /// Whether we copied a password (vs username/URL which don't need clearing).
    clipboard_is_sensitive: bool,
    /// Notification message to show briefly.
    pub clipboard_notification: Option<String>,
    /// When the notification was shown.
    notification_shown_at: Option<Instant>,
    /// Pending multi-key state for y-prefixed sequences (yu, yp, yU).
    pending_yank: bool,
    /// Search state.
    pub search_active: bool,
    pub search_query: String,
    pub search_results: Vec<SearchableEntry>,
    pub search_selected: usize,
    /// File picker state.
    file_picker_active: bool,
    file_picker_entries: Vec<PathBuf>,
    file_picker_query: String,
    file_picker_filtered: Vec<usize>,
    file_picker_selected: usize,
}

impl KeePassTool {
    pub fn new(conn: Connection) -> anyhow::Result<Self> {
        model::init_db(&conn)?;
        let mut sidebar = SidebarState::new();
        sidebar.reload(&conn)?;

        let clipboard = arboard::Clipboard::new().ok();

        Ok(Self {
            sidebar,
            vault: None,
            detail: DetailPanel::new(),
            focus: ToolFocus::Sidebar,
            mode: InputMode::Normal,
            key_state: KeyState::default(),
            conn,
            input_prompt: None,
            locked: false,
            last_activity: Instant::now(),
            clipboard,
            clipboard_set_at: None,
            clipboard_is_sensitive: false,
            clipboard_notification: None,
            notification_shown_at: None,
            pending_yank: false,
            search_active: false,
            search_query: String::new(),
            search_results: Vec::new(),
            search_selected: 0,
            file_picker_active: false,
            file_picker_entries: Vec::new(),
            file_picker_query: String::new(),
            file_picker_filtered: Vec::new(),
            file_picker_selected: 0,
        })
    }

    /// Record user activity to reset the auto-lock timer.
    fn touch_activity(&mut self) {
        self.last_activity = Instant::now();
    }

    // ── File opening ─────────────────────────────────────────────────

    /// Start the process of opening a .kdbx file.
    fn start_open_file(&mut self, file_path: &str) {
        let path = shellexpand(file_path);

        // Check if this file has a valid PIN stored
        if let Ok(Some(file)) = model::get_file_by_path(&self.conn, &path) {
            if file.has_pin {
                if let Some(ref expires) = file.pin_expires_at {
                    if !crypto::is_pin_expired(expires) {
                        // Has valid PIN — ask for PIN instead
                        self.input_prompt = Some(InputPrompt::PinInput {
                            buffer: String::new(),
                            file_id: file.id,
                            file_path: path,
                            error: None,
                        });
                        return;
                    } else {
                        // PIN expired — clear it and ask for full password
                        let _ = model::clear_pin(&self.conn, file.id);
                        self.input_prompt = Some(InputPrompt::MasterPassword {
                            buffer: String::new(),
                            file_path: path,
                            error: Some("PIN expired. Please enter your master password.".into()),
                        });
                        return;
                    }
                }
            }
        }

        // No PIN — ask for master password
        self.input_prompt = Some(InputPrompt::MasterPassword {
            buffer: String::new(),
            file_path: path,
            error: None,
        });
    }

    /// Actually open the vault with the given password.
    fn open_vault_with_password(&mut self, file_path: &str, password: &str) -> Result<(), String> {
        match VaultState::open(file_path, password) {
            Ok(vault) => {
                // Update the DB history
                let display_name = Path::new(file_path)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "vault".to_string());

                let file_id = model::upsert_file(&self.conn, file_path, &display_name)
                    .map_err(|e| e.to_string())?;

                let _ = model::touch_file(&self.conn, file_id);
                let _ = self.sidebar.reload(&self.conn);

                self.vault = Some(vault);
                self.detail.clear();
                self.locked = false;
                self.focus = ToolFocus::Tree;
                self.touch_activity();

                // Ask if user wants to set up a PIN
                self.input_prompt = Some(InputPrompt::PinSetup {
                    file_id,
                    password: password.to_string(),
                });

                Ok(())
            }
            Err(e) => Err(format!("Failed to open vault: {e}")),
        }
    }

    /// Lock the vault (clear sensitive data from detail, keep tree structure).
    fn lock_vault(&mut self) {
        if self.vault.is_some() {
            self.locked = true;
            self.detail.clear();
            // Don't clear vault tree — we just prevent access until re-unlock
        }
    }

    /// Attempt to unlock with PIN or password.
    fn unlock_vault(&mut self) {
        if !self.locked {
            return;
        }
        if let Some(ref vault) = self.vault {
            let file_path = vault.file_path.clone();
            self.start_open_file(&file_path);
        }
    }

    // ── Clipboard ────────────────────────────────────────────────────

    fn copy_to_clipboard(&mut self, text: &str, label: &str, sensitive: bool) {
        if let Some(ref mut cb) = self.clipboard {
            if cb.set_text(text.to_string()).is_ok() {
                self.clipboard_notification = Some(format!("Copied {label}"));
                self.notification_shown_at = Some(Instant::now());
                self.clipboard_is_sensitive = sensitive;
                if sensitive {
                    self.clipboard_set_at = Some(Instant::now());
                }
            }
        }
    }

    fn clear_clipboard_if_expired(&mut self) {
        if self.clipboard_is_sensitive {
            if let Some(set_at) = self.clipboard_set_at {
                if set_at.elapsed().as_secs() >= CLIPBOARD_CLEAR_SECS {
                    if let Some(ref mut cb) = self.clipboard {
                        let _ = cb.set_text(String::new());
                    }
                    self.clipboard_set_at = None;
                    self.clipboard_is_sensitive = false;
                }
            }
        }
    }

    // ── Search ───────────────────────────────────────────────────────

    fn open_search(&mut self) {
        if let Some(ref vault) = self.vault {
            self.search_results = vault.collect_searchable_entries();
            self.search_query.clear();
            self.search_selected = 0;
            self.search_active = true;
        }
    }

    fn filter_search(&mut self) {
        if let Some(ref vault) = self.vault {
            let query = self.search_query.to_lowercase();
            self.search_results = vault
                .collect_searchable_entries()
                .into_iter()
                .filter(|e| {
                    if query.is_empty() {
                        return true;
                    }
                    e.title.to_lowercase().contains(&query)
                })
                .collect();
            if self.search_results.is_empty() {
                self.search_selected = 0;
            } else if self.search_selected >= self.search_results.len() {
                self.search_selected = self.search_results.len() - 1;
            }
        }
    }

    fn close_search(&mut self) {
        self.search_active = false;
        self.search_query.clear();
        self.search_results.clear();
        self.search_selected = 0;
    }

    fn confirm_search_selection(&mut self) {
        if let Some(entry) = self.search_results.get(self.search_selected) {
            let path = entry.tree_path.clone();
            self.close_search();

            // Navigate to the selected entry in the vault tree
            if let Some(ref mut vault) = self.vault {
                // Expand all parent groups to make the entry visible
                for depth in 1..path.len() {
                    let parent_path = &path[..depth];
                    if let Some(node) = vault.node_at_path_mut_public(parent_path) {
                        node.expanded = true;
                    }
                }
                vault.rebuild_flat_view();

                // Select the entry
                if let Some(idx) = vault.flat_view.iter().position(|n| n.path == path) {
                    vault.selected = idx;
                    self.update_detail_from_selection();
                    self.focus = ToolFocus::Tree;
                }
            }
        }
    }

    // ── File picker ──────────────────────────────────────────────────

    fn open_file_picker(&mut self) {
        let keepass_dir = dirs_keepass();
        self.file_picker_entries.clear();
        self.file_picker_query.clear();
        self.file_picker_filtered.clear();
        self.file_picker_selected = 0;

        // Recursively scan for .kdbx files
        if let Ok(entries) = scan_kdbx_files(&keepass_dir) {
            self.file_picker_entries = entries;
            self.file_picker_filtered = (0..self.file_picker_entries.len()).collect();
        }

        self.file_picker_active = true;
    }

    fn filter_file_picker(&mut self) {
        let query = self.file_picker_query.to_lowercase();
        self.file_picker_filtered = self
            .file_picker_entries
            .iter()
            .enumerate()
            .filter(|(_, path)| {
                if query.is_empty() {
                    return true;
                }
                path.file_name()
                    .map(|n| n.to_string_lossy().to_lowercase().contains(&query))
                    .unwrap_or(false)
            })
            .map(|(i, _)| i)
            .collect();
        if self.file_picker_filtered.is_empty() {
            self.file_picker_selected = 0;
        } else if self.file_picker_selected >= self.file_picker_filtered.len() {
            self.file_picker_selected = self.file_picker_filtered.len() - 1;
        }
    }

    // ── Detail update ────────────────────────────────────────────────

    fn update_detail_from_selection(&mut self) {
        if let Some(ref vault) = self.vault {
            let details = vault.selected_details().cloned();
            self.detail.set_entry(details);
        }
    }

    // ── Key handling ─────────────────────────────────────────────────

    fn handle_prompt_key(&mut self, key: KeyEvent) -> Action {
        let prompt = match self.input_prompt.take() {
            Some(p) => p,
            None => return Action::None,
        };

        match prompt {
            InputPrompt::MasterPassword {
                mut buffer,
                file_path,
                ..
            } => match key.code {
                KeyCode::Esc => {
                    buffer.zeroize();
                    // Prompt dismissed
                }
                KeyCode::Enter => {
                    let result = self.open_vault_with_password(&file_path, &buffer);
                    buffer.zeroize();
                    if let Err(e) = result {
                        self.input_prompt = Some(InputPrompt::MasterPassword {
                            buffer: String::new(),
                            file_path,
                            error: Some(e),
                        });
                    }
                    // If successful, input_prompt is set to PinSetup in open_vault_with_password
                }
                KeyCode::Char(c) => {
                    buffer.push(c);
                    self.input_prompt = Some(InputPrompt::MasterPassword {
                        buffer,
                        file_path,
                        error: None,
                    });
                }
                KeyCode::Backspace => {
                    buffer.pop();
                    self.input_prompt = Some(InputPrompt::MasterPassword {
                        buffer,
                        file_path,
                        error: None,
                    });
                }
                _ => {
                    self.input_prompt = Some(InputPrompt::MasterPassword {
                        buffer,
                        file_path,
                        error: None,
                    });
                }
            },
            InputPrompt::PinInput {
                mut buffer,
                file_id,
                file_path,
                ..
            } => match key.code {
                KeyCode::Esc => {
                    buffer.zeroize();
                    // Prompt dismissed, ask for full password instead
                    self.input_prompt = Some(InputPrompt::MasterPassword {
                        buffer: String::new(),
                        file_path,
                        error: None,
                    });
                }
                KeyCode::Char(c) if c.is_ascii_digit() && buffer.len() < 4 => {
                    buffer.push(c);
                    if buffer.len() == 4 {
                        // Auto-submit when 4 digits entered
                        let pin = buffer.clone();
                        buffer.zeroize();

                        // Try to decrypt the password with this PIN
                        if let Ok(Some(file)) = model::get_file_by_path(&self.conn, &file_path) {
                            if let (Some(enc), Some(salt), Some(nonce)) =
                                (&file.encrypted_password, &file.pin_salt, &file.pin_nonce)
                            {
                                match crypto::decrypt_with_pin(enc, salt, nonce, &pin) {
                                    Ok(mut password) => {
                                        let result =
                                            self.open_vault_with_password(&file_path, &password);
                                        password.zeroize();
                                        if let Err(e) = result {
                                            self.input_prompt = Some(InputPrompt::PinInput {
                                                buffer: String::new(),
                                                file_id,
                                                file_path,
                                                error: Some(e),
                                            });
                                        } else {
                                            // Successfully opened — don't ask for PIN setup again
                                            // since we already have one
                                            self.input_prompt = None;
                                        }
                                    }
                                    Err(_) => {
                                        self.input_prompt = Some(InputPrompt::PinInput {
                                            buffer: String::new(),
                                            file_id,
                                            file_path,
                                            error: Some("Wrong PIN".into()),
                                        });
                                    }
                                }
                            }
                        }
                    } else {
                        self.input_prompt = Some(InputPrompt::PinInput {
                            buffer,
                            file_id,
                            file_path,
                            error: None,
                        });
                    }
                }
                KeyCode::Backspace => {
                    buffer.pop();
                    self.input_prompt = Some(InputPrompt::PinInput {
                        buffer,
                        file_id,
                        file_path,
                        error: None,
                    });
                }
                _ => {
                    self.input_prompt = Some(InputPrompt::PinInput {
                        buffer,
                        file_id,
                        file_path,
                        error: None,
                    });
                }
            },
            InputPrompt::PinSetup {
                file_id,
                mut password,
            } => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.input_prompt = Some(InputPrompt::PinCreate {
                        buffer: String::new(),
                        file_id,
                        password,
                    });
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    password.zeroize();
                    // Declined PIN setup
                }
                _ => {
                    self.input_prompt = Some(InputPrompt::PinSetup { file_id, password });
                }
            },
            InputPrompt::PinCreate {
                mut buffer,
                file_id,
                mut password,
            } => match key.code {
                KeyCode::Esc => {
                    buffer.zeroize();
                    password.zeroize();
                }
                KeyCode::Char(c) if c.is_ascii_digit() && buffer.len() < 4 => {
                    buffer.push(c);
                    if buffer.len() == 4 {
                        // PIN entered — encrypt and store
                        match crypto::encrypt_with_pin(&password, &buffer) {
                            Ok((enc, salt, nonce, expires)) => {
                                let _ = model::store_pin(
                                    &self.conn, file_id, &enc, &salt, &nonce, &expires,
                                );
                                let _ = self.sidebar.reload(&self.conn);
                            }
                            Err(_) => {
                                // Silently fail — PIN not stored
                            }
                        }
                        buffer.zeroize();
                        password.zeroize();
                    } else {
                        self.input_prompt = Some(InputPrompt::PinCreate {
                            buffer,
                            file_id,
                            password,
                        });
                    }
                }
                KeyCode::Backspace => {
                    buffer.pop();
                    self.input_prompt = Some(InputPrompt::PinCreate {
                        buffer,
                        file_id,
                        password,
                    });
                }
                _ => {
                    self.input_prompt = Some(InputPrompt::PinCreate {
                        buffer,
                        file_id,
                        password,
                    });
                }
            },
        }

        Action::None
    }

    fn handle_search_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc => {
                self.close_search();
            }
            KeyCode::Enter => {
                self.confirm_search_selection();
            }
            KeyCode::Char(c) => {
                self.search_query.push(c);
                self.filter_search();
            }
            KeyCode::Backspace => {
                self.search_query.pop();
                self.filter_search();
            }
            KeyCode::Down | KeyCode::Tab => {
                if !self.search_results.is_empty() {
                    self.search_selected = (self.search_selected + 1) % self.search_results.len();
                }
            }
            KeyCode::Up | KeyCode::BackTab => {
                if !self.search_results.is_empty() {
                    self.search_selected = if self.search_selected == 0 {
                        self.search_results.len() - 1
                    } else {
                        self.search_selected - 1
                    };
                }
            }
            _ => {}
        }
        Action::None
    }

    fn handle_file_picker_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc => {
                self.file_picker_active = false;
            }
            KeyCode::Enter => {
                if let Some(&idx) = self.file_picker_filtered.get(self.file_picker_selected) {
                    if let Some(path) = self.file_picker_entries.get(idx) {
                        let path_str = path.to_string_lossy().to_string();
                        self.file_picker_active = false;
                        self.start_open_file(&path_str);
                    }
                }
            }
            KeyCode::Char(c) => {
                self.file_picker_query.push(c);
                self.filter_file_picker();
            }
            KeyCode::Backspace => {
                self.file_picker_query.pop();
                self.filter_file_picker();
            }
            KeyCode::Down | KeyCode::Tab => {
                if !self.file_picker_filtered.is_empty() {
                    self.file_picker_selected =
                        (self.file_picker_selected + 1) % self.file_picker_filtered.len();
                }
            }
            KeyCode::Up | KeyCode::BackTab => {
                if !self.file_picker_filtered.is_empty() {
                    self.file_picker_selected = if self.file_picker_selected == 0 {
                        self.file_picker_filtered.len() - 1
                    } else {
                        self.file_picker_selected - 1
                    };
                }
            }
            _ => {}
        }
        Action::None
    }

    fn handle_sidebar_normal_key(&mut self, key: KeyEvent) -> Action {
        // Handle Ctrl-l to move to tree panel
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('l') => {
                    if self.vault.is_some() && !self.locked {
                        self.focus = ToolFocus::Tree;
                    }
                    return Action::None;
                }
                KeyCode::Char('j') | KeyCode::Char('k') | KeyCode::Char('h') => {
                    return Action::None;
                }
                _ => {}
            }
        }

        // Handle confirm delete
        if self.sidebar.confirm_delete {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    if let Some(file) = self.sidebar.selected_file() {
                        let id = file.id;
                        let _ = model::delete_file(&self.conn, id);
                        let _ = self.sidebar.reload(&self.conn);
                    }
                    self.sidebar.confirm_delete = false;
                }
                _ => {
                    self.sidebar.confirm_delete = false;
                }
            }
            return Action::None;
        }

        // Standard normal-mode handling
        let action = rstools_core::keybinds::process_normal_key(key, &mut self.key_state);

        match action {
            Action::MoveDown(_) => {
                self.sidebar.move_down();
                Action::None
            }
            Action::MoveUp(_) => {
                self.sidebar.move_up();
                Action::None
            }
            Action::GotoTop => {
                self.sidebar.goto_top();
                Action::None
            }
            Action::GotoBottom => {
                self.sidebar.goto_bottom();
                Action::None
            }
            Action::HalfPageDown => {
                self.sidebar.half_page_down(20);
                Action::None
            }
            Action::HalfPageUp => {
                self.sidebar.half_page_up(20);
                Action::None
            }
            Action::Confirm => {
                // Open the selected file
                if let Some(file) = self.sidebar.selected_file() {
                    let path = file.file_path.clone();
                    self.start_open_file(&path);
                }
                Action::None
            }
            Action::Delete => {
                if self.sidebar.selected_file().is_some() {
                    self.sidebar.confirm_delete = true;
                }
                Action::None
            }
            Action::Search => {
                self.open_search();
                Action::None
            }
            // Pass through hub-level actions
            Action::Quit
            | Action::LeaderKey
            | Action::LeaderSequence(_)
            | Action::SwitchTool(_)
            | Action::NextTool
            | Action::PrevTool
            | Action::ToolPicker
            | Action::Telescope
            | Action::Help
            | Action::SetMode(_) => action,
            _ => Action::None,
        }
    }

    fn handle_tree_normal_key(&mut self, key: KeyEvent) -> Action {
        // Handle Ctrl-h/l for panel navigation
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('h') => {
                    if self.sidebar.visible {
                        self.focus = ToolFocus::Sidebar;
                    }
                    return Action::None;
                }
                KeyCode::Char('l') => {
                    self.focus = ToolFocus::Detail;
                    return Action::None;
                }
                KeyCode::Char('j') | KeyCode::Char('k') => {
                    return Action::None;
                }
                _ => {}
            }
        }

        // Handle y-prefixed yank sequences
        if self.pending_yank {
            self.pending_yank = false;
            match key.code {
                KeyCode::Char('u') => {
                    // Yank username
                    if let Some(ref details) = self.detail.details {
                        let val = details.username.clone();
                        self.copy_to_clipboard(&val, "username", false);
                    }
                    return Action::None;
                }
                KeyCode::Char('p') => {
                    // Yank password
                    if let Some(ref details) = self.detail.details {
                        let val = details.password.clone();
                        self.copy_to_clipboard(&val, "password", true);
                    }
                    return Action::None;
                }
                KeyCode::Char('U') => {
                    // Yank URL
                    if let Some(ref details) = self.detail.details {
                        let val = details.url.clone();
                        self.copy_to_clipboard(&val, "URL", false);
                    }
                    return Action::None;
                }
                _ => {
                    // Invalid yank sequence, fall through to normal processing
                }
            }
        }

        // Check for 'y' to start yank sequence
        if key.code == KeyCode::Char('y') && key.modifiers == KeyModifiers::NONE {
            self.pending_yank = true;
            return Action::None;
        }

        // Check for 'p' to toggle password visibility
        if key.code == KeyCode::Char('p') && key.modifiers == KeyModifiers::NONE {
            self.detail.toggle_password();
            return Action::None;
        }

        let action = rstools_core::keybinds::process_normal_key(key, &mut self.key_state);

        match action {
            Action::MoveDown(_) => {
                if let Some(ref mut vault) = self.vault {
                    vault.move_down();
                }
                self.update_detail_from_selection();
                Action::None
            }
            Action::MoveUp(_) => {
                if let Some(ref mut vault) = self.vault {
                    vault.move_up();
                }
                self.update_detail_from_selection();
                Action::None
            }
            Action::GotoTop => {
                if let Some(ref mut vault) = self.vault {
                    vault.goto_top();
                }
                self.update_detail_from_selection();
                Action::None
            }
            Action::GotoBottom => {
                if let Some(ref mut vault) = self.vault {
                    vault.goto_bottom();
                }
                self.update_detail_from_selection();
                Action::None
            }
            Action::HalfPageDown => {
                if let Some(ref mut vault) = self.vault {
                    vault.half_page_down(20);
                }
                self.update_detail_from_selection();
                Action::None
            }
            Action::HalfPageUp => {
                if let Some(ref mut vault) = self.vault {
                    vault.half_page_up(20);
                }
                self.update_detail_from_selection();
                Action::None
            }
            Action::Confirm => {
                // Toggle expand or select entry
                if let Some(ref mut vault) = self.vault {
                    if let Some(flat) = vault.flat_view.get(vault.selected) {
                        if flat.node_type == vault::NodeType::Group {
                            vault.toggle_expand();
                        }
                    }
                }
                self.update_detail_from_selection();
                Action::None
            }
            Action::Search => {
                self.open_search();
                Action::None
            }
            // 'h' collapses or goes to parent, 'l' expands
            Action::None if key.code == KeyCode::Char('h') => {
                if let Some(ref mut vault) = self.vault {
                    vault.collapse_or_parent();
                }
                self.update_detail_from_selection();
                Action::None
            }
            Action::SetMode(InputMode::Insert) if key.code == KeyCode::Char('l') => {
                // Override 'l' to expand instead of going to insert mode
                if let Some(ref mut vault) = self.vault {
                    vault.expand_selected();
                }
                self.update_detail_from_selection();
                Action::None
            }
            // Pass through hub-level actions
            Action::Quit
            | Action::LeaderKey
            | Action::LeaderSequence(_)
            | Action::SwitchTool(_)
            | Action::NextTool
            | Action::PrevTool
            | Action::ToolPicker
            | Action::Telescope
            | Action::Help
            | Action::SetMode(_) => action,
            _ => Action::None,
        }
    }

    fn handle_detail_normal_key(&mut self, key: KeyEvent) -> Action {
        // Handle Ctrl-h to go to tree
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('h') => {
                    self.focus = ToolFocus::Tree;
                    return Action::None;
                }
                KeyCode::Char('j') | KeyCode::Char('k') | KeyCode::Char('l') => {
                    return Action::None;
                }
                _ => {}
            }
        }

        // Handle y-prefixed yank sequences
        if self.pending_yank {
            self.pending_yank = false;
            match key.code {
                KeyCode::Char('u') => {
                    if let Some(ref details) = self.detail.details {
                        let val = details.username.clone();
                        self.copy_to_clipboard(&val, "username", false);
                    }
                    return Action::None;
                }
                KeyCode::Char('p') => {
                    if let Some(ref details) = self.detail.details {
                        let val = details.password.clone();
                        self.copy_to_clipboard(&val, "password", true);
                    }
                    return Action::None;
                }
                KeyCode::Char('U') => {
                    if let Some(ref details) = self.detail.details {
                        let val = details.url.clone();
                        self.copy_to_clipboard(&val, "URL", false);
                    }
                    return Action::None;
                }
                _ => {}
            }
        }

        if key.code == KeyCode::Char('y') && key.modifiers == KeyModifiers::NONE {
            self.pending_yank = true;
            return Action::None;
        }

        if key.code == KeyCode::Char('p') && key.modifiers == KeyModifiers::NONE {
            self.detail.toggle_password();
            return Action::None;
        }

        match key.code {
            KeyCode::Char('j') => {
                self.detail.scroll_down();
                Action::None
            }
            KeyCode::Char('k') => {
                self.detail.scroll_up();
                Action::None
            }
            KeyCode::Char('G') => {
                // Scroll to bottom (large number)
                self.detail.scroll = 999;
                Action::None
            }
            KeyCode::Char('/') => {
                self.open_search();
                Action::None
            }
            KeyCode::Char(' ') => {
                self.key_state.leader_active = true;
                Action::LeaderKey
            }
            KeyCode::Char(':') => Action::SetMode(InputMode::Command),
            KeyCode::Char('?') => Action::Help,
            KeyCode::Char('q') => Action::Quit,
            _ => {
                let action = rstools_core::keybinds::process_normal_key(key, &mut self.key_state);
                match action {
                    Action::Quit
                    | Action::LeaderKey
                    | Action::LeaderSequence(_)
                    | Action::SwitchTool(_)
                    | Action::NextTool
                    | Action::PrevTool
                    | Action::ToolPicker
                    | Action::Telescope
                    | Action::Help
                    | Action::SetMode(_) => action,
                    _ => Action::None,
                }
            }
        }
    }
}

// ── Tool trait implementation ────────────────────────────────────────

impl Tool for KeePassTool {
    fn name(&self) -> &str {
        "KeePass"
    }

    fn description(&self) -> &str {
        "KeePass KDBX4 vault viewer"
    }

    fn mode(&self) -> InputMode {
        self.mode
    }

    fn init_db(&self, conn: &Connection) -> anyhow::Result<()> {
        model::init_db(conn)
    }

    fn which_key_entries(&self) -> Vec<WhichKeyEntry> {
        vec![
            WhichKeyEntry::action('o', "Open file picker"),
            WhichKeyEntry::action('e', "Toggle sidebar"),
            WhichKeyEntry::action('s', "Search entries"),
        ]
    }

    fn telescope_items(&self) -> Vec<TelescopeItem> {
        let mut items = Vec::new();
        if let Some(ref vault) = self.vault {
            for entry in vault.collect_searchable_entries() {
                items.push(TelescopeItem {
                    label: entry.title.clone(),
                    description: format!("{} ({})", entry.group_path, entry.username),
                    id: format!("keepass:{}", entry.title),
                });
            }
        }
        items
    }

    fn help_entries(&self) -> Vec<HelpEntry> {
        vec![
            // Sidebar
            HelpEntry::with_section("Sidebar", "j / k", "Navigate up / down"),
            HelpEntry::with_section("Sidebar", "Enter", "Open selected file"),
            HelpEntry::with_section("Sidebar", "dd", "Remove file from history"),
            HelpEntry::with_section("Sidebar", "gg / G", "Go to top / bottom"),
            HelpEntry::with_section("Sidebar", "Ctrl-l", "Move focus to tree"),
            // Tree
            HelpEntry::with_section("Tree", "j / k", "Navigate up / down"),
            HelpEntry::with_section("Tree", "h", "Collapse / go to parent"),
            HelpEntry::with_section("Tree", "l / Enter", "Expand group"),
            HelpEntry::with_section("Tree", "gg / G", "Go to top / bottom"),
            HelpEntry::with_section("Tree", "Ctrl-h", "Focus sidebar"),
            HelpEntry::with_section("Tree", "Ctrl-l", "Focus details"),
            HelpEntry::with_section("Tree", "/", "Search entries"),
            // Detail
            HelpEntry::with_section("Detail", "j / k", "Scroll up / down"),
            HelpEntry::with_section("Detail", "p", "Toggle password visibility"),
            HelpEntry::with_section("Detail", "Ctrl-h", "Focus tree"),
            // Copy
            HelpEntry::with_section("Copy", "yu", "Copy username"),
            HelpEntry::with_section("Copy", "yp", "Copy password (auto-clears 30s)"),
            HelpEntry::with_section("Copy", "yU", "Copy URL"),
            // General
            HelpEntry::with_section("General", "<Space>ko", "File picker (~/keepass)"),
            HelpEntry::with_section("General", "<Space>ke", "Toggle sidebar"),
            HelpEntry::with_section("General", "<Space>ks", "Search entries"),
            HelpEntry::with_section("General", ":open <path>", "Open .kdbx file"),
        ]
    }

    fn handle_key(&mut self, key: KeyEvent) -> Action {
        self.touch_activity();

        // Handle input prompts first
        if self.input_prompt.is_some() {
            return self.handle_prompt_key(key);
        }

        // Handle search overlay
        if self.search_active {
            return self.handle_search_key(key);
        }

        // Handle file picker overlay
        if self.file_picker_active {
            return self.handle_file_picker_key(key);
        }

        // Handle locked state
        if self.locked {
            if key.code == KeyCode::Enter {
                self.unlock_vault();
            }
            return Action::None;
        }

        match self.mode {
            InputMode::Normal => match self.focus {
                ToolFocus::Sidebar => self.handle_sidebar_normal_key(key),
                ToolFocus::Tree => self.handle_tree_normal_key(key),
                ToolFocus::Detail => self.handle_detail_normal_key(key),
            },
            InputMode::Insert | InputMode::Command => {
                // Command mode is handled by the hub
                Action::None
            }
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent, area: Rect) -> Action {
        self.touch_activity();

        if self.input_prompt.is_some() || self.search_active || self.file_picker_active {
            return Action::None;
        }

        let sidebar_width = if self.sidebar.visible {
            ui::SIDEBAR_WIDTH.min(area.width.saturating_sub(20))
        } else {
            0
        };

        let in_sidebar = self.sidebar.visible && mouse.column < area.x + sidebar_width;

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if in_sidebar {
                    self.focus = ToolFocus::Sidebar;
                    // Calculate clicked index
                    let y_offset = mouse.row.saturating_sub(area.y + 1) as usize;
                    let visible_lines = area.height.saturating_sub(2) as usize;
                    let scroll_offset = if self.sidebar.selected >= visible_lines {
                        self.sidebar.selected - visible_lines + 1
                    } else {
                        0
                    };
                    let clicked = scroll_offset + y_offset;
                    if clicked < self.sidebar.files.len() {
                        self.sidebar.selected = clicked;
                    }
                } else {
                    // Determine if click is in tree or detail area
                    let content_x = area.x + sidebar_width;
                    let content_width = area.width.saturating_sub(sidebar_width);
                    let tree_width = content_width * 40 / 100;

                    if mouse.column < content_x + tree_width {
                        self.focus = ToolFocus::Tree;
                    } else {
                        self.focus = ToolFocus::Detail;
                    }
                }
                Action::None
            }
            MouseEventKind::ScrollDown => {
                match self.focus {
                    ToolFocus::Sidebar => self.sidebar.move_down(),
                    ToolFocus::Tree => {
                        if let Some(ref mut vault) = self.vault {
                            vault.move_down();
                        }
                        self.update_detail_from_selection();
                    }
                    ToolFocus::Detail => self.detail.scroll_down(),
                }
                Action::None
            }
            MouseEventKind::ScrollUp => {
                match self.focus {
                    ToolFocus::Sidebar => self.sidebar.move_up(),
                    ToolFocus::Tree => {
                        if let Some(ref mut vault) = self.vault {
                            vault.move_up();
                        }
                        self.update_detail_from_selection();
                    }
                    ToolFocus::Detail => self.detail.scroll_up(),
                }
                Action::None
            }
            _ => Action::None,
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        // If file picker is active, render it as overlay
        if self.file_picker_active {
            ui::render_keepass_tool(frame, area, self);
            render_file_picker(frame, area, self);
            return;
        }
        ui::render_keepass_tool(frame, area, self);
    }

    fn handle_leader_action(&mut self, key: char) -> Option<Action> {
        match key {
            'e' => {
                self.sidebar.visible = !self.sidebar.visible;
                if self.sidebar.visible {
                    self.focus = ToolFocus::Sidebar;
                } else if self.vault.is_some() {
                    self.focus = ToolFocus::Tree;
                }
                Some(Action::None)
            }
            'o' => {
                self.open_file_picker();
                Some(Action::None)
            }
            's' => {
                self.open_search();
                Some(Action::None)
            }
            _ => None,
        }
    }

    fn reset_key_state(&mut self) {
        self.key_state.reset();
        self.pending_yank = false;
    }

    fn tick(&mut self) {
        // Auto-lock check
        if self.vault.is_some()
            && !self.locked
            && self.input_prompt.is_none()
            && self.last_activity.elapsed().as_secs() >= AUTO_LOCK_SECS
        {
            self.lock_vault();
        }

        // Clipboard auto-clear
        self.clear_clipboard_if_expired();

        // Clear notification after 2 seconds
        if let Some(shown_at) = self.notification_shown_at {
            if shown_at.elapsed().as_secs() >= 2 {
                self.clipboard_notification = None;
                self.notification_shown_at = None;
            }
        }
    }

    fn handle_command(&mut self, cmd: &str) -> bool {
        let parts: Vec<&str> = cmd.splitn(2, ' ').collect();
        match parts.first() {
            Some(&"open") => {
                if let Some(path) = parts.get(1) {
                    self.start_open_file(path.trim());
                }
                true
            }
            _ => false,
        }
    }

    fn on_focus(&mut self) {
        self.touch_activity();
    }

    fn on_blur(&mut self) {}
}

// ── File picker overlay rendering ────────────────────────────────────

fn render_file_picker(frame: &mut Frame, area: Rect, tool: &KeePassTool) {
    use ratatui::{
        layout::{Constraint, Flex, Layout},
        style::{Modifier, Style},
        text::{Line, Span},
        widgets::{Block, Borders, Clear, Paragraph},
    };

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

    frame.render_widget(Clear, popup_area);

    let [input_area, results_area] =
        Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).areas(popup_area);

    // Search input
    let input_block = Block::default()
        .title(" Open KeePass File ")
        .borders(Borders::ALL);

    let input_text = Paragraph::new(Line::from(vec![
        Span::styled("> ", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(&tool.file_picker_query),
    ]))
    .block(input_block);
    frame.render_widget(input_text, input_area);

    frame.set_cursor_position((
        input_area.x + 2 + tool.file_picker_query.len() as u16 + 1,
        input_area.y + 1,
    ));

    // Results list
    let results_block = Block::default().borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM);
    let inner = results_block.inner(results_area);
    frame.render_widget(results_block, results_area);

    let visible_lines = inner.height as usize;
    let scroll = if tool.file_picker_selected >= visible_lines {
        tool.file_picker_selected - visible_lines + 1
    } else {
        0
    };

    let lines: Vec<Line> = tool
        .file_picker_filtered
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible_lines)
        .map(|(i, &idx)| {
            let path = &tool.file_picker_entries[idx];
            let is_selected = i == tool.file_picker_selected;
            let bg = if is_selected {
                ratatui::style::Color::DarkGray
            } else {
                ratatui::style::Color::Reset
            };
            let prefix = if is_selected { "> " } else { "  " };
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            Line::from(vec![
                Span::styled(
                    prefix,
                    Style::default().fg(ratatui::style::Color::Yellow).bg(bg),
                ),
                Span::styled(
                    name,
                    Style::default()
                        .fg(ratatui::style::Color::White)
                        .bg(bg)
                        .add_modifier(if is_selected {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
            ])
        })
        .collect();

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

// ── Utility functions ────────────────────────────────────────────────

/// Expand ~ to home directory.
fn shellexpand(path: &str) -> String {
    if path.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            return format!("{}{}", home.display(), &path[1..]);
        }
    }
    path.to_string()
}

/// Get the default KeePass directory.
fn dirs_keepass() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("keepass")
}

/// Get the user's home directory.
mod dirs {
    use std::path::PathBuf;

    pub fn home_dir() -> Option<PathBuf> {
        std::env::var("HOME").ok().map(PathBuf::from)
    }
}

/// Recursively scan a directory for .kdbx files.
fn scan_kdbx_files(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut results = Vec::new();

    if !dir.exists() {
        return Ok(results);
    }

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Ok(sub) = scan_kdbx_files(&path) {
                    results.extend(sub);
                }
            } else if path.extension().is_some_and(|e| e == "kdbx") {
                results.push(path);
            }
        }
    }

    results.sort();
    Ok(results)
}
