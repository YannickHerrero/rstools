use anyhow::Result;
use rusqlite::Connection;

// ── Data model ───────────────────────────────────────────────────────

/// A previously opened KeePass file tracked in the sidebar history.
#[derive(Debug, Clone)]
pub struct KeePassFile {
    pub id: i64,
    /// Absolute path to the .kdbx file.
    pub file_path: String,
    /// Display name (derived from filename).
    pub display_name: String,
    /// Whether a PIN is stored for quick access.
    pub has_pin: bool,
    /// Encrypted master password (base64), if PIN is set.
    pub encrypted_password: Option<String>,
    /// Salt used for PIN key derivation (base64).
    pub pin_salt: Option<String>,
    /// AES-GCM nonce used for encryption (base64).
    pub pin_nonce: Option<String>,
    /// When the PIN expires (ISO 8601 string).
    pub pin_expires_at: Option<String>,
    /// When the file was last opened.
    pub last_opened_at: String,
    /// When the record was created.
    pub created_at: String,
}

// ── Database schema ──────────────────────────────────────────────────

pub fn init_db(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS keepass_files (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            file_path TEXT NOT NULL UNIQUE,
            display_name TEXT NOT NULL,
            encrypted_password TEXT,
            pin_salt TEXT,
            pin_nonce TEXT,
            pin_expires_at TEXT,
            last_opened_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TRIGGER IF NOT EXISTS keepass_files_updated_at
        AFTER UPDATE ON keepass_files
        BEGIN
            UPDATE keepass_files SET updated_at = CURRENT_TIMESTAMP WHERE id = NEW.id;
        END;",
    )?;
    Ok(())
}

// ── CRUD operations ──────────────────────────────────────────────────

/// List all tracked KeePass files, ordered by most recently opened first.
pub fn list_files(conn: &Connection) -> Result<Vec<KeePassFile>> {
    let mut stmt = conn.prepare(
        "SELECT id, file_path, display_name, encrypted_password, pin_salt, pin_nonce,
                pin_expires_at, last_opened_at, created_at
         FROM keepass_files
         ORDER BY last_opened_at DESC",
    )?;

    let files = stmt
        .query_map([], |row| {
            let encrypted_password: Option<String> = row.get(3)?;
            let pin_salt: Option<String> = row.get(4)?;
            let pin_nonce: Option<String> = row.get(5)?;
            let pin_expires_at: Option<String> = row.get(6)?;

            let has_pin = encrypted_password.is_some() && pin_salt.is_some() && pin_nonce.is_some();

            Ok(KeePassFile {
                id: row.get(0)?,
                file_path: row.get(1)?,
                display_name: row.get(2)?,
                has_pin,
                encrypted_password,
                pin_salt,
                pin_nonce,
                pin_expires_at,
                last_opened_at: row.get(7)?,
                created_at: row.get(8)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    Ok(files)
}

/// Get a file by its path.
pub fn get_file_by_path(conn: &Connection, path: &str) -> Result<Option<KeePassFile>> {
    let mut stmt = conn.prepare(
        "SELECT id, file_path, display_name, encrypted_password, pin_salt, pin_nonce,
                pin_expires_at, last_opened_at, created_at
         FROM keepass_files
         WHERE file_path = ?1",
    )?;

    let file = stmt
        .query_row([path], |row| {
            let encrypted_password: Option<String> = row.get(3)?;
            let pin_salt: Option<String> = row.get(4)?;
            let pin_nonce: Option<String> = row.get(5)?;
            let pin_expires_at: Option<String> = row.get(6)?;

            let has_pin = encrypted_password.is_some() && pin_salt.is_some() && pin_nonce.is_some();

            Ok(KeePassFile {
                id: row.get(0)?,
                file_path: row.get(1)?,
                display_name: row.get(2)?,
                has_pin,
                encrypted_password,
                pin_salt,
                pin_nonce,
                pin_expires_at,
                last_opened_at: row.get(7)?,
                created_at: row.get(8)?,
            })
        })
        .ok();

    Ok(file)
}

/// Insert or update a file in the history (upsert by file_path).
/// Updates last_opened_at on conflict.
pub fn upsert_file(conn: &Connection, file_path: &str, display_name: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO keepass_files (file_path, display_name, last_opened_at)
         VALUES (?1, ?2, CURRENT_TIMESTAMP)
         ON CONFLICT(file_path) DO UPDATE SET
            display_name = excluded.display_name,
            last_opened_at = CURRENT_TIMESTAMP",
        rusqlite::params![file_path, display_name],
    )?;

    let id = conn.last_insert_rowid();
    // If it was an update, last_insert_rowid may be 0; query for the actual id.
    if id == 0 {
        let actual_id: i64 = conn.query_row(
            "SELECT id FROM keepass_files WHERE file_path = ?1",
            [file_path],
            |row| row.get(0),
        )?;
        Ok(actual_id)
    } else {
        Ok(id)
    }
}

/// Store the encrypted PIN data for a file.
pub fn store_pin(
    conn: &Connection,
    file_id: i64,
    encrypted_password: &str,
    pin_salt: &str,
    pin_nonce: &str,
    expires_at: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE keepass_files
         SET encrypted_password = ?1, pin_salt = ?2, pin_nonce = ?3, pin_expires_at = ?4
         WHERE id = ?5",
        rusqlite::params![encrypted_password, pin_salt, pin_nonce, expires_at, file_id],
    )?;
    Ok(())
}

/// Clear the PIN data for a file (expired or user-requested).
pub fn clear_pin(conn: &Connection, file_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE keepass_files
         SET encrypted_password = NULL, pin_salt = NULL, pin_nonce = NULL, pin_expires_at = NULL
         WHERE id = ?1",
        [file_id],
    )?;
    Ok(())
}

/// Remove a file from the history entirely.
pub fn delete_file(conn: &Connection, file_id: i64) -> Result<()> {
    conn.execute("DELETE FROM keepass_files WHERE id = ?1", [file_id])?;
    Ok(())
}

/// Update the last_opened_at timestamp for a file.
pub fn touch_file(conn: &Connection, file_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE keepass_files SET last_opened_at = CURRENT_TIMESTAMP WHERE id = ?1",
        [file_id],
    )?;
    Ok(())
}
