use anyhow::Result;
use rusqlite::Connection;

// ── Entry types ──────────────────────────────────────────────────────

/// Entry type: folder or query (like directory vs file in neo-tree).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryType {
    Folder,
    Query,
}

/// HTTP method for a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

impl HttpMethod {
    pub fn as_str(&self) -> &str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Patch => "PATCH",
            HttpMethod::Delete => "DELETE",
            HttpMethod::Head => "HEAD",
            HttpMethod::Options => "OPTIONS",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "POST" => HttpMethod::Post,
            "PUT" => HttpMethod::Put,
            "PATCH" => HttpMethod::Patch,
            "DELETE" => HttpMethod::Delete,
            "HEAD" => HttpMethod::Head,
            "OPTIONS" => HttpMethod::Options,
            _ => HttpMethod::Get,
        }
    }

    /// Returns the next method in cycle order.
    pub fn next(self) -> Self {
        match self {
            HttpMethod::Get => HttpMethod::Post,
            HttpMethod::Post => HttpMethod::Put,
            HttpMethod::Put => HttpMethod::Patch,
            HttpMethod::Patch => HttpMethod::Delete,
            HttpMethod::Delete => HttpMethod::Head,
            HttpMethod::Head => HttpMethod::Options,
            HttpMethod::Options => HttpMethod::Get,
        }
    }

    /// Returns the previous method in cycle order.
    pub fn prev(self) -> Self {
        match self {
            HttpMethod::Get => HttpMethod::Options,
            HttpMethod::Post => HttpMethod::Get,
            HttpMethod::Put => HttpMethod::Post,
            HttpMethod::Patch => HttpMethod::Put,
            HttpMethod::Delete => HttpMethod::Patch,
            HttpMethod::Head => HttpMethod::Delete,
            HttpMethod::Options => HttpMethod::Head,
        }
    }
}

// ── Request data model ───────────────────────────────────────────────

/// A persisted HTTP request linked to a query entry.
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub id: i64,
    pub entry_id: i64,
    pub method: HttpMethod,
    pub url: String,
    pub body: String,
}

/// A single header row for a request.
#[derive(Debug, Clone)]
pub struct HttpHeader {
    pub id: i64,
    pub request_id: i64,
    pub key: String,
    pub value: String,
    pub enabled: bool,
    pub sort_order: i64,
}

/// A single query parameter row for a request.
#[derive(Debug, Clone)]
pub struct HttpQueryParam {
    pub id: i64,
    pub request_id: i64,
    pub key: String,
    pub value: String,
    pub enabled: bool,
    pub sort_order: i64,
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
    pub expanded: bool,
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
        END;

