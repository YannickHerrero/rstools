use anyhow::Result;
use rusqlite::Connection;

/// Entry type: folder or query (like directory vs file in neo-tree).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryType {
    Folder,
    Query,
}

impl EntryType {
    pub fn as_str(&self) -> &str {
        match self {
            EntryType::Folder => "folder",
            EntryType::Query => "query",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "folder" => Some(EntryType::Folder),
            "query" => Some(EntryType::Query),
            _ => None,
        }
    }
}

/// A single entry in the HTTP explorer tree.
#[derive(Debug, Clone)]
pub struct HttpEntry {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub entry_type: EntryType,
    pub created_at: String,
    pub updated_at: String,
}

/// Initialize the database tables for the HTTP tool.
pub fn init_db(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS http_entries (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            parent_id INTEGER REFERENCES http_entries(id) ON DELETE CASCADE,
            name TEXT NOT NULL,
            entry_type TEXT NOT NULL CHECK(entry_type IN ('folder', 'query')),
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TRIGGER IF NOT EXISTS http_entries_updated_at
        AFTER UPDATE ON http_entries
        BEGIN
            UPDATE http_entries SET updated_at = CURRENT_TIMESTAMP WHERE id = NEW.id;
        END;",
    )?;
    Ok(())
}

/// List all entries from the database.
pub fn list_entries(conn: &Connection) -> Result<Vec<HttpEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, parent_id, name, entry_type, created_at, updated_at
         FROM http_entries
         ORDER BY entry_type ASC, name ASC",
    )?;
    let entries = stmt
        .query_map([], |row| {
            let entry_type_str: String = row.get(3)?;
            Ok(HttpEntry {
                id: row.get(0)?,
                parent_id: row.get(1)?,
                name: row.get(2)?,
                entry_type: EntryType::from_str(&entry_type_str).unwrap_or(EntryType::Query),
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
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
        "INSERT INTO http_entries (parent_id, name, entry_type) VALUES (?1, ?2, ?3)",
        rusqlite::params![parent_id, name, entry_type.as_str()],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Rename an entry.
pub fn rename_entry(conn: &Connection, id: i64, new_name: &str) -> Result<()> {
    conn.execute(
        "UPDATE http_entries SET name = ?1 WHERE id = ?2",
        rusqlite::params![new_name, id],
    )?;
    Ok(())
}

/// Delete an entry (and all children via CASCADE).
pub fn delete_entry(conn: &Connection, id: i64) -> Result<()> {
    // Enable foreign keys for CASCADE to work
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    conn.execute(
        "DELETE FROM http_entries WHERE id = ?1",
        rusqlite::params![id],
    )?;
    Ok(())
}

/// Move an entry to a new parent.
pub fn move_entry(conn: &Connection, id: i64, new_parent_id: Option<i64>) -> Result<()> {
    conn.execute(
        "UPDATE http_entries SET parent_id = ?1 WHERE id = ?2",
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
    // Get the source entry
    let source: HttpEntry = conn.query_row(
        "SELECT id, parent_id, name, entry_type, created_at, updated_at
         FROM http_entries WHERE id = ?1",
        rusqlite::params![source_id],
        |row| {
            let entry_type_str: String = row.get(3)?;
            Ok(HttpEntry {
                id: row.get(0)?,
                parent_id: row.get(1)?,
                name: row.get(2)?,
                entry_type: EntryType::from_str(&entry_type_str).unwrap_or(EntryType::Query),
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        },
    )?;

    // Create the copy
    let new_id = add_entry(conn, new_parent_id, &source.name, source.entry_type)?;

    // Recursively copy children
    let children: Vec<i64> = {
        let mut stmt = conn.prepare("SELECT id FROM http_entries WHERE parent_id = ?1")?;
        stmt.query_map(rusqlite::params![source_id], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?
    };

    for child_id in children {
        copy_entry_recursive(conn, child_id, Some(new_id))?;
    }

    Ok(new_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstools_core::db::open_memory_db;

    #[test]
    fn test_init_db() {
        let conn = open_memory_db().unwrap();
        init_db(&conn).unwrap();
        // Should be idempotent
        init_db(&conn).unwrap();
    }

    #[test]
    fn test_add_and_list_entries() {
        let conn = open_memory_db().unwrap();
        init_db(&conn).unwrap();

        add_entry(&conn, None, "folder-a", EntryType::Folder).unwrap();
        add_entry(&conn, None, "query-b", EntryType::Query).unwrap();

        let entries = list_entries(&conn).unwrap();
        assert_eq!(entries.len(), 2);
        // Folders first in sort order
        assert_eq!(entries[0].name, "folder-a");
        assert_eq!(entries[0].entry_type, EntryType::Folder);
        assert_eq!(entries[1].name, "query-b");
        assert_eq!(entries[1].entry_type, EntryType::Query);
    }

    #[test]
    fn test_nested_entries() {
        let conn = open_memory_db().unwrap();
        init_db(&conn).unwrap();

        let folder_id = add_entry(&conn, None, "api", EntryType::Folder).unwrap();
        add_entry(&conn, Some(folder_id), "get-users", EntryType::Query).unwrap();
        add_entry(&conn, Some(folder_id), "post-user", EntryType::Query).unwrap();

        let entries = list_entries(&conn).unwrap();
        assert_eq!(entries.len(), 3);

        let children: Vec<_> = entries
            .iter()
            .filter(|e| e.parent_id == Some(folder_id))
            .collect();
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn test_rename_entry() {
        let conn = open_memory_db().unwrap();
        init_db(&conn).unwrap();

        let id = add_entry(&conn, None, "old-name", EntryType::Query).unwrap();
        rename_entry(&conn, id, "new-name").unwrap();

        let entries = list_entries(&conn).unwrap();
        assert_eq!(entries[0].name, "new-name");
    }

    #[test]
    fn test_delete_entry_cascade() {
        let conn = open_memory_db().unwrap();
        init_db(&conn).unwrap();

        let folder_id = add_entry(&conn, None, "api", EntryType::Folder).unwrap();
        add_entry(&conn, Some(folder_id), "get-users", EntryType::Query).unwrap();
        add_entry(&conn, Some(folder_id), "post-user", EntryType::Query).unwrap();

        delete_entry(&conn, folder_id).unwrap();

        let entries = list_entries(&conn).unwrap();
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn test_move_entry() {
        let conn = open_memory_db().unwrap();
        init_db(&conn).unwrap();

        let folder_a = add_entry(&conn, None, "folder-a", EntryType::Folder).unwrap();
        let folder_b = add_entry(&conn, None, "folder-b", EntryType::Folder).unwrap();
        let query = add_entry(&conn, Some(folder_a), "query", EntryType::Query).unwrap();

        move_entry(&conn, query, Some(folder_b)).unwrap();

        let entries = list_entries(&conn).unwrap();
        let moved = entries.iter().find(|e| e.id == query).unwrap();
        assert_eq!(moved.parent_id, Some(folder_b));
    }

    #[test]
    fn test_copy_entry_recursive() {
        let conn = open_memory_db().unwrap();
        init_db(&conn).unwrap();

        let folder = add_entry(&conn, None, "api", EntryType::Folder).unwrap();
        add_entry(&conn, Some(folder), "get-users", EntryType::Query).unwrap();
        add_entry(&conn, Some(folder), "post-user", EntryType::Query).unwrap();

        let copy_id = copy_entry_recursive(&conn, folder, None).unwrap();

        let entries = list_entries(&conn).unwrap();
        // Original folder + 2 children + copied folder + 2 copied children = 6
        assert_eq!(entries.len(), 6);

        let copied_children: Vec<_> = entries
            .iter()
            .filter(|e| e.parent_id == Some(copy_id))
            .collect();
        assert_eq!(copied_children.len(), 2);
    }
}
