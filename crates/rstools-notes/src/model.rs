use anyhow::Result;
use rusqlite::Connection;

// ── Entry types ──────────────────────────────────────────────────────

/// Entry type: folder or note (like directory vs file in neo-tree).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryType {
    Folder,
    Note,
}

impl EntryType {
    pub fn as_str(&self) -> &str {
        match self {
            EntryType::Folder => "folder",
            EntryType::Note => "note",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "folder" => Some(EntryType::Folder),
            "note" => Some(EntryType::Note),
            _ => None,
        }
    }
}

// ── Data models ──────────────────────────────────────────────────────

/// A single entry in the notes tree (folder or note).
#[derive(Debug, Clone)]
pub struct NoteEntry {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub entry_type: EntryType,
    pub expanded: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// The content of a note (one-to-one with a note entry).
#[derive(Debug, Clone)]
pub struct NoteContent {
    pub id: i64,
    pub entry_id: i64,
    pub body: String,
    pub created_at: String,
    pub updated_at: String,
}

// ── Database ─────────────────────────────────────────────────────────

/// Initialize the database tables for the Notes tool.
pub fn init_db(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS note_entries (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            parent_id INTEGER REFERENCES note_entries(id) ON DELETE CASCADE,
            name TEXT NOT NULL,
            entry_type TEXT NOT NULL CHECK(entry_type IN ('folder', 'note')),
            expanded INTEGER NOT NULL DEFAULT 0,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TRIGGER IF NOT EXISTS note_entries_updated_at
        AFTER UPDATE ON note_entries
        BEGIN
            UPDATE note_entries SET updated_at = CURRENT_TIMESTAMP WHERE id = NEW.id;
        END;

        CREATE TABLE IF NOT EXISTS note_contents (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            entry_id INTEGER UNIQUE NOT NULL REFERENCES note_entries(id) ON DELETE CASCADE,
            body TEXT NOT NULL DEFAULT '',
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TRIGGER IF NOT EXISTS note_contents_updated_at
        AFTER UPDATE ON note_contents
        BEGIN
            UPDATE note_contents SET updated_at = CURRENT_TIMESTAMP WHERE id = NEW.id;
        END;",
    )?;
    Ok(())
}

// ── CRUD operations ──────────────────────────────────────────────────

/// List all entries from the database.
pub fn list_entries(conn: &Connection) -> Result<Vec<NoteEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, parent_id, name, entry_type, expanded, created_at, updated_at
         FROM note_entries
         ORDER BY entry_type ASC, name ASC",
    )?;
    let entries = stmt
        .query_map([], |row| {
            let entry_type_str: String = row.get(3)?;
            Ok(NoteEntry {
                id: row.get(0)?,
                parent_id: row.get(1)?,
                name: row.get(2)?,
                entry_type: EntryType::from_str(&entry_type_str).unwrap_or(EntryType::Note),
                expanded: row.get::<_, i64>(4)? != 0,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(entries)
}

/// Add a new entry to the database. Returns the new entry's ID.
pub fn add_entry(
    conn: &Connection,
    parent_id: Option<i64>,
    name: &str,
    entry_type: EntryType,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO note_entries (parent_id, name, entry_type) VALUES (?1, ?2, ?3)",
        rusqlite::params![parent_id, name, entry_type.as_str()],
    )?;
    let entry_id = conn.last_insert_rowid();

    // Auto-create note content row for notes
    if entry_type == EntryType::Note {
        conn.execute(
            "INSERT INTO note_contents (entry_id, body) VALUES (?1, '')",
            rusqlite::params![entry_id],
        )?;
    }

    Ok(entry_id)
}

/// Update the expanded state of a folder entry.
pub fn set_entry_expanded(conn: &Connection, id: i64, expanded: bool) -> Result<()> {
    conn.execute(
        "UPDATE note_entries SET expanded = ?1 WHERE id = ?2",
        rusqlite::params![expanded as i64, id],
    )?;
    Ok(())
}

/// Rename an entry.
pub fn rename_entry(conn: &Connection, id: i64, new_name: &str) -> Result<()> {
    conn.execute(
        "UPDATE note_entries SET name = ?1 WHERE id = ?2",
        rusqlite::params![new_name, id],
    )?;
    Ok(())
}

/// Delete an entry (and all children via CASCADE).
pub fn delete_entry(conn: &Connection, id: i64) -> Result<()> {
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    conn.execute(
        "DELETE FROM note_entries WHERE id = ?1",
        rusqlite::params![id],
    )?;
    Ok(())
}

/// Move an entry to a new parent.
pub fn move_entry(conn: &Connection, id: i64, new_parent_id: Option<i64>) -> Result<()> {
    conn.execute(
        "UPDATE note_entries SET parent_id = ?1 WHERE id = ?2",
        rusqlite::params![new_parent_id, id],
    )?;
    Ok(())
}

/// Recursively copy an entry and all its children to a new parent.
/// Returns the ID of the newly created root copy.
pub fn copy_entry_recursive(
    conn: &Connection,
    source_id: i64,
    new_parent_id: Option<i64>,
) -> Result<i64> {
    let source: NoteEntry = conn.query_row(
        "SELECT id, parent_id, name, entry_type, expanded, created_at, updated_at
         FROM note_entries WHERE id = ?1",
        rusqlite::params![source_id],
        |row| {
            let entry_type_str: String = row.get(3)?;
            Ok(NoteEntry {
                id: row.get(0)?,
                parent_id: row.get(1)?,
                name: row.get(2)?,
                entry_type: EntryType::from_str(&entry_type_str).unwrap_or(EntryType::Note),
                expanded: row.get::<_, i64>(4)? != 0,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        },
    )?;

    let new_id = add_entry(conn, new_parent_id, &source.name, source.entry_type)?;

    // If the source is a note, copy its content
    if source.entry_type == EntryType::Note {
        if let Ok(content) = get_note_content(conn, source_id) {
            save_note_content(conn, new_id, &content.body)?;
        }
    }

    // Recursively copy children
    let children: Vec<i64> = {
        let mut stmt = conn.prepare("SELECT id FROM note_entries WHERE parent_id = ?1")?;
        stmt.query_map(rusqlite::params![source_id], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?
    };

    for child_id in children {
        copy_entry_recursive(conn, child_id, Some(new_id))?;
    }

    Ok(new_id)
}

/// Get the content of a note by its entry ID.
pub fn get_note_content(conn: &Connection, entry_id: i64) -> Result<NoteContent> {
    let content = conn.query_row(
        "SELECT id, entry_id, body, created_at, updated_at
         FROM note_contents WHERE entry_id = ?1",
        rusqlite::params![entry_id],
        |row| {
            Ok(NoteContent {
                id: row.get(0)?,
                entry_id: row.get(1)?,
                body: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        },
    )?;
    Ok(content)
}

/// Save (update) the content of a note.
pub fn save_note_content(conn: &Connection, entry_id: i64, body: &str) -> Result<()> {
    conn.execute(
        "UPDATE note_contents SET body = ?1 WHERE entry_id = ?2",
        rusqlite::params![body, entry_id],
    )?;
    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rstools_core::db::open_memory_db;

    fn setup_db() -> Connection {
        let conn = open_memory_db().unwrap();
        init_db(&conn).unwrap();
        conn
    }

    #[test]
    fn test_init_db() {
        let conn = setup_db();
        let entries = list_entries(&conn).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_add_and_list_entries() {
        let conn = setup_db();

        let folder_id = add_entry(&conn, None, "My Folder", EntryType::Folder).unwrap();
        let note_id = add_entry(&conn, Some(folder_id), "My Note", EntryType::Note).unwrap();

        let entries = list_entries(&conn).unwrap();
        assert_eq!(entries.len(), 2);

        // Folders come first (sorted by entry_type ASC: folder < note)
        assert_eq!(entries[0].name, "My Folder");
        assert_eq!(entries[0].entry_type, EntryType::Folder);
        assert_eq!(entries[1].name, "My Note");
        assert_eq!(entries[1].entry_type, EntryType::Note);
        assert_eq!(entries[1].parent_id, Some(folder_id));

        // Note should have auto-created content
        let content = get_note_content(&conn, note_id).unwrap();
        assert_eq!(content.body, "");
    }

    #[test]
    fn test_note_content() {
        let conn = setup_db();

        let note_id = add_entry(&conn, None, "Test Note", EntryType::Note).unwrap();
        save_note_content(&conn, note_id, "Hello, world!").unwrap();

        let content = get_note_content(&conn, note_id).unwrap();
        assert_eq!(content.body, "Hello, world!");
    }

    #[test]
    fn test_rename_entry() {
        let conn = setup_db();

        let id = add_entry(&conn, None, "Old Name", EntryType::Note).unwrap();
        rename_entry(&conn, id, "New Name").unwrap();

        let entries = list_entries(&conn).unwrap();
        assert_eq!(entries[0].name, "New Name");
    }

    #[test]
    fn test_delete_cascade() {
        let conn = setup_db();

        let folder_id = add_entry(&conn, None, "Folder", EntryType::Folder).unwrap();
        add_entry(&conn, Some(folder_id), "Note", EntryType::Note).unwrap();

        delete_entry(&conn, folder_id).unwrap();

        let entries = list_entries(&conn).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_copy_recursive() {
        let conn = setup_db();

        let folder_id = add_entry(&conn, None, "Folder", EntryType::Folder).unwrap();
        let note_id = add_entry(&conn, Some(folder_id), "Note", EntryType::Note).unwrap();
        save_note_content(&conn, note_id, "Content here").unwrap();

        let copied_id = copy_entry_recursive(&conn, folder_id, None).unwrap();

        let entries = list_entries(&conn).unwrap();
        // Original folder + note + copied folder + copied note = 4
        assert_eq!(entries.len(), 4);

        // Find the copied note (child of copied folder)
        let copied_note = entries
            .iter()
            .find(|e| e.parent_id == Some(copied_id) && e.entry_type == EntryType::Note)
            .unwrap();
        let copied_content = get_note_content(&conn, copied_note.id).unwrap();
        assert_eq!(copied_content.body, "Content here");
    }

    #[test]
    fn test_expanded_state() {
        let conn = setup_db();

        let id = add_entry(&conn, None, "Folder", EntryType::Folder).unwrap();
        let entries = list_entries(&conn).unwrap();
        assert!(!entries[0].expanded);

        set_entry_expanded(&conn, id, true).unwrap();
        let entries = list_entries(&conn).unwrap();
        assert!(entries[0].expanded);
    }

    #[test]
    fn test_move_entry() {
        let conn = setup_db();

        let folder_a = add_entry(&conn, None, "A", EntryType::Folder).unwrap();
        let folder_b = add_entry(&conn, None, "B", EntryType::Folder).unwrap();
        let note_id = add_entry(&conn, Some(folder_a), "Note", EntryType::Note).unwrap();

        move_entry(&conn, note_id, Some(folder_b)).unwrap();

        let entries = list_entries(&conn).unwrap();
        let moved = entries.iter().find(|e| e.id == note_id).unwrap();
        assert_eq!(moved.parent_id, Some(folder_b));
    }
}
