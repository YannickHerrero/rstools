pub mod connection_form;
pub mod driver;
pub mod model;
pub mod table_view;
pub mod tunnel;
pub mod ui;

use std::sync::mpsc;
use std::thread;
use std::time::Instant;

use rstools_core::help_popup::HelpEntry;
use rstools_core::keybinds::{Action, InputMode, KeyState, process_normal_key};
use rstools_core::telescope::TelescopeItem;
use rstools_core::tool::Tool;
use rstools_core::which_key::WhichKeyEntry;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Frame, layout::Rect};
use rusqlite::Connection;

use connection_form::ConnectionForm;
use driver::{ColumnInfo, PgDriver, QueryParams, QueryResult, SortDirection, TableInfo};
use model::DbConnection;
use table_view::{TableAction, TableView, BATCH_SIZE};
use tunnel::SshTunnelConfig;

// ── Async executor ──────────────────────────────────────────────────

/// Commands sent from the UI thread to the executor thread.
enum DbCommand {
    Connect {
        host: String,
        port: u16,
        database: String,
        username: String,
        password: String,
        ssl: bool,
        ssh_config: Option<SshTunnelConfig>,
    },
    GetTables,
    GetColumns {
        schema: String,
        table: String,
    },
    Query(QueryParams),
    TestConnection {
        host: String,
        port: u16,
        database: String,
        username: String,
        password: String,
        ssl: bool,
        ssh_config: Option<SshTunnelConfig>,
    },
    Disconnect,
}

/// Results sent back from the executor thread.
enum DbResult {
    Connected(String),
    Tables(Vec<TableInfo>),
    Columns(Vec<ColumnInfo>),
    QueryResult(QueryResult),
    TestResult(String),
    Error(String),
    Disconnected,
}

struct DbExecutor {
    sender: mpsc::Sender<DbCommand>,
    receiver: mpsc::Receiver<DbResult>,
}

impl DbExecutor {
    fn spawn() -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<DbCommand>();
        let (result_tx, result_rx) = mpsc::channel::<DbResult>();

        thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create tokio runtime");

