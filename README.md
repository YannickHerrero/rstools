# rstools

A vim-centric terminal toolset built in Rust with [ratatui](https://ratatui.rs).

## Philosophy

rstools is a collection of independent terminal tools unified by a single hub,
designed for people who think in vim. If you live in neovim and wish every tool
had hjkl navigation, a leader key, which-key for discoverability, and telescope
for finding things — this is for you.

**Core principles:**

- **Vim keybindings are not an afterthought** — they are the primary and only
  input method. Modal editing (Normal/Insert/Command), motions, and operators
  work the way you expect.
- **Space as leader** — press Space in Normal mode and a which-key popup shows
  you every available action. No memorization required.
- **Telescope everywhere** — fuzzy find anything across any tool.
- **One hub, many tools** — tools are embedded views inside a single terminal
  app, like buffers in neovim. Switch between them instantly.
- **Local-first data** — everything is stored in a single SQLite database on
  your machine. No cloud, no accounts, no sync complexity.

## Tools

| Tool | Status | Description |
|------|--------|-------------|
| Hub | MVP | Main orchestrator — dashboard, tool picker, tab switching |
| Todo | MVP | Minimalist todo list with vim navigation, filtering, CRUD |

**Planned:** KeePass client, database viewer, and more.

## Building

```bash
cargo build --release
```

The binary is `rstools`:

```bash
./target/release/rstools
```

## Keybinds

### Global (all modes)

| Key | Action |
|-----|--------|
| `Esc` | Return to Normal mode / cancel input |
| `Ctrl-c` | Force quit |

### Normal Mode — Global

| Key | Action |
|-----|--------|
| `<Space>` | Open which-key (leader menu) |
| `<Space><Space>` | Tool picker (telescope) |
| `<Space>f` | Find (telescope fuzzy finder) |
| `<Space>t` | Todo tool (switch or sub-menu) |
| `<Space>q` | Quit |
| `<Space>1-9` | Switch to tool by index |
| `gt` / `gT` | Next / previous tool |
| `:q` | Close current tool / quit |
| `:qa` | Quit all |
| `q` | Quit (from dashboard) |
| `?` | Help |

### Normal Mode — List Navigation (all tools)

| Key | Action |
|-----|--------|
| `j` / `k` | Move down / up |
| `gg` | Go to top |
| `G` | Go to bottom |
| `Ctrl-d` / `Ctrl-u` | Half-page down / up |
| `/` | Search / filter within current view |
| `Enter` | Confirm / select / toggle |
| `dd` | Delete item |

### Todo Tool

| Key | Action |
|-----|--------|
| `a` | Add new todo (enters Insert mode) |
| `o` | Add todo below current (enters Insert mode) |
| `e` | Edit selected todo (enters Insert mode) |
| `Enter` | Toggle completed |
| `dd` | Delete todo |
| `/` | Filter todos (live search) |
| `j` / `k` | Move down / up |
| `gg` / `G` | Jump to top / bottom |

### Insert Mode (text input)

| Key | Action |
|-----|--------|
| `Enter` | Submit input |
| `Esc` | Cancel and return to Normal mode |
| `Left` / `Right` | Move cursor |
| `Backspace` | Delete character before cursor |

### Telescope (fuzzy finder)

| Key | Action |
|-----|--------|
| Type | Filter results |
| `Tab` / `Down` | Move selection down |
| `Shift-Tab` / `Up` | Move selection up |
| `Enter` | Select item |
| `Esc` | Close telescope |
| `Backspace` | Delete search character |

## Architecture

```
rstools/
├── Cargo.toml               # Workspace root
├── crates/
│   ├── rstools-core/         # Shared: vim keybinds, which-key, telescope, db, UI
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── db.rs          # SQLite connection (WAL mode, XDG paths)
│   │       ├── keybinds.rs    # Vim modes, multi-key sequences, Action enum
│   │       ├── which_key.rs   # Leader key popup overlay
│   │       ├── telescope.rs   # Fuzzy finder overlay
│   │       ├── tool.rs        # Tool trait (every tool implements this)
│   │       └── ui.rs          # Shared UI: tab bar, status bar, command line
│   ├── rstools-hub/          # The orchestrator binary
│   │   └── src/
│   │       ├── main.rs        # Entry point, terminal setup, event loop
│   │       └── app.rs         # App state, tool registry, event routing
│   └── rstools-todo/         # Todo list tool
│       └── src/
│           ├── lib.rs         # Tool trait impl, input handling, state
│           ├── model.rs       # Todo struct, SQLite CRUD operations
│           └── ui.rs          # Todo list and input rendering
```

### How it works

- **rstools-core** — every tool depends on this. It provides the `Tool` trait,
  vim-style input handling with multi-key sequences (`gg`, `dd`, `gt`), the
  which-key popup, telescope overlay, database connection, and shared UI
  components (tab bar, status bar, command line).

- **rstools-hub** — the only binary. It owns the event loop, manages a registry
  of tools, routes key events to the active tool, and renders everything. Tools
  are embedded views — switching between them is instant, like switching buffers
  in neovim.

- **rstools-todo** (and future tools) — library crates that implement the `Tool`
  trait. Each tool manages its own state, handles its own keybinds (delegating
  global ones back to the hub via the `Action` enum), renders its own UI, and
  owns its own database tables.

### Adding a new tool

1. Create `crates/rstools-<name>/` with `Cargo.toml`
2. Implement the `Tool` trait from `rstools-core`
3. Add database migration in the tool's `init_db()` method
4. Register the tool in `rstools-hub/src/main.rs`
5. Add which-key bindings (mandatory)
6. Update `AGENTS.md` and `README.md`

## Data Storage

All data lives in `~/.local/share/rstools/rstools.db` (XDG-compliant on Linux).
SQLite with WAL mode and foreign keys enabled. Each tool manages its own tables.
Back up this single file to back up everything.

### Todo schema

```sql
CREATE TABLE todos (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    completed BOOLEAN NOT NULL DEFAULT 0,
    description TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
```

## Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| ratatui | 0.30 | TUI rendering framework |
| crossterm | 0.29 | Terminal backend (input/output) |
| rusqlite | 0.38 | SQLite database (bundled) |
| chrono | 0.4 | Timestamps |
| directories | 6 | XDG-compliant data paths |
| anyhow | 1 | Error handling |

## License

MIT
