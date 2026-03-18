use crossterm::event::{KeyCode, KeyEvent};

use crate::model::{DbConnection, DbConnectionInput};

// ── Form fields ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FormField {
    Name,
    Host,
    Port,
    Database,
    Username,
    Password,
    SslEnabled,
    SshEnabled,
    SshHost,
    SshPort,
    SshUsername,
    SshPrivateKeyPath,
    SshPassphrase,
    TestButton,
    SaveButton,
}

const FIELDS: &[FormField] = &[
    FormField::Name,
    FormField::Host,
    FormField::Port,
    FormField::Database,
    FormField::Username,
    FormField::Password,
    FormField::SslEnabled,
    FormField::SshEnabled,
    FormField::SshHost,
    FormField::SshPort,
    FormField::SshUsername,
    FormField::SshPrivateKeyPath,
    FormField::SshPassphrase,
    FormField::TestButton,
    FormField::SaveButton,
];

impl FormField {
    pub fn label(&self) -> &str {
        match self {
            Self::Name => "Name",
            Self::Host => "Host",
            Self::Port => "Port",
            Self::Database => "Database",
            Self::Username => "Username",
            Self::Password => "Password",
            Self::SslEnabled => "SSL",
            Self::SshEnabled => "SSH Tunnel",
            Self::SshHost => "SSH Host",
            Self::SshPort => "SSH Port",
            Self::SshUsername => "SSH Username",
            Self::SshPrivateKeyPath => "SSH Key Path",
            Self::SshPassphrase => "SSH Passphrase",
            Self::TestButton => "[ Test Connection ]",
            Self::SaveButton => "[ Save ]",
        }
    }

    pub fn is_toggle(&self) -> bool {
        matches!(self, Self::SslEnabled | Self::SshEnabled)
    }

    pub fn is_button(&self) -> bool {
        matches!(self, Self::TestButton | Self::SaveButton)
    }

    pub fn is_masked(&self) -> bool {
        matches!(self, Self::Password | Self::SshPassphrase)
    }

    pub fn is_ssh_field(&self) -> bool {
        matches!(
            self,
            Self::SshHost
                | Self::SshPort
                | Self::SshUsername
                | Self::SshPrivateKeyPath
                | Self::SshPassphrase
        )
    }
}

// ── Form action ─────────────────────────────────────────────────────

pub enum FormAction {
    None,
    Cancel,
    Save,
    TestConnection,
}

// ── Connection form ─────────────────────────────────────────────────

pub struct ConnectionForm {
    pub name: String,
    pub host: String,
    pub port: String,
    pub database: String,
    pub username: String,
    pub password: String,
    pub ssl_enabled: bool,
    pub ssh_enabled: bool,
    pub ssh_host: String,
    pub ssh_port: String,
    pub ssh_username: String,
    pub ssh_private_key_path: String,
    pub ssh_passphrase: String,

    pub focused_field: usize,
    pub cursor_pos: usize,
    pub test_result: Option<Result<String, String>>,
}

impl ConnectionForm {
    pub fn new() -> Self {
        Self {
            name: String::new(),
            host: "localhost".to_string(),
            port: "5432".to_string(),
            database: String::new(),
            username: String::new(),
            password: String::new(),
            ssl_enabled: false,
            ssh_enabled: false,
            ssh_host: String::new(),
            ssh_port: "22".to_string(),
            ssh_username: String::new(),
            ssh_private_key_path: "~/.ssh/id_rsa".to_string(),
            ssh_passphrase: String::new(),
            focused_field: 0,
            cursor_pos: 0,
            test_result: None,
        }
    }

    pub fn from_connection(conn: &DbConnection) -> Self {
        Self {
            name: conn.name.clone(),
            host: conn.host.clone(),
            port: conn.port.to_string(),
            database: conn.database_name.clone(),
            username: conn.username.clone(),
            password: String::new(), // Don't pre-fill passwords
            ssl_enabled: conn.ssl_enabled,
            ssh_enabled: conn.ssh_enabled,
            ssh_host: conn.ssh_host.clone().unwrap_or_default(),
            ssh_port: conn.ssh_port.map(|p| p.to_string()).unwrap_or_else(|| "22".to_string()),
            ssh_username: conn.ssh_username.clone().unwrap_or_default(),
            ssh_private_key_path: conn
                .ssh_private_key_path
                .clone()
                .unwrap_or_else(|| "~/.ssh/id_rsa".to_string()),
            ssh_passphrase: String::new(),
            focused_field: 0,
            cursor_pos: 0,
            test_result: None,
        }
    }

    pub fn current_field(&self) -> FormField {
        self.visible_fields()[self.focused_field.min(self.visible_fields().len() - 1)]
    }

    pub fn visible_fields(&self) -> Vec<FormField> {
        FIELDS
            .iter()
            .filter(|f| {
                if f.is_ssh_field() {
                    self.ssh_enabled
                } else {
                    true
                }
            })
            .copied()
            .collect()
    }

    fn field_value(&self, field: FormField) -> &str {
        match field {
            FormField::Name => &self.name,
            FormField::Host => &self.host,
            FormField::Port => &self.port,
            FormField::Database => &self.database,
            FormField::Username => &self.username,
            FormField::Password => &self.password,
            FormField::SshHost => &self.ssh_host,
            FormField::SshPort => &self.ssh_port,
            FormField::SshUsername => &self.ssh_username,
            FormField::SshPrivateKeyPath => &self.ssh_private_key_path,
            FormField::SshPassphrase => &self.ssh_passphrase,
            _ => "",
        }
    }

