use anyhow::Result;
use rusqlite::Connection;

use rstools_http::model as http_model;
use rstools_keepass::model as keepass_model;
use rstools_todo::model as todo_model;

pub fn seed_demo_data(conn: &Connection) -> Result<()> {
    todo_model::init_db(conn)?;
    http_model::init_db(conn)?;
    keepass_model::init_db(conn)?;

    seed_todos(conn)?;
    seed_http(conn)?;
    seed_keepass(conn)?;
    Ok(())
}

fn seed_todos(conn: &Connection) -> Result<()> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM todos", [], |row| row.get(0))?;
    if count > 0 {
        return Ok(());
    }

    let shipped = todo_model::add_todo(conn, "Ship demo mode", Some("Seeded screenshot data"))?;
    let _ = todo_model::add_todo(
        conn,
        "Capture Todo screenshot",
        Some("Use clean terminal theme and full-width layout"),
    )?;
    let _ = todo_model::add_todo(
        conn,
        "Capture HTTP screenshot",
        Some("Open Demo APIs/users/get-user first"),
    )?;
    let docs = todo_model::add_todo(
        conn,
        "Polish README copy",
        Some("Keep sections short and scan-friendly"),
    )?;

    todo_model::toggle_todo(conn, shipped)?;
    todo_model::toggle_todo(conn, docs)?;
    Ok(())
}

fn seed_http(conn: &Connection) -> Result<()> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM http_entries", [], |row| row.get(0))?;
    if count > 0 {
        return Ok(());
    }

    let demo_root = http_model::add_entry(conn, None, "Demo APIs", http_model::EntryType::Folder)?;
    http_model::set_entry_expanded(conn, demo_root, true)?;

    let users = http_model::add_entry(
        conn,
        Some(demo_root),
        "users",
        http_model::EntryType::Folder,
    )?;
    http_model::set_entry_expanded(conn, users, true)?;

    let auth = http_model::add_entry(conn, Some(demo_root), "auth", http_model::EntryType::Folder)?;
    http_model::set_entry_expanded(conn, auth, true)?;

    let get_user =
        http_model::add_entry(conn, Some(users), "get-user", http_model::EntryType::Query)?;
    let create_user = http_model::add_entry(
        conn,
        Some(users),
        "create-user",
        http_model::EntryType::Query,
    )?;
    let login = http_model::add_entry(conn, Some(auth), "login", http_model::EntryType::Query)?;

    seed_http_request(
        conn,
        get_user,
        http_model::HttpMethod::Get,
        "https://jsonplaceholder.typicode.com/users/1",
        "",
        &[("accept", "application/json", true)],
        &[],
    )?;

    seed_http_request(
        conn,
        create_user,
        http_model::HttpMethod::Post,
        "https://api.demo.local/users",
        "{\n  \"name\": \"Ari Demo\",\n  \"email\": \"ari@example.com\"\n}",
        &[
            ("accept", "application/json", true),
            ("content-type", "application/json", true),
        ],
        &[],
    )?;

    seed_http_request(
        conn,
        login,
        http_model::HttpMethod::Post,
        "https://api.demo.local/auth/login",
        "{\n  \"email\": \"ari@example.com\",\n  \"password\": \"demo-password\"\n}",
        &[
            ("accept", "application/json", true),
            ("content-type", "application/json", true),
        ],
        &[],
    )?;

    Ok(())
}

fn seed_http_request(
    conn: &Connection,
    entry_id: i64,
    method: http_model::HttpMethod,
    url: &str,
    body: &str,
    headers: &[(&str, &str, bool)],
    query_params: &[(&str, &str, bool)],
) -> Result<()> {
    let request_id = http_model::ensure_request(conn, entry_id)?;
    http_model::save_request(conn, request_id, method, url, body)?;

    let header_rows: Vec<(String, String, bool)> = headers
        .iter()
        .map(|(k, v, enabled)| ((*k).to_string(), (*v).to_string(), *enabled))
        .collect();
    http_model::replace_headers(conn, request_id, &header_rows)?;

    let param_rows: Vec<(String, String, bool)> = query_params
        .iter()
        .map(|(k, v, enabled)| ((*k).to_string(), (*v).to_string(), *enabled))
        .collect();
    http_model::replace_query_params(conn, request_id, &param_rows)?;

    Ok(())
}

fn seed_keepass(conn: &Connection) -> Result<()> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM keepass_files", [], |row| row.get(0))?;
    if count > 0 {
        return Ok(());
    }

    let _ = keepass_model::upsert_file(conn, "/demo/vaults/personal-demo.kdbx", "personal-demo")?;
    let _ = keepass_model::upsert_file(conn, "/demo/vaults/work-demo.kdbx", "work-demo")?;
    let _ = keepass_model::upsert_file(
        conn,
        "/demo/vaults/shared-team-demo.kdbx",
        "shared-team-demo",
    )?;

    Ok(())
}