            rt.block_on(async move {
                let mut driver: Option<PgDriver> = None;
                let mut _tunnel_handle: Option<tokio::task::JoinHandle<()>> = None;

                while let Ok(cmd) = cmd_rx.recv() {
                    match cmd {
                        DbCommand::Connect {
                            host,
                            port,
                            database,
                            username,
                            password,
                            ssl,
                            ssh_config,
                        } => {
                            let result = async {
                                let (connect_host, connect_port) = if let Some(ssh) = ssh_config {
                                    let (local_port, handle) =
                                        tunnel::create_ssh_tunnel(&ssh).await?;
                                    _tunnel_handle = Some(handle);
                                    ("127.0.0.1".to_string(), local_port)
                                } else {
                                    (host, port)
                                };

                                let pg = PgDriver::connect(
                                    &connect_host,
                                    connect_port,
                                    &database,
                                    &username,
                                    &password,
                                    ssl,
                                )
                                .await?;

                                let version = pg.server_version().await?;
                                Ok::<(PgDriver, String), anyhow::Error>((pg, version))
                            }
                            .await;

                            match result {
                                Ok((pg, version)) => {
                                    driver = Some(pg);
                                    let _ = result_tx.send(DbResult::Connected(version));
                                }
                                Err(e) => {
                                    let _ =
                                        result_tx.send(DbResult::Error(format!("Connect: {e}")));
                                }
                            }
                        }
                        DbCommand::GetTables => {
                            if let Some(ref d) = driver {
                                match d.get_tables().await {
                                    Ok(tables) => {
                                        let _ = result_tx.send(DbResult::Tables(tables));
                                    }
                                    Err(e) => {
                                        let _ = result_tx
                                            .send(DbResult::Error(format!("Tables: {e}")));
                                    }
                                }
                            } else {
                                let _ =
                                    result_tx.send(DbResult::Error("Not connected".to_string()));
                            }
                        }
                        DbCommand::GetColumns { schema, table } => {
                            if let Some(ref d) = driver {
                                match d.get_columns(&schema, &table).await {
                                    Ok(cols) => {
                                        let _ = result_tx.send(DbResult::Columns(cols));
                                    }
                                    Err(e) => {
                                        let _ = result_tx
                                            .send(DbResult::Error(format!("Columns: {e}")));
                                    }
                                }
                            } else {
                                let _ =
                                    result_tx.send(DbResult::Error("Not connected".to_string()));
                            }
                        }
                        DbCommand::Query(params) => {
                            if let Some(ref d) = driver {
                                match d.query(&params).await {
                                    Ok(qr) => {
                                        let _ = result_tx.send(DbResult::QueryResult(qr));
                                    }
                                    Err(e) => {
                                        let _ =
                                            result_tx.send(DbResult::Error(format!("Query: {e}")));
                                    }
                                }
                            } else {
                                let _ =
                                    result_tx.send(DbResult::Error("Not connected".to_string()));
                            }
                        }
                        DbCommand::TestConnection {
                            host,
                            port,
                            database,
                            username,
                            password,
                            ssl,
                            ssh_config,
                        } => {
                            let result = async {
                                let (connect_host, connect_port, _handle) =
                                    if let Some(ssh) = ssh_config {
                                        let (local_port, handle) =
                                            tunnel::create_ssh_tunnel(&ssh).await?;
                                        ("127.0.0.1".to_string(), local_port, Some(handle))
                                    } else {
                                        (host, port, None)
                                    };

                                let pg = PgDriver::connect(
                                    &connect_host,
                                    connect_port,
                                    &database,
                                    &username,
                                    &password,
                                    ssl,
                                )
                                .await?;

                                let version = pg.server_version().await?;
                                Ok::<String, anyhow::Error>(version)
                            }
                            .await;

                            match result {
                                Ok(version) => {
                                    let _ = result_tx.send(DbResult::TestResult(version));
                                }
                                Err(e) => {
                                    let _ = result_tx.send(DbResult::Error(format!("Test: {e}")));
                                }
                            }
                        }
                        DbCommand::Disconnect => {
                            driver = None;
                            _tunnel_handle = None;
                            let _ = result_tx.send(DbResult::Disconnected);
                        }
                    }
                }
            });
        });

        Self {
            sender: cmd_tx,
            receiver: result_rx,
        }
    }

    fn send(&self, cmd: DbCommand) {
        let _ = self.sender.send(cmd);
    }

    fn try_recv(&self) -> Option<DbResult> {
        self.receiver.try_recv().ok()
    }
}

// ── Focus state ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum Focus {
    Sidebar,
    TableView,
    ConnectionForm,
    PinPrompt,
}

/// PIN prompt for decrypting connection passwords.
struct PinPrompt {
    buffer: String,
    connection_id: i64,
    error: Option<String>,
}

// ── Sidebar entry ───────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum SidebarSection {
    Connections,
    Tables,
}

#[derive(Debug, Clone)]
struct SidebarEntry {
    id: String,
    label: String,
    _section: SidebarSection,
    is_header: bool,
}

// ── Main tool ───────────────────────────────────────────────────────

pub struct DatabaseTool {
    conn: Connection,
    executor: DbExecutor,
    mode: InputMode,
    key_state: KeyState,
    focus: Focus,

    // Sidebar state
    connections: Vec<DbConnection>,
    tables: Vec<TableInfo>,
    sidebar_entries: Vec<SidebarEntry>,
    sidebar_selected: usize,
    sidebar_scroll: usize,

    // Active connection
    active_connection_id: Option<i64>,
    connected_version: Option<String>,

    // Table view
    table_view: TableView,
    active_table: Option<(String, String)>, // (schema, table)

    // Connection form
    connection_form: Option<ConnectionForm>,
    editing_connection_id: Option<i64>,

    // PIN prompt
    pin_prompt: Option<PinPrompt>,

