use anyhow::Result;
use rusqlite::Connection;

// ── Data model ───────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct DbConnection {
    pub id: i64,
    pub name: String,
    pub provider: String,
    pub host: String,
    pub port: i32,
    pub database_name: String,
    pub username: String,
    /// Plaintext password (stored directly; use encrypted_password for PIN-protected storage).
    pub password: String,
    // Encrypted password fields (optional PIN-based encryption)
    pub encrypted_password: Option<String>,
    pub password_salt: Option<String>,
    pub password_nonce: Option<String>,
    pub password_expires_at: Option<String>,
    pub ssl_enabled: bool,
    // SSH tunnel config
    pub ssh_enabled: bool,
    pub ssh_host: Option<String>,
    pub ssh_port: Option<i32>,
    pub ssh_username: Option<String>,
    pub ssh_private_key_path: Option<String>,
    // Encrypted SSH passphrase
    pub ssh_encrypted_passphrase: Option<String>,
    pub ssh_passphrase_salt: Option<String>,
    pub ssh_passphrase_nonce: Option<String>,
    pub ssh_passphrase_expires_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Form data before encryption.
#[derive(Debug, Clone, Default)]
pub struct DbConnectionInput {
    pub name: String,
    pub host: String,
    pub port: i32,
    pub database_name: String,
    pub username: String,
    pub password: String,
    pub ssl_enabled: bool,
    pub ssh_enabled: bool,
    pub ssh_host: String,
    pub ssh_port: i32,
    pub ssh_username: String,
    pub ssh_private_key_path: String,
    pub ssh_passphrase: String,
}

// ── Database schema ──────────────────────────────────────────────────

pub fn init_db(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS db_connections (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            provider TEXT NOT NULL DEFAULT 'postgresql',
            host TEXT NOT NULL DEFAULT 'localhost',
            port INTEGER NOT NULL DEFAULT 5432,
            database_name TEXT NOT NULL DEFAULT '',
            username TEXT NOT NULL DEFAULT '',
            password TEXT NOT NULL DEFAULT '',
            encrypted_password TEXT,
            password_salt TEXT,
            password_nonce TEXT,
            password_expires_at TEXT,
            ssl_enabled INTEGER NOT NULL DEFAULT 0,
            ssh_enabled INTEGER NOT NULL DEFAULT 0,
            ssh_host TEXT,
            ssh_port INTEGER DEFAULT 22,
            ssh_username TEXT,
            ssh_private_key_path TEXT,
            ssh_encrypted_passphrase TEXT,
            ssh_passphrase_salt TEXT,
            ssh_passphrase_nonce TEXT,
            ssh_passphrase_expires_at TEXT,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TRIGGER IF NOT EXISTS db_connections_updated_at
        AFTER UPDATE ON db_connections
        BEGIN
            UPDATE db_connections SET updated_at = CURRENT_TIMESTAMP WHERE id = NEW.id;
        END;",
    )?;

    // Migration: add password column if missing (table existed before this column was added)
    let has_password_col: bool = conn
        .prepare("SELECT COUNT(*) FROM pragma_table_info('db_connections') WHERE name = 'password'")?
        .query_row([], |row| row.get::<_, i64>(0))
        .map(|count| count > 0)?;
    if !has_password_col {
        conn.execute_batch("ALTER TABLE db_connections ADD COLUMN password TEXT NOT NULL DEFAULT '';")?;
    }

    Ok(())
}

// ── CRUD operations ──────────────────────────────────────────────────

