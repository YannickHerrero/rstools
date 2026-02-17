use anyhow::Result;
use rusqlite::Connection;

/// A single todo item.
#[derive(Debug, Clone)]
pub struct Todo {
    pub id: i64,
    pub title: String,
    pub completed: bool,
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Initialize the todos table.
pub fn init_db(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS todos (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            title TEXT NOT NULL,
            completed BOOLEAN NOT NULL DEFAULT 0,
            description TEXT,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TRIGGER IF NOT EXISTS todos_updated_at
        AFTER UPDATE ON todos
        BEGIN
            UPDATE todos SET updated_at = CURRENT_TIMESTAMP WHERE id = NEW.id;
        END;",
    )?;
    Ok(())
}

/// Fetch all todos, ordered by creation date (newest first for incomplete, then completed).
pub fn list_todos(conn: &Connection) -> Result<Vec<Todo>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, completed, description, created_at, updated_at
         FROM todos
         ORDER BY completed ASC, created_at DESC",
    )?;

    let todos = stmt
        .query_map([], |row| {
            Ok(Todo {
                id: row.get(0)?,
                title: row.get(1)?,
                completed: row.get(2)?,
                description: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(todos)
}

/// Insert a new todo. Returns the new todo's id.
pub fn add_todo(conn: &Connection, title: &str, description: Option<&str>) -> Result<i64> {
    conn.execute(
        "INSERT INTO todos (title, description) VALUES (?1, ?2)",
        rusqlite::params![title, description],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Toggle the completed status of a todo.
pub fn toggle_todo(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE todos SET completed = NOT completed WHERE id = ?1",
        [id],
    )?;
    Ok(())
}

/// Update a todo's title (and optionally description).
pub fn update_todo(
    conn: &Connection,
    id: i64,
    title: &str,
    description: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE todos SET title = ?1, description = ?2 WHERE id = ?3",
        rusqlite::params![title, description, id],
    )?;
    Ok(())
}

/// Delete a todo by id.
pub fn delete_todo(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM todos WHERE id = ?1", [id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstools_core::db::open_memory_db;

    #[test]
    fn test_crud_operations() {
        let conn = open_memory_db().unwrap();
        init_db(&conn).unwrap();

        // Add
        let id = add_todo(&conn, "Test todo", None).unwrap();
        assert!(id > 0);

        // List
        let todos = list_todos(&conn).unwrap();
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].title, "Test todo");
        assert!(!todos[0].completed);
        assert!(todos[0].description.is_none());

        // Toggle
        toggle_todo(&conn, id).unwrap();
        let todos = list_todos(&conn).unwrap();
        assert!(todos[0].completed);

        // Update
        update_todo(&conn, id, "Updated", Some("A description")).unwrap();
        let todos = list_todos(&conn).unwrap();
        assert_eq!(todos[0].title, "Updated");
        assert_eq!(todos[0].description.as_deref(), Some("A description"));

        // Delete
        delete_todo(&conn, id).unwrap();
        let todos = list_todos(&conn).unwrap();
        assert!(todos.is_empty());
    }
}