    // Async state
    loading: bool,
    spinner_frame: u8,
    /// When true, the next QueryResult should be appended instead of replacing.
    pending_load_more: bool,
    error_message: Option<String>,
    error_shown_at: Option<Instant>,
    status_message: Option<String>,
    status_shown_at: Option<Instant>,
}

impl DatabaseTool {
    pub fn new(conn: Connection) -> anyhow::Result<Self> {
        model::init_db(&conn)?;
        let connections = model::list_connections(&conn)?;

        let mut tool = Self {
            conn,
            executor: DbExecutor::spawn(),
            mode: InputMode::Normal,
            key_state: KeyState::default(),
            focus: Focus::Sidebar,
            connections,
            tables: Vec::new(),
            sidebar_entries: Vec::new(),
            sidebar_selected: 0,
            sidebar_scroll: 0,
            active_connection_id: None,
            connected_version: None,
            table_view: TableView::new(),
            active_table: None,
            connection_form: None,
            editing_connection_id: None,
            pin_prompt: None,
            loading: false,
            spinner_frame: 0,
            pending_load_more: false,
            error_message: None,
            error_shown_at: None,
            status_message: None,
            status_shown_at: None,
        };
        tool.rebuild_sidebar();
        Ok(tool)
    }

    fn rebuild_sidebar(&mut self) {
        self.sidebar_entries.clear();

        // Connections header
        self.sidebar_entries.push(SidebarEntry {
            id: "header:connections".to_string(),
            label: "Connections".to_string(),
            _section: SidebarSection::Connections,
            is_header: true,
        });

        for c in &self.connections {
            self.sidebar_entries.push(SidebarEntry {
                id: format!("conn:{}", c.id),
                label: c.name.clone(),
                _section: SidebarSection::Connections,
                is_header: false,
            });
        }

        // Tables header (only if connected)
        if self.active_connection_id.is_some() {
            self.sidebar_entries.push(SidebarEntry {
                id: "header:tables".to_string(),
                label: "Tables".to_string(),
                _section: SidebarSection::Tables,
                is_header: true,
            });

            for t in &self.tables {
                let label = if t.schema == "public" {
                    t.name.clone()
                } else {
                    format!("{}.{}", t.schema, t.name)
                };
                self.sidebar_entries.push(SidebarEntry {
                    id: format!("table:{}.{}", t.schema, t.name),
                    label,
                    _section: SidebarSection::Tables,
                    is_header: false,
                });
            }
        }
    }

    fn reload_connections(&mut self) {
        if let Ok(conns) = model::list_connections(&self.conn) {
            self.connections = conns;
            self.rebuild_sidebar();
        }
    }

    fn sidebar_move_down(&mut self) {
        if self.sidebar_selected + 1 < self.sidebar_entries.len() {
            self.sidebar_selected += 1;
            // Skip headers
            if self.sidebar_entries[self.sidebar_selected].is_header
                && self.sidebar_selected + 1 < self.sidebar_entries.len()
            {
                self.sidebar_selected += 1;
            }
        }
    }

    fn sidebar_move_up(&mut self) {
        if self.sidebar_selected > 0 {
            self.sidebar_selected -= 1;
            // Skip headers
            if self.sidebar_entries[self.sidebar_selected].is_header && self.sidebar_selected > 0 {
                self.sidebar_selected -= 1;
            }
        }
    }

    fn sidebar_confirm(&mut self) {
        if self.sidebar_selected >= self.sidebar_entries.len() {
            return;
        }
        let entry = self.sidebar_entries[self.sidebar_selected].clone();
        if entry.is_header {
            return;
        }

        if let Some(id_str) = entry.id.strip_prefix("conn:") {
            if let Ok(id) = id_str.parse::<i64>() {
                self.connect_to(id);
            }
        } else if let Some(table_ref) = entry.id.strip_prefix("table:") {
            if let Some((schema, table)) = table_ref.split_once('.') {
                self.select_table(schema.to_string(), table.to_string());
            }
        }
    }