pub fn list_connections(conn: &Connection) -> Result<Vec<DbConnection>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, provider, host, port, database_name, username, password,
                encrypted_password, password_salt, password_nonce, password_expires_at,
                ssl_enabled, ssh_enabled, ssh_host, ssh_port, ssh_username, ssh_private_key_path,
                ssh_encrypted_passphrase, ssh_passphrase_salt, ssh_passphrase_nonce,
                ssh_passphrase_expires_at, created_at, updated_at
         FROM db_connections
         ORDER BY name ASC",
    )?;

    let connections = stmt
        .query_map([], |row| {
            Ok(DbConnection {
                id: row.get(0)?,
                name: row.get(1)?,
                provider: row.get(2)?,
                host: row.get(3)?,
                port: row.get(4)?,
                database_name: row.get(5)?,
                username: row.get(6)?,
                password: row.get(7)?,
                encrypted_password: row.get(8)?,
                password_salt: row.get(9)?,
                password_nonce: row.get(10)?,
                password_expires_at: row.get(11)?,
                ssl_enabled: row.get::<_, i32>(12)? != 0,
                ssh_enabled: row.get::<_, i32>(13)? != 0,
                ssh_host: row.get(14)?,
                ssh_port: row.get(15)?,
                ssh_username: row.get(16)?,
                ssh_private_key_path: row.get(17)?,
                ssh_encrypted_passphrase: row.get(18)?,
                ssh_passphrase_salt: row.get(19)?,
                ssh_passphrase_nonce: row.get(20)?,
                ssh_passphrase_expires_at: row.get(21)?,
                created_at: row.get(22)?,
                updated_at: row.get(23)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    Ok(connections)
}

pub fn add_connection(conn: &Connection, input: &DbConnectionInput) -> Result<i64> {
    conn.execute(
        "INSERT INTO db_connections (name, host, port, database_name, username, password,
                ssl_enabled, ssh_enabled, ssh_host, ssh_port, ssh_username, ssh_private_key_path)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        rusqlite::params![
            input.name,
            input.host,
            input.port,
            input.database_name,
            input.username,
            input.password,
            input.ssl_enabled as i32,
            input.ssh_enabled as i32,
            if input.ssh_host.is_empty() {
                None
            } else {
                Some(&input.ssh_host)
            },
            if input.ssh_enabled {
                Some(input.ssh_port)
            } else {
                None
            },
            if input.ssh_username.is_empty() {
                None
            } else {
                Some(&input.ssh_username)
            },
            if input.ssh_private_key_path.is_empty() {
                None
            } else {
                Some(&input.ssh_private_key_path)
            },
        ],
    )?;

    let id = conn.last_insert_rowid();
    Ok(id)
}

pub fn update_connection(conn: &Connection, id: i64, input: &DbConnectionInput) -> Result<()> {
    conn.execute(
        "UPDATE db_connections SET
            name = ?1, host = ?2, port = ?3, database_name = ?4, username = ?5,
            password = ?6, ssl_enabled = ?7, ssh_enabled = ?8, ssh_host = ?9, ssh_port = ?10,
            ssh_username = ?11, ssh_private_key_path = ?12
         WHERE id = ?13",
        rusqlite::params![
            input.name,
            input.host,
            input.port,
            input.database_name,
            input.username,
            input.password,
            input.ssl_enabled as i32,
            input.ssh_enabled as i32,
            if input.ssh_host.is_empty() {
                None
            } else {
                Some(&input.ssh_host)
            },
            if input.ssh_enabled {
                Some(input.ssh_port)
            } else {
                None
            },
            if input.ssh_username.is_empty() {
                None
            } else {
                Some(&input.ssh_username)
            },
            if input.ssh_private_key_path.is_empty() {
                None
            } else {
                Some(&input.ssh_private_key_path)
            },
            id,
        ],
    )?;
    Ok(())
}

pub fn store_password(
    conn: &Connection,
    connection_id: i64,
    encrypted_password: &str,
    salt: &str,
    nonce: &str,
    expires_at: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE db_connections
         SET encrypted_password = ?1, password_salt = ?2, password_nonce = ?3, password_expires_at = ?4
         WHERE id = ?5",
        rusqlite::params![encrypted_password, salt, nonce, expires_at, connection_id],
    )?;
    Ok(())
}

pub fn delete_connection(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM db_connections WHERE id = ?1", [id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        init_db(&conn).unwrap();
        conn
    }

    #[test]
    fn test_crud() {
        let conn = test_conn();

        let input = DbConnectionInput {
            name: "test-db".to_string(),
            host: "localhost".to_string(),
            port: 5432,
            database_name: "mydb".to_string(),
            username: "user".to_string(),
            ..Default::default()
        };

        let id = add_connection(&conn, &input).unwrap();
        assert!(id > 0);

        let list = list_connections(&conn).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "test-db");
        assert_eq!(list[0].host, "localhost");

        let updated = DbConnectionInput {
            name: "prod-db".to_string(),
            ..input.clone()
        };
        update_connection(&conn, id, &updated).unwrap();

        let list = list_connections(&conn).unwrap();
        assert_eq!(list[0].name, "prod-db");

        delete_connection(&conn, id).unwrap();
        let list = list_connections(&conn).unwrap();
        assert!(list.is_empty());
    }
}