    fn field_value_mut(&mut self, field: FormField) -> Option<&mut String> {
        match field {
            FormField::Name => Some(&mut self.name),
            FormField::Host => Some(&mut self.host),
            FormField::Port => Some(&mut self.port),
            FormField::Database => Some(&mut self.database),
            FormField::Username => Some(&mut self.username),
            FormField::Password => Some(&mut self.password),
            FormField::SshHost => Some(&mut self.ssh_host),
            FormField::SshPort => Some(&mut self.ssh_port),
            FormField::SshUsername => Some(&mut self.ssh_username),
            FormField::SshPrivateKeyPath => Some(&mut self.ssh_private_key_path),
            FormField::SshPassphrase => Some(&mut self.ssh_passphrase),
            _ => None,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> FormAction {
        let field = self.current_field();

        match key.code {
            KeyCode::Esc => return FormAction::Cancel,
            KeyCode::Tab | KeyCode::Down => {
                let visible = self.visible_fields();
                if self.focused_field + 1 < visible.len() {
                    self.focused_field += 1;
                }
                self.cursor_pos = self.field_value(self.current_field()).len();
                return FormAction::None;
            }
            KeyCode::BackTab | KeyCode::Up => {
                if self.focused_field > 0 {
                    self.focused_field -= 1;
                }
                self.cursor_pos = self.field_value(self.current_field()).len();
                return FormAction::None;
            }
            _ => {}
        }

        // Handle toggles
        if field.is_toggle() {
            match key.code {
                KeyCode::Enter | KeyCode::Char(' ') => {
                    match field {
                        FormField::SslEnabled => self.ssl_enabled = !self.ssl_enabled,
                        FormField::SshEnabled => {
                            self.ssh_enabled = !self.ssh_enabled;
                            // Adjust focused_field if SSH fields disappeared
                            let visible = self.visible_fields();
                            if self.focused_field >= visible.len() {
                                self.focused_field = visible.len() - 1;
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
            return FormAction::None;
        }

        // Handle buttons
        if field.is_button() {
            if key.code == KeyCode::Enter {
                return match field {
                    FormField::TestButton => FormAction::TestConnection,
                    FormField::SaveButton => FormAction::Save,
                    _ => FormAction::None,
                };
            }
            return FormAction::None;
        }

        // Text input — use cursor_pos local copy to avoid borrow conflicts
        let pos = self.cursor_pos;
        match key.code {
            KeyCode::Char(c) => {
                if let Some(val) = self.field_value_mut(field) {
                    val.insert(pos, c);
                }
                self.cursor_pos = pos + c.len_utf8();
            }
            KeyCode::Backspace => {
                if pos > 0 {
                    if let Some(val) = self.field_value_mut(field) {
                        let prev = val[..pos]
                            .char_indices()
                            .next_back()
                            .map(|(i, _)| i)
                            .unwrap_or(0);
                        val.drain(prev..pos);
                        self.cursor_pos = prev;
                    }
                }
            }
            KeyCode::Delete => {
                if let Some(val) = self.field_value_mut(field) {
                    if pos < val.len() {
                        let next = val[pos..]
                            .char_indices()
                            .nth(1)
                            .map(|(i, _)| pos + i)
                            .unwrap_or(val.len());
                        val.drain(pos..next);
                    }
                }
            }
            KeyCode::Left => {
                if pos > 0 {
                    let val = self.field_value(field);
                    self.cursor_pos = val[..pos]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                }
            }
            KeyCode::Right => {
                let val = self.field_value(field);
                let len = val.len();
                if pos < len {
                    self.cursor_pos = val[pos..]
                        .char_indices()
                        .nth(1)
                        .map(|(i, _)| pos + i)
                        .unwrap_or(len);
                }
            }
            KeyCode::Home => {
                self.cursor_pos = 0;
            }
            KeyCode::End => {
                self.cursor_pos = self.field_value(field).len();
            }
            KeyCode::Enter => {
                // Move to next field on Enter (for text inputs)
                let visible = self.visible_fields();
                if self.focused_field + 1 < visible.len() {
                    self.focused_field += 1;
                    self.cursor_pos = self.field_value(self.current_field()).len();
                }
            }
            _ => {}
        }

        FormAction::None
    }

    pub fn paste(&mut self, text: &str) {
        let field = self.current_field();
        let pos = self.cursor_pos;
        let clean: String = text.chars().filter(|c| *c != '\n' && *c != '\r').collect();
        if let Some(val) = self.field_value_mut(field) {
            val.insert_str(pos, &clean);
        }
        self.cursor_pos = pos + clean.len();
    }

    pub fn to_connection_input(&self) -> DbConnectionInput {
        DbConnectionInput {
            name: self.name.clone(),
            host: if self.host.is_empty() {
                "localhost".to_string()
            } else {
                self.host.clone()
            },
            port: self.port.parse().unwrap_or(5432),
            database_name: self.database.clone(),
            username: self.username.clone(),
            password: self.password.clone(),
            ssl_enabled: self.ssl_enabled,
            ssh_enabled: self.ssh_enabled,
            ssh_host: self.ssh_host.clone(),
            ssh_port: self.ssh_port.parse().unwrap_or(22),
            ssh_username: self.ssh_username.clone(),
            ssh_private_key_path: self.ssh_private_key_path.clone(),
            ssh_passphrase: self.ssh_passphrase.clone(),
        }
    }

    /// For rendering: get the display value for a field.
    pub fn display_value(&self, field: FormField) -> String {
        if field.is_toggle() {
            let on = match field {
                FormField::SslEnabled => self.ssl_enabled,
                FormField::SshEnabled => self.ssh_enabled,
                _ => false,
            };
            return if on { "[x]".to_string() } else { "[ ]".to_string() };
        }

        if field.is_button() {
            return field.label().to_string();
        }

        let val = self.field_value(field);
        if field.is_masked() && !val.is_empty() {
            "*".repeat(val.len())
        } else {
            val.to_string()
        }
    }
}