    fn connect_to(&mut self, connection_id: i64) {
        let Some(conn) = self.connections.iter().find(|c| c.id == connection_id) else {
            return;
        };

        // Check if password is encrypted and needs PIN
        if conn.encrypted_password.is_some() {
            // Check expiry
            let expired = conn
                .password_expires_at
                .as_ref()
                .map(|e| rstools_core::crypto::is_pin_expired(e))
                .unwrap_or(true);

            if expired {
                // PIN expired, need to re-enter
                self.pin_prompt = Some(PinPrompt {
                    buffer: String::new(),
                    connection_id,
                    error: Some("PIN expired — re-enter to decrypt".to_string()),
                });
                self.focus = Focus::PinPrompt;
            } else {
                // Need PIN to decrypt
                self.pin_prompt = Some(PinPrompt {
                    buffer: String::new(),
                    connection_id,
                    error: None,
                });
                self.focus = Focus::PinPrompt;
            }
            return;
        }

        // No encryption — connect directly with stored plaintext password
        let password = conn.password.clone();
        self.do_connect(connection_id, password);
    }

    fn do_connect(&mut self, connection_id: i64, password: String) {
        let Some(conn) = self.connections.iter().find(|c| c.id == connection_id) else {
            return;
        };

        let ssh_config = if conn.ssh_enabled {
            Some(SshTunnelConfig {
                ssh_host: conn.ssh_host.clone().unwrap_or_default(),
                ssh_port: conn.ssh_port.unwrap_or(22) as u16,
                ssh_username: conn.ssh_username.clone().unwrap_or_default(),
                private_key_path: conn.ssh_private_key_path.clone().unwrap_or_default(),
                passphrase: None, // TODO: decrypt SSH passphrase if encrypted
                remote_host: conn.host.clone(),
                remote_port: conn.port as u16,
            })
        } else {
            None
        };

        self.loading = true;
        self.active_connection_id = Some(connection_id);
        self.executor.send(DbCommand::Connect {
            host: conn.host.clone(),
            port: conn.port as u16,
            database: conn.database_name.clone(),
            username: conn.username.clone(),
            password,
            ssl: conn.ssl_enabled,
            ssh_config,
        });
    }

    fn select_table(&mut self, schema: String, table: String) {
        self.active_table = Some((schema.clone(), table.clone()));
        self.table_view.reset();
        self.loading = true;
        self.executor.send(DbCommand::GetColumns { schema: schema.clone(), table: table.clone() });
        self.executor.send(DbCommand::Query(QueryParams {
            table,
            schema,
            offset: 0,
            limit: BATCH_SIZE,
            sort_column: None,
            sort_direction: SortDirection::Asc,
            filters: Vec::new(),
        }));
    }

    /// Re-fetch from scratch (sort/filter changed).
    fn refresh_table(&mut self) {
        if let Some((ref schema, ref table)) = self.active_table {
            self.table_view.rows.clear();
            self.table_view.loaded_count = 0;
            self.table_view.selected_row = 0;
            self.loading = true;
            self.executor.send(DbCommand::Query(QueryParams {
                table: table.clone(),
                schema: schema.clone(),
                offset: 0,
                limit: BATCH_SIZE,
                sort_column: self.table_view.sort_column_name(),
                sort_direction: self.table_view.sort_direction,
                filters: self.table_view.filters.clone(),
            }));
        }
    }

    /// Fetch the next batch of rows and append.
    fn load_more_rows(&mut self) {
        if let Some((ref schema, ref table)) = self.active_table {
            let offset_rows = self.table_view.loaded_count;
            self.loading = true;
            self.pending_load_more = true;
            self.executor.send(DbCommand::Query(QueryParams {
                table: table.clone(),
                schema: schema.clone(),
                offset: offset_rows,
                limit: BATCH_SIZE,
                sort_column: self.table_view.sort_column_name(),
                sort_direction: self.table_view.sort_direction,
                filters: self.table_view.filters.clone(),
            }));
        }
    }