        CREATE TABLE IF NOT EXISTS http_requests (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            entry_id INTEGER UNIQUE NOT NULL REFERENCES http_entries(id) ON DELETE CASCADE,
            method TEXT NOT NULL DEFAULT 'GET',
            url TEXT NOT NULL DEFAULT '',
            body TEXT NOT NULL DEFAULT '',
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TRIGGER IF NOT EXISTS http_requests_updated_at
        AFTER UPDATE ON http_requests
        BEGIN
            UPDATE http_requests SET updated_at = CURRENT_TIMESTAMP WHERE id = NEW.id;
        END;

        CREATE TABLE IF NOT EXISTS http_headers (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            request_id INTEGER NOT NULL REFERENCES http_requests(id) ON DELETE CASCADE,
            key TEXT NOT NULL,
            value TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            sort_order INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS http_query_params (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            request_id INTEGER NOT NULL REFERENCES http_requests(id) ON DELETE CASCADE,
            key TEXT NOT NULL,
            value TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            sort_order INTEGER NOT NULL DEFAULT 0
        );",
    )?;

    // Migration: add expanded column to http_entries if it doesn't exist yet.
    let has_expanded: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('http_entries') WHERE name = 'expanded'")?
        .exists([])?;
    if !has_expanded {
        conn.execute_batch(
            "ALTER TABLE http_entries ADD COLUMN expanded INTEGER NOT NULL DEFAULT 0;",
        )?;
    }

    Ok(())
}

/// List all entries from the database.
pub fn list_entries(conn: &Connection) -> Result<Vec<HttpEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, parent_id, name, entry_type, expanded, created_at, updated_at
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
        "INSERT INTO http_entries (parent_id, name, entry_type) VALUES (?1, ?2, ?3)",
        rusqlite::params![parent_id, name, entry_type.as_str()],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Update the expanded state of a folder entry.
pub fn set_entry_expanded(conn: &Connection, id: i64, expanded: bool) -> Result<()> {
    conn.execute(
        "UPDATE http_entries SET expanded = ?1 WHERE id = ?2",
        rusqlite::params![expanded as i64, id],
    )?;
    Ok(())
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
        "SELECT id, parent_id, name, entry_type, expanded, created_at, updated_at
         FROM http_entries WHERE id = ?1",
        rusqlite::params![source_id],
        |row| {
            let entry_type_str: String = row.get(3)?;
            Ok(HttpEntry {
                id: row.get(0)?,
                parent_id: row.get(1)?,
                name: row.get(2)?,
                entry_type: EntryType::from_str(&entry_type_str).unwrap_or(EntryType::Query),
                expanded: row.get::<_, i64>(4)? != 0,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
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

// ── Request CRUD ─────────────────────────────────────────────────────

/// Ensure a request row exists for the given entry. Creates a default one if missing.
/// Returns the request ID.
pub fn ensure_request(conn: &Connection, entry_id: i64) -> Result<i64> {
    // Try to find existing
    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM http_requests WHERE entry_id = ?1",
            rusqlite::params![entry_id],
            |row| row.get(0),
        )
        .ok();

    if let Some(id) = existing {
        return Ok(id);
    }

    conn.execute(
        "INSERT INTO http_requests (entry_id, method, url, body) VALUES (?1, 'GET', '', '')",
        rusqlite::params![entry_id],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Load a request by entry_id. Returns None if no request row exists.
pub fn load_request(conn: &Connection, entry_id: i64) -> Result<Option<HttpRequest>> {
    let result = conn.query_row(
        "SELECT id, entry_id, method, url, body FROM http_requests WHERE entry_id = ?1",
        rusqlite::params![entry_id],
        |row| {
            let method_str: String = row.get(2)?;
            Ok(HttpRequest {
                id: row.get(0)?,
                entry_id: row.get(1)?,
                method: HttpMethod::from_str(&method_str),
                url: row.get(3)?,
                body: row.get(4)?,
            })
        },
    );

    match result {
        Ok(req) => Ok(Some(req)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Save a request (method, url, body) by request ID.
pub fn save_request(
    conn: &Connection,
    request_id: i64,
    method: HttpMethod,
    url: &str,
    body: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE http_requests SET method = ?1, url = ?2, body = ?3 WHERE id = ?4",
        rusqlite::params![method.as_str(), url, body, request_id],
    )?;
    Ok(())
}

// ── Header CRUD ──────────────────────────────────────────────────────

/// Load all headers for a request, ordered by sort_order.
pub fn load_headers(conn: &Connection, request_id: i64) -> Result<Vec<HttpHeader>> {
    let mut stmt = conn.prepare(
        "SELECT id, request_id, key, value, enabled, sort_order
         FROM http_headers
         WHERE request_id = ?1
         ORDER BY sort_order ASC, id ASC",
    )?;
    let headers = stmt
        .query_map(rusqlite::params![request_id], |row| {
            Ok(HttpHeader {
                id: row.get(0)?,
                request_id: row.get(1)?,
                key: row.get(2)?,
                value: row.get(3)?,
                enabled: row.get::<_, i64>(4)? != 0,
                sort_order: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(headers)
}

/// Add a new header. Returns the new header's ID.
pub fn add_header(
    conn: &Connection,
    request_id: i64,
    key: &str,
    value: &str,
    sort_order: i64,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO http_headers (request_id, key, value, sort_order) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![request_id, key, value, sort_order],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Update an existing header's key and value.
pub fn update_header(conn: &Connection, header_id: i64, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "UPDATE http_headers SET key = ?1, value = ?2 WHERE id = ?3",
        rusqlite::params![key, value, header_id],
    )?;
    Ok(())
}

/// Toggle a header's enabled state.
pub fn toggle_header(conn: &Connection, header_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE http_headers SET enabled = NOT enabled WHERE id = ?1",
        rusqlite::params![header_id],
    )?;
    Ok(())
}

/// Delete a header.
pub fn delete_header(conn: &Connection, header_id: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM http_headers WHERE id = ?1",
        rusqlite::params![header_id],
    )?;
    Ok(())
}

/// Replace all headers for a request (used for bulk save).
pub fn replace_headers(
    conn: &Connection,
    request_id: i64,
    headers: &[(String, String, bool)],
) -> Result<()> {
    conn.execute(
        "DELETE FROM http_headers WHERE request_id = ?1",
        rusqlite::params![request_id],
    )?;
    for (i, (key, value, enabled)) in headers.iter().enumerate() {
        conn.execute(
            "INSERT INTO http_headers (request_id, key, value, enabled, sort_order) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![request_id, key, value, *enabled as i64, i as i64],
        )?;
    }
    Ok(())
}

// ── Query Param CRUD ─────────────────────────────────────────────────

/// Load all query params for a request, ordered by sort_order.
pub fn load_query_params(conn: &Connection, request_id: i64) -> Result<Vec<HttpQueryParam>> {
    let mut stmt = conn.prepare(
        "SELECT id, request_id, key, value, enabled, sort_order
         FROM http_query_params
         WHERE request_id = ?1
         ORDER BY sort_order ASC, id ASC",
    )?;
    let params = stmt
        .query_map(rusqlite::params![request_id], |row| {
            Ok(HttpQueryParam {
                id: row.get(0)?,
                request_id: row.get(1)?,
                key: row.get(2)?,
                value: row.get(3)?,
                enabled: row.get::<_, i64>(4)? != 0,
                sort_order: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(params)
}

/// Add a new query param. Returns the new param's ID.
pub fn add_query_param(
    conn: &Connection,
    request_id: i64,
    key: &str,
    value: &str,
    sort_order: i64,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO http_query_params (request_id, key, value, sort_order) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![request_id, key, value, sort_order],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Update an existing query param's key and value.
pub fn update_query_param(conn: &Connection, param_id: i64, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "UPDATE http_query_params SET key = ?1, value = ?2 WHERE id = ?3",
        rusqlite::params![key, value, param_id],
    )?;
    Ok(())
}

/// Toggle a query param's enabled state.
pub fn toggle_query_param(conn: &Connection, param_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE http_query_params SET enabled = NOT enabled WHERE id = ?1",
        rusqlite::params![param_id],
    )?;
    Ok(())
}

/// Delete a query param.
pub fn delete_query_param(conn: &Connection, param_id: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM http_query_params WHERE id = ?1",
        rusqlite::params![param_id],
    )?;
    Ok(())
}

/// Replace all query params for a request (used for bulk save).
pub fn replace_query_params(
    conn: &Connection,
    request_id: i64,
    params: &[(String, String, bool)],
) -> Result<()> {
    conn.execute(
        "DELETE FROM http_query_params WHERE request_id = ?1",
        rusqlite::params![request_id],
    )?;
    for (i, (key, value, enabled)) in params.iter().enumerate() {
        conn.execute(
            "INSERT INTO http_query_params (request_id, key, value, enabled, sort_order) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![request_id, key, value, *enabled as i64, i as i64],
        )?;
    }
    Ok(())
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

    // ── Request tests ────────────────────────────────────────────────

    #[test]
    fn test_ensure_and_load_request() {
        let conn = open_memory_db().unwrap();
        init_db(&conn).unwrap();

        let entry_id = add_entry(&conn, None, "test-query", EntryType::Query).unwrap();
        let req_id = ensure_request(&conn, entry_id).unwrap();
        assert!(req_id > 0);

        // Calling again returns the same ID
        let req_id2 = ensure_request(&conn, entry_id).unwrap();
        assert_eq!(req_id, req_id2);

        let req = load_request(&conn, entry_id).unwrap().unwrap();
        assert_eq!(req.method, HttpMethod::Get);
        assert_eq!(req.url, "");
        assert_eq!(req.body, "");
    }

    #[test]
    fn test_save_request() {
        let conn = open_memory_db().unwrap();
        init_db(&conn).unwrap();

        let entry_id = add_entry(&conn, None, "test-query", EntryType::Query).unwrap();
        let req_id = ensure_request(&conn, entry_id).unwrap();

        save_request(
            &conn,
            req_id,
            HttpMethod::Post,
            "https://api.example.com",
            "{\"key\": \"val\"}",
        )
        .unwrap();

        let req = load_request(&conn, entry_id).unwrap().unwrap();
        assert_eq!(req.method, HttpMethod::Post);
        assert_eq!(req.url, "https://api.example.com");
        assert_eq!(req.body, "{\"key\": \"val\"}");
    }

    #[test]
    fn test_load_request_nonexistent() {
        let conn = open_memory_db().unwrap();
        init_db(&conn).unwrap();

        let result = load_request(&conn, 999).unwrap();
        assert!(result.is_none());
    }

    // ── Header tests ─────────────────────────────────────────────────

    #[test]
    fn test_headers_crud() {
        let conn = open_memory_db().unwrap();
        init_db(&conn).unwrap();

        let entry_id = add_entry(&conn, None, "test-query", EntryType::Query).unwrap();
        let req_id = ensure_request(&conn, entry_id).unwrap();

        // Add headers
        let h1 = add_header(&conn, req_id, "Content-Type", "application/json", 0).unwrap();
        let h2 = add_header(&conn, req_id, "Authorization", "Bearer token", 1).unwrap();

        let headers = load_headers(&conn, req_id).unwrap();
        assert_eq!(headers.len(), 2);
        assert_eq!(headers[0].key, "Content-Type");
        assert!(headers[0].enabled);

        // Update
        update_header(&conn, h1, "Accept", "text/html").unwrap();
        let headers = load_headers(&conn, req_id).unwrap();
        assert_eq!(headers[0].key, "Accept");

        // Toggle
        toggle_header(&conn, h1).unwrap();
        let headers = load_headers(&conn, req_id).unwrap();
        assert!(!headers[0].enabled);

        // Delete
        delete_header(&conn, h2).unwrap();
        let headers = load_headers(&conn, req_id).unwrap();
        assert_eq!(headers.len(), 1);
    }

    #[test]
    fn test_replace_headers() {
        let conn = open_memory_db().unwrap();
        init_db(&conn).unwrap();

        let entry_id = add_entry(&conn, None, "test-query", EntryType::Query).unwrap();
        let req_id = ensure_request(&conn, entry_id).unwrap();

        add_header(&conn, req_id, "Old-Header", "old-value", 0).unwrap();

        let new_headers = vec![
            (
                "Content-Type".to_string(),
                "application/json".to_string(),
                true,
            ),
            ("X-Custom".to_string(), "value".to_string(), false),
        ];
        replace_headers(&conn, req_id, &new_headers).unwrap();

        let headers = load_headers(&conn, req_id).unwrap();
        assert_eq!(headers.len(), 2);
        assert_eq!(headers[0].key, "Content-Type");
        assert!(headers[0].enabled);
        assert_eq!(headers[1].key, "X-Custom");
        assert!(!headers[1].enabled);
    }

    // ── Query param tests ────────────────────────────────────────────

    #[test]
    fn test_query_params_crud() {
        let conn = open_memory_db().unwrap();
        init_db(&conn).unwrap();

        let entry_id = add_entry(&conn, None, "test-query", EntryType::Query).unwrap();
        let req_id = ensure_request(&conn, entry_id).unwrap();

        // Add params
        let p1 = add_query_param(&conn, req_id, "page", "1", 0).unwrap();
        let p2 = add_query_param(&conn, req_id, "limit", "10", 1).unwrap();

        let params = load_query_params(&conn, req_id).unwrap();
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].key, "page");
        assert!(params[0].enabled);

        // Update
        update_query_param(&conn, p1, "offset", "0").unwrap();
        let params = load_query_params(&conn, req_id).unwrap();
        assert_eq!(params[0].key, "offset");

        // Toggle
        toggle_query_param(&conn, p1).unwrap();
        let params = load_query_params(&conn, req_id).unwrap();
        assert!(!params[0].enabled);

        // Delete
        delete_query_param(&conn, p2).unwrap();
        let params = load_query_params(&conn, req_id).unwrap();
        assert_eq!(params.len(), 1);
    }

    #[test]
    fn test_replace_query_params() {
        let conn = open_memory_db().unwrap();
        init_db(&conn).unwrap();

        let entry_id = add_entry(&conn, None, "test-query", EntryType::Query).unwrap();
        let req_id = ensure_request(&conn, entry_id).unwrap();

        add_query_param(&conn, req_id, "old", "param", 0).unwrap();

        let new_params = vec![
            ("page".to_string(), "1".to_string(), true),
            ("limit".to_string(), "10".to_string(), true),
            ("debug".to_string(), "true".to_string(), false),
        ];
        replace_query_params(&conn, req_id, &new_params).unwrap();

        let params = load_query_params(&conn, req_id).unwrap();
        assert_eq!(params.len(), 3);
        assert_eq!(params[0].key, "page");
        assert_eq!(params[2].key, "debug");
        assert!(!params[2].enabled);
    }

    // ── HttpMethod tests ─────────────────────────────────────────────

    #[test]
    fn test_http_method_cycle() {
        let method = HttpMethod::Get;
        assert_eq!(method.next(), HttpMethod::Post);
        assert_eq!(method.prev(), HttpMethod::Options);

        // Full cycle
        let mut m = HttpMethod::Get;
        for _ in 0..7 {
            m = m.next();
        }
        assert_eq!(m, HttpMethod::Get);
    }

    #[test]
    fn test_http_method_from_str() {
        assert_eq!(HttpMethod::from_str("GET"), HttpMethod::Get);
        assert_eq!(HttpMethod::from_str("post"), HttpMethod::Post);
        assert_eq!(HttpMethod::from_str("unknown"), HttpMethod::Get);
    }

    // ── Cascade delete tests ─────────────────────────────────────────

    #[test]
    fn test_delete_entry_cascades_request_data() {
        let conn = open_memory_db().unwrap();
        init_db(&conn).unwrap();

        let entry_id = add_entry(&conn, None, "test-query", EntryType::Query).unwrap();
        let req_id = ensure_request(&conn, entry_id).unwrap();
        add_header(&conn, req_id, "Content-Type", "application/json", 0).unwrap();
        add_query_param(&conn, req_id, "key", "value", 0).unwrap();

        // Delete the entry — should cascade to request, headers, params
        delete_entry(&conn, entry_id).unwrap();

        let req = load_request(&conn, entry_id).unwrap();
        assert!(req.is_none());
    }
}
