# rstools

A vim-centric terminal toolset built in Rust with [ratatui](https://ratatui.rs).

## Philosophy

rstools is a collection of independent terminal tools unified by a single hub.
The entire UX is modeled after neovim with modal editing, hjkl navigation, leader key, which-key, and telescope.

**Core principles:**

- **Vim-native input**: modal editing (Normal/Insert/Command), motions, and
  operators are the primary and only input method.
- **Space as leader**: press Space in Normal mode and a which-key popup shows
  every available action.
- **Telescope**: fuzzy find anything across any tool.
- **One hub, many tools**: tools are embedded views inside a single terminal
  app. Switch between them like buffers in neovim.
- **Local-first data**: everything is stored in a single local SQLite database.

## Tools

| Tool | Status | Description |
|------|--------|-------------|
| Hub | MVP | Main orchestrator, dashboard, tool picker, tab switching |
| Todo | MVP | Minimalist todo list with vim navigation, filtering, CRUD |
| HTTP | MVP | HTTP client & API explorer (Postman-like) with neo-tree sidebar |
| KeePass | MVP | Read-only KDBX4 vault viewer with PIN quick-access |

**Planned:** database viewer, and more.

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

### Normal Mode, Global

| Key | Action |
|-----|--------|
| `<Space>` | Open which-key (leader menu) |
| `<Space><Space>` | Tool picker (telescope) |
| `<Space>f` | Find (telescope fuzzy finder) |
| `<Space>t` | Todo tool (switch or sub-menu) |
| `<Space>h` | HTTP tool (switch or sub-menu) |
| `<Space>k` | KeePass tool (switch or sub-menu) |
| `<Space>e` | Toggle HTTP explorer sidebar |
| `<Space>q` | Quit |
| `<Space>1-9` | Switch to tool by index |
| `gt` / `gT` | Next / previous tool |
| `:q` | Close current tool / quit |
| `:qa` | Quit all |
| `q` | Quit (from dashboard) |
| `?` | Help |

### Normal Mode, List Navigation (all tools)

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

### HTTP Tool — Explorer Sidebar

| Key | Action |
|-----|--------|
| `a` | Add entry (supports paths like `group/api/get-user`) |
| `r` | Rename selected entry |
| `d` | Delete selected entry (with y/n confirmation) |
| `y` | Copy selected entry to clipboard |
| `x` | Cut selected entry to clipboard |
| `p` | Paste from clipboard (recursive for folders) |
| `h` | Collapse folder / go to parent |
| `l` / `Enter` | Expand folder / open query in content panel |
| `j` / `k` | Move down / up |
| `gg` / `G` | Go to top / bottom |
| `Ctrl-d` / `Ctrl-u` | Half-page down / up |
| `Ctrl-l` | Move focus to content panel |

**Path creation:** When adding entries, use `/` to create nested folders.
`group/api/get-user` creates folders "group" and "api", then query "get-user".
Trailing `/` creates folder-only paths. Existing folders are reused.

### HTTP Tool — Request Panel

| Key | Action |
|-----|--------|
| `Tab` / `Shift-Tab` | Cycle sections (URL → Params → Headers → Body) |
| `Ctrl-h` | Move focus to sidebar |
| `Ctrl-j` | Move focus from request panel to response panel |
| `Ctrl-k` | Move focus from response to request, or from request to sidebar |
| `Ctrl-Enter` | Send request |
| `<Space>s` | Send request (leader key) |
| `<Space>e` | Toggle explorer sidebar |
| `m` / `M` | Cycle HTTP method forward / backward |
| `f` | Toggle fullscreen for the focused panel |
| `:w` | Save request to database |

#### URL Section

| Key | Action |
|-----|--------|
| `i` / `a` | Enter insert mode to edit URL |

#### Params / Headers Sections

| Key | Action |
|-----|--------|
| `j` / `k` | Move up / down |
| `a` | Add new key-value row |
| `i` / `Enter` | Edit selected row inline |
| `dd` | Delete selected row |
| `x` | Toggle row enabled / disabled |
| `Tab` (while editing) | Switch between key and value fields |

#### Body Section

| Key | Action |
|-----|--------|
| `i` / `a` / `A` / `I` | Enter insert mode |
| `o` / `O` | Insert line below / above |
| `h` / `j` / `k` / `l` | Cursor movement |
| `0` / `$` | Line start / end |

#### Response Panel

| Key | Action |
|-----|--------|
| `j` / `k` | Scroll response body / headers |
| `gg` / `G` | Go to top / bottom |
| `Ctrl-d` / `Ctrl-u` | Half-page down / up |
| `Tab` | Switch between Body and Headers tabs |

### KeePass Tool — Sidebar (File History)

| Key | Action |
|-----|--------|
| `j` / `k` | Move down / up |
| `Enter` | Open selected file (prompts for PIN or password) |
| `dd` | Remove file from history (with y/n confirmation) |
| `gg` / `G` | Go to top / bottom |
| `Ctrl-d` / `Ctrl-u` | Half-page down / up |
| `Ctrl-l` | Move focus to tree panel |

### KeePass Tool — Tree Panel

| Key | Action |
|-----|--------|
| `j` / `k` | Move down / up |
| `h` | Collapse group / go to parent |
| `l` / `Enter` | Expand group |
| `gg` / `G` | Go to top / bottom |
| `Ctrl-d` / `Ctrl-u` | Half-page down / up |
| `Ctrl-h` | Move focus to sidebar |
| `Ctrl-l` | Move focus to detail panel |
| `/` | Search entries (telescope-style overlay) |
| `p` | Toggle password visibility in detail panel |
| `yu` | Copy username to clipboard |
| `yp` | Copy password to clipboard (auto-clears after 30s) |
| `yU` | Copy URL to clipboard |

### KeePass Tool — Detail Panel

| Key | Action |
|-----|--------|
| `j` / `k` | Scroll up / down |
| `p` | Toggle password visibility |
| `yu` / `yp` / `yU` | Copy username / password / URL |
| `Ctrl-h` | Move focus to tree panel |

### KeePass Tool — Leader Keys

| Key | Action |
|-----|--------|
| `<Space>ko` | Open file picker (`~/keepass`) |
| `<Space>ke` | Toggle sidebar |
| `<Space>ks` | Search entries |
| `:open <path>` | Open a `.kdbx` file |

### Insert Mode (text input)

| Key | Action |
|-----|--------|
| `Enter` | Submit input / new line (body) |
| `Esc` | Cancel and return to Normal mode |
| `Left` / `Right` | Move cursor |
| `Backspace` | Delete character before cursor |

### Telescope (fuzzy finder)

| Key | Action |
|-----|--------|
| Type | Filter results |
| `Tab` / `Down` | Move selection down |
| `Shift-Tab` / `Up` | Move selection up |
| `Enter` | Select item and jump to it (cross-tool) |
| `Esc` | Close telescope |
| `Backspace` | Delete search character |

Telescope is global: it indexes items from all tools, and selecting a result
opens the owning tool and navigates directly to that item.

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
│   ├── rstools-todo/         # Todo list tool
│   │   └── src/
│   │       ├── lib.rs         # Tool trait impl, input handling, state
│   │       ├── model.rs       # Todo struct, SQLite CRUD operations
│   │       └── ui.rs          # Todo list and input rendering
│   ├── rstools-http/         # HTTP client & API explorer
│   │   └── src/
│   │       ├── lib.rs         # Tool trait impl, key handling, path creation
│   │       ├── model.rs       # Data models, SQLite CRUD (entries, requests, headers, params)
│   │       ├── sidebar.rs     # Tree state, flatten/sort, clipboard, navigation
│   │       ├── request_panel.rs # Request editor state (URL, headers, body, response)
│   │       ├── executor.rs    # Async HTTP executor (background tokio runtime)
│   │       └── ui.rs          # Full UI rendering (sidebar, request panel, response viewer)
│   └── rstools-keepass/      # KeePass KDBX4 vault viewer
│       └── src/
│           ├── lib.rs         # Tool trait impl, key handling, PIN/password prompts
│           ├── model.rs       # File history, SQLite CRUD, PIN storage
│           ├── crypto.rs      # AES-256-GCM PIN encryption, Argon2id key derivation
│           ├── sidebar.rs     # File history list, navigation, PIN status
│           ├── vault.rs       # KDBX4 parser, tree structure, search collection
│           ├── detail.rs      # Entry detail panel state, field display
│           └── ui.rs          # Full UI rendering (sidebar, tree, detail, overlays)
```

### How it works

- **rstools-core** is the shared foundation. It provides the `Tool` trait,
  vim-style input handling with multi-key sequences (`gg`, `dd`, `gt`), the
  which-key popup, telescope overlay, database connection, and shared UI
  components (tab bar, status bar, command line).

- **rstools-hub** is the only binary. It owns the event loop, manages a registry
  of tools, routes key events to the active tool, and renders everything. Tools
  are embedded views, similar to buffers in neovim.

- **rstools-todo** and **rstools-http** (and future tools) are library crates that
  implement the `Tool` trait. Each tool manages its own state, handles its own
  keybinds (delegating global ones back to the hub via the `Action` enum), renders
  its own UI, and owns its own database tables.

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

### HTTP schema

```sql
CREATE TABLE http_entries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    parent_id INTEGER REFERENCES http_entries(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    entry_type TEXT NOT NULL CHECK(entry_type IN ('folder', 'query')),
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE http_requests (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    entry_id INTEGER UNIQUE NOT NULL REFERENCES http_entries(id) ON DELETE CASCADE,
    method TEXT NOT NULL DEFAULT 'GET',
    url TEXT NOT NULL DEFAULT '',
    body TEXT NOT NULL DEFAULT '',
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE http_headers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    request_id INTEGER NOT NULL REFERENCES http_requests(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    sort_order INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE http_query_params (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    request_id INTEGER NOT NULL REFERENCES http_requests(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    sort_order INTEGER NOT NULL DEFAULT 0
);
```

## Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| ratatui | 0.30 | TUI rendering framework |
| crossterm | 0.29 | Terminal backend (input/output) |
| rusqlite | 0.38 | SQLite database (bundled) |
| reqwest | 0.12 | HTTP client (async, rustls) |
| tokio | 1 | Async runtime (background thread for HTTP) |
| serde_json | 1 | JSON pretty-printing for responses |
| chrono | 0.4 | Timestamps |
| directories | 6 | XDG-compliant data paths |
| anyhow | 1 | Error handling |
| unicode-width | 0.2 | Unicode text width calculation |

## License

MIT