    fn poll_results(&mut self) {
        while let Some(result) = self.executor.try_recv() {
            match result {
                DbResult::Connected(version) => {
                    self.loading = false;
                    self.connected_version = Some(version.clone());
                    self.status_message = Some(format!("Connected: {version}"));
                    self.status_shown_at = Some(Instant::now());
                    // Fetch tables
                    self.executor.send(DbCommand::GetTables);
                }
                DbResult::Tables(tables) => {
                    self.tables = tables;
                    self.rebuild_sidebar();
                }
                DbResult::Columns(cols) => {
                    self.table_view.set_columns(cols);
                }
                DbResult::QueryResult(qr) => {
                    self.loading = false;
                    if self.pending_load_more {
                        self.pending_load_more = false;
                        self.table_view.append_data(qr);
                    } else {
                        self.table_view.set_data(qr);
                    }
                    self.focus = Focus::TableView;
                }
                DbResult::TestResult(version) => {
                    self.loading = false;
                    if let Some(ref mut form) = self.connection_form {
                        form.test_result = Some(Ok(version));
                    }
                }
                DbResult::Error(msg) => {
                    self.loading = false;
                    // Check if we're testing connection
                    if let Some(ref mut form) = self.connection_form {
                        if msg.starts_with("Test:") {
                            form.test_result =
                                Some(Err(msg.strip_prefix("Test: ").unwrap_or(&msg).to_string()));
                            continue;
                        }
                    }
                    self.error_message = Some(msg);
                    self.error_shown_at = Some(Instant::now());
                }
                DbResult::Disconnected => {
                    self.active_connection_id = None;
                    self.connected_version = None;
                    self.tables.clear();
                    self.table_view.reset();
                    self.active_table = None;
                    self.rebuild_sidebar();
                }
            }
        }
    }

