use anyhow::{Context, Result};
use directories::ProjectDirs;
use rusqlite::Connection;
use std::path::PathBuf;

/// Returns the path to the shared rstools database.
/// Location: `~/.local/share/rstools/rstools.db` (XDG-compliant)
pub fn db_path() -> Result<PathBuf> {
    let dirs =
        ProjectDirs::from("", "", "rstools").context("Could not determine data directory")?;
    let data_dir = dirs.data_dir();
    std::fs::create_dir_all(data_dir)
        .with_context(|| format!("Failed to create data directory: {}", data_dir.display()))?;
    Ok(data_dir.join("rstools.db"))
}

/// Opens (or creates) the shared SQLite database and returns the connection.
/// Enables WAL mode for better concurrent read performance.
pub fn open_db() -> Result<Connection> {
    let path = db_path()?;
    let conn = Connection::open(&path)
        .with_context(|| format!("Failed to open database at {}", path.display()))?;

    // Enable WAL mode for better performance
    conn.pragma_update(None, "journal_mode", "WAL")?;
    // Enable foreign keys
    conn.pragma_update(None, "foreign_keys", "ON")?;

    Ok(conn)
}

/// Open an in-memory database for testing.
pub fn open_memory_db() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(conn)
}