    fn handle_pin_key(&mut self, key: KeyEvent) -> Action {
        let Some(ref mut prompt) = self.pin_prompt else {
            return Action::None;
        };

        match key.code {
            KeyCode::Esc => {
                self.pin_prompt = None;
                self.focus = Focus::Sidebar;
            }
            KeyCode::Char(c) if c.is_ascii_digit() && prompt.buffer.len() < 4 => {
                prompt.buffer.push(c);
                prompt.error = None;

                if prompt.buffer.len() == 4 {
                    let pin = prompt.buffer.clone();
                    let connection_id = prompt.connection_id;

                    // Try to decrypt the password
                    if let Some(conn) = self.connections.iter().find(|c| c.id == connection_id) {
                        if let (Some(enc), Some(salt), Some(nonce)) = (
                            &conn.encrypted_password,
                            &conn.password_salt,
                            &conn.password_nonce,
                        ) {
                            match rstools_core::crypto::decrypt_with_pin(enc, salt, nonce, &pin) {
                                Ok(password) => {
                                    self.pin_prompt = None;
                                    self.focus = Focus::Sidebar;
                                    self.do_connect(connection_id, password);
                                }
                                Err(_) => {
                                    if let Some(ref mut p) = self.pin_prompt {
                                        p.buffer.clear();
                                        p.error = Some("Wrong PIN".to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }
            KeyCode::Backspace => {
                prompt.buffer.pop();
                prompt.error = None;
            }
            _ => {}
        }
        Action::None
    }

    fn handle_sidebar_key(&mut self, key: KeyEvent) -> Action {
        let action = process_normal_key(key, &mut self.key_state);
        match action {
            Action::MoveDown(_) => {
                self.sidebar_move_down();
                Action::None
            }
            Action::MoveUp(_) => {
                self.sidebar_move_up();
                Action::None
            }
            Action::GotoTop => {
                self.sidebar_selected = 0;
                if !self.sidebar_entries.is_empty() && self.sidebar_entries[0].is_header {
                    if self.sidebar_entries.len() > 1 {
                        self.sidebar_selected = 1;
                    }
                }
                Action::None
            }
            Action::GotoBottom => {
                if !self.sidebar_entries.is_empty() {
                    self.sidebar_selected = self.sidebar_entries.len() - 1;
                }
                Action::None
            }
            Action::Confirm => {
                self.sidebar_confirm();
                Action::None
            }
            Action::Add => {
                // 'a' — add new connection
                self.connection_form = Some(ConnectionForm::new());
                self.editing_connection_id = None;
                self.focus = Focus::ConnectionForm;
                self.mode = InputMode::Insert;
                Action::None
            }
            Action::Edit => {
                // 'e' — edit selected connection
                if self.sidebar_selected < self.sidebar_entries.len() {
                    let entry = &self.sidebar_entries[self.sidebar_selected];
                    if let Some(id_str) = entry.id.strip_prefix("conn:") {
                        if let Ok(id) = id_str.parse::<i64>() {
                            if let Some(conn) = self.connections.iter().find(|c| c.id == id) {
                                self.connection_form =
                                    Some(ConnectionForm::from_connection(conn));
                                self.editing_connection_id = Some(id);
                                self.focus = Focus::ConnectionForm;
                                self.mode = InputMode::Insert;
                            }
                        }
                    }
                }
                Action::None
            }
            Action::Delete => {
                // 'dd' — delete selected connection
                if self.sidebar_selected < self.sidebar_entries.len() {
                    let entry = &self.sidebar_entries[self.sidebar_selected];
                    if let Some(id_str) = entry.id.strip_prefix("conn:") {
                        if let Ok(id) = id_str.parse::<i64>() {
                            let _ = model::delete_connection(&self.conn, id);
                            if self.active_connection_id == Some(id) {
                                self.executor.send(DbCommand::Disconnect);
                            }
                            self.reload_connections();
                            if self.sidebar_selected >= self.sidebar_entries.len() {
                                self.sidebar_selected =
                                    self.sidebar_entries.len().saturating_sub(1);
                            }
                        }
                    }
                }
                Action::None
            }
            Action::None => {
                // Handle 'l' to switch focus to table view
                match key.code {
                    KeyCode::Char('l') | KeyCode::Right => {
                        if self.active_table.is_some() {
                            self.focus = Focus::TableView;
                        }
                        Action::None
                    }
                    KeyCode::Char('D') => {
                        // Disconnect
                        if self.active_connection_id.is_some() {
                            self.executor.send(DbCommand::Disconnect);
                        }
                        Action::None
                    }
                    _ => Action::None,
                }
            }
            other => other,
        }
    }

    fn handle_table_view_key(&mut self, key: KeyEvent) -> Action {
        // Check for focus switch back to sidebar
        match key.code {
            KeyCode::Char('h') | KeyCode::Left
                if key.modifiers == KeyModifiers::NONE && !self.table_view.is_filtering() =>
            {
                self.focus = Focus::Sidebar;
                return Action::None;
            }
            _ => {}
        }

        match self.table_view.handle_key(key) {
            TableAction::None => {}
            TableAction::Refresh => self.refresh_table(),
            TableAction::LoadMore => self.load_more_rows(),
        }
        Action::None
    }

    fn handle_connection_form_key(&mut self, key: KeyEvent) -> Action {
        let Some(ref mut form) = self.connection_form else {
            return Action::None;
        };

        match form.handle_key(key) {
            connection_form::FormAction::None => Action::None,
            connection_form::FormAction::Cancel => {
                self.connection_form = None;
                self.editing_connection_id = None;
                self.focus = Focus::Sidebar;
                self.mode = InputMode::Normal;
                Action::None
            }
            connection_form::FormAction::Save => {
                let form_data = form.to_connection_input();
                let result = if let Some(edit_id) = self.editing_connection_id {
                    model::update_connection(&self.conn, edit_id, &form_data)
                } else {
                    model::add_connection(&self.conn, &form_data).map(|_| ())
                };

                match result {
                    Ok(()) => {
                        self.connection_form = None;
                        self.editing_connection_id = None;
                        self.focus = Focus::Sidebar;
                        self.mode = InputMode::Normal;
                        self.reload_connections();
                        self.status_message = Some("Connection saved".to_string());
                        self.status_shown_at = Some(Instant::now());
                    }
                    Err(e) => {
                        if let Some(ref mut f) = self.connection_form {
                            f.test_result = Some(Err(format!("Save failed: {e}")));
                        }
                    }
                }
                Action::None
            }
            connection_form::FormAction::TestConnection => {
                let form_data = form.to_connection_input();
                self.loading = true;
                self.executor.send(DbCommand::TestConnection {
                    host: form_data.host.clone(),
                    port: form_data.port as u16,
                    database: form_data.database_name.clone(),
                    username: form_data.username.clone(),
                    password: form_data.password.clone(),
                    ssl: form_data.ssl_enabled,
                    ssh_config: if form_data.ssh_enabled {
                        Some(SshTunnelConfig {
                            ssh_host: form_data.ssh_host.clone(),
                            ssh_port: form_data.ssh_port as u16,
                            ssh_username: form_data.ssh_username.clone(),
                            private_key_path: form_data.ssh_private_key_path.clone(),
                            passphrase: if form_data.ssh_passphrase.is_empty() {
                                None
                            } else {
                                Some(form_data.ssh_passphrase.clone())
                            },
                            remote_host: form_data.host.clone(),
                            remote_port: form_data.port as u16,
                        })
                    } else {
                        None
                    },
                });
                Action::None
            }
        }
    }

}

impl Tool for DatabaseTool {
    fn name(&self) -> &str {
        "Database"
    }

    fn description(&self) -> &str {
        "PostgreSQL database browser"
    }

    fn mode(&self) -> InputMode {
        self.mode
    }

    fn init_db(&self, conn: &Connection) -> anyhow::Result<()> {
        model::init_db(conn)
    }

    fn which_key_entries(&self) -> Vec<WhichKeyEntry> {
        vec![
            WhichKeyEntry::action("a", "Add connection"),
            WhichKeyEntry::action("e", "Edit connection"),
            WhichKeyEntry::action("D", "Disconnect"),
        ]
    }

    fn telescope_items(&self) -> Vec<TelescopeItem> {
        self.connections
            .iter()
            .map(|c| TelescopeItem {
                label: c.name.clone(),
                description: format!("{}@{}:{}/{}", c.username, c.host, c.port, c.database_name),
                id: format!("db:{}", c.id),
            })
            .collect()
    }

    fn handle_telescope_selection(&mut self, id: &str) -> bool {
        if let Some(id_str) = id.strip_prefix("db:") {
            if let Ok(id) = id_str.parse::<i64>() {
                self.connect_to(id);
                return true;
            }
        }
        false
    }

    fn help_entries(&self) -> Vec<HelpEntry> {
        vec![
            HelpEntry::with_section("Database", "a", "Add new connection"),
            HelpEntry::with_section("Database", "e", "Edit connection"),
            HelpEntry::with_section("Database", "dd", "Delete connection"),
            HelpEntry::with_section("Database", "Enter", "Connect / select table"),
            HelpEntry::with_section("Database", "D", "Disconnect"),
            HelpEntry::with_section("Database", "h/l", "Switch sidebar/table focus"),
            HelpEntry::with_section("Table View", "j/k", "Navigate rows"),
            HelpEntry::with_section("Table View", "h/l", "Scroll columns"),
            HelpEntry::with_section("Table View", "n/p", "Next/previous page"),
            HelpEntry::with_section("Table View", "s", "Sort by current column"),
            HelpEntry::with_section("Table View", "/", "Filter"),
            HelpEntry::with_section("Table View", "gg/G", "Top/bottom of page"),
        ]
    }

    fn handle_key(&mut self, key: KeyEvent) -> Action {
        // Clear stale messages
        if let Some(shown_at) = self.error_shown_at {
            if shown_at.elapsed().as_secs() > 5 {
                self.error_message = None;
                self.error_shown_at = None;
            }
        }
        if let Some(shown_at) = self.status_shown_at {
            if shown_at.elapsed().as_secs() > 3 {
                self.status_message = None;
                self.status_shown_at = None;
            }
        }

        match self.focus {
            Focus::PinPrompt => self.handle_pin_key(key),
            Focus::ConnectionForm => self.handle_connection_form_key(key),
            Focus::Sidebar => self.handle_sidebar_key(key),
            Focus::TableView => self.handle_table_view_key(key),
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent, area: Rect) -> Action {
        // Basic mouse: click on sidebar vs table view areas
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let sidebar_width = area.width.min(30);
                if mouse.column < area.x + sidebar_width {
                    self.focus = Focus::Sidebar;
                    // Calculate which sidebar entry was clicked
                    let relative_row = mouse.row.saturating_sub(area.y + 1) as usize;
                    let idx = self.sidebar_scroll + relative_row;
                    if idx < self.sidebar_entries.len() && !self.sidebar_entries[idx].is_header {
                        self.sidebar_selected = idx;
                    }
                } else if self.active_table.is_some() {
                    self.focus = Focus::TableView;
                }
            }
            MouseEventKind::ScrollDown => {
                if self.focus == Focus::Sidebar {
                    self.sidebar_move_down();
                } else if self.focus == Focus::TableView {
                    self.table_view.move_down(1);
                }
            }
            MouseEventKind::ScrollUp => {
                if self.focus == Focus::Sidebar {
                    self.sidebar_move_up();
                } else if self.focus == Focus::TableView {
                    self.table_view.move_up(1);
                }
            }
            _ => {}
        }
        Action::None
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        ui::render(self, frame, area);
    }

    fn handle_leader_action(&mut self, key: char) -> Option<Action> {
        match key {
            'a' => {
                self.connection_form = Some(ConnectionForm::new());
                self.editing_connection_id = None;
                self.focus = Focus::ConnectionForm;
                self.mode = InputMode::Insert;
                Some(Action::None)
            }
            'e' => {
                // Edit selected connection
                if self.sidebar_selected < self.sidebar_entries.len() {
                    let entry = &self.sidebar_entries[self.sidebar_selected];
                    if let Some(id_str) = entry.id.strip_prefix("conn:") {
                        if let Ok(id) = id_str.parse::<i64>() {
                            if let Some(conn) = self.connections.iter().find(|c| c.id == id) {
                                self.connection_form =
                                    Some(ConnectionForm::from_connection(conn));
                                self.editing_connection_id = Some(id);
                                self.focus = Focus::ConnectionForm;
                                self.mode = InputMode::Insert;
                            }
                        }
                    }
                }
                Some(Action::None)
            }
            _ => None,
        }
    }

    fn reset_key_state(&mut self) {
        self.key_state.reset();
    }

    fn tick(&mut self) {
        self.poll_results();
        if self.loading {
            self.spinner_frame = (self.spinner_frame + 1) % 4;
        }
    }

    fn handle_command(&mut self, cmd: &str) -> bool {
        match cmd {
            "disconnect" | "dc" => {
                if self.active_connection_id.is_some() {
                    self.executor.send(DbCommand::Disconnect);
                }
                true
            }
            "reset-connections" => {
                // TODO: remove this temporary command
                let _ = self.conn.execute_batch("DROP TABLE IF EXISTS db_connections;");
                let _ = model::init_db(&self.conn);
                self.connections.clear();
                self.rebuild_sidebar();
                self.status_message = Some("db_connections table reset".to_string());
                self.status_shown_at = Some(Instant::now());
                true
            }
            _ => false,
        }
    }

    fn handle_paste(&mut self, text: &str) -> Action {
        if self.focus == Focus::ConnectionForm {
            if let Some(ref mut form) = self.connection_form {
                form.paste(text);
            }
        }
        Action::None
    }

    fn on_focus(&mut self) {
        self.reload_connections();
    }
}
