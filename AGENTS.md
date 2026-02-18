# AGENTS.md - rstools Development Guidelines

## Project Overview

rstools is a collection of independent terminal tools written in Rust, unified by a
single hub orchestrator. The UI is built with ratatui and the entire UX is modeled
after neovim.

## Architecture

- **Workspace layout**: Cargo workspace with crates under `crates/`
- **Hub model**: Embedded views — tools are library crates rendered as views inside
  the hub, like neovim buffers. Users switch between them without leaving the hub.
- **Database**: Single shared SQLite file (`~/.local/share/rstools/rstools.db`) with
  separate tables per tool. Managed via rusqlite with the `bundled` feature.
- **Binary**: The only binary is `rstools` (from `rstools-hub`). Tools are library
  crates that implement the `Tool` trait.

## Vim-First UX — Critical Conventions

This is the most important aspect of the project. Every tool MUST follow these rules:

### Input Modes
- **Normal** — default mode. Navigation with hjkl, actions via keybinds.
- **Insert** — text input (adding/editing items). Entered with `i`, `a`, `o`, etc.
  Exited with `Esc` or `Ctrl-[`.
- **Command** — command-line mode entered with `:`. Supports `:q`, `:w`, etc.

### Leader Key
- **Space** is the leader key (like LazyVim/modern neovim configs).
- Pressing Space in Normal mode opens the **which-key** popup.

### Which-Key
- Every keybind group and individual binding MUST be registered in which-key.
- When adding a new tool or keybind, ALWAYS update the which-key registration.
- Format: `<Space>` shows top-level groups, then next key shows subgroup/action.
- Example groups:
  - `<Space>t` — Todo
  - `<Space>h` — HTTP
  - `<Space>f` — Find (telescope)
  - `<Space>e` — Toggle HTTP explorer sidebar
  - `<Space>q` — Quit/Session

### Telescope (Fuzzy Finder)
- Triggered by `<Space>ff` (find), `<Space>fg` (grep), etc.
- Provides fuzzy matching over items from any tool.
- Each tool can register searchable items via the `Tool` trait.

### Standard Navigation Keybinds
These MUST be consistent across ALL tools:
- `j/k` — move down/up in lists
- `h/l` — collapse/expand or move left/right where applicable
- `gg` — go to top
- `G` — go to bottom
- `Ctrl-d/Ctrl-u` — half-page down/up
- `/` — search/filter within current view
- `Enter` — confirm/select/toggle
- `dd` — delete item
- `u` — undo (where applicable)
- `?` — show help

### Tool Switching (Hub)
- `<Space>1-9` — switch to tool by index
- `<Space><Space>` — tool picker (telescope over tools)
- `gt` / `gT` — next/previous tool tab

## Adding a New Tool

1. Create a new crate: `crates/rstools-<name>/`
2. Implement the `Tool` trait from `rstools-core`
3. Add database migration in the tool's `init_db()` method
4. Register the tool in `rstools-hub`'s tool registry
5. Add which-key bindings for the tool (MANDATORY)
6. Update this file's keybind reference
7. Update README.md with the new tool's section

## Data Model Conventions

- Keep models minimal — only fields that are truly needed
- Use `created_at` / `updated_at` timestamps (managed by SQLite)
- Use `INTEGER PRIMARY KEY AUTOINCREMENT` for IDs
- Each tool owns its own tables, prefixed if ambiguity is possible

## Current Tools

### Todo (`rstools-todo`)
- Tables: `todos`
- Model: id, title, completed, description (optional), created_at, updated_at
- Keybinds (Normal mode):
  - `j/k` — move up/down
  - `Enter` — toggle complete
  - `a` — add new todo (enters Insert mode)
  - `e` — edit selected todo (enters Insert mode)
  - `dd` — delete todo
  - `/` — search/filter todos
  - `o` — add todo below current

### HTTP (`rstools-http`)
- Tables: `http_entries`, `http_requests`, `http_headers`, `http_query_params`
- Models:
  - `HttpEntry`: id, parent_id, name, entry_type (folder/query), created_at, updated_at
  - `HttpRequest`: id, entry_id, method, url, body, created_at, updated_at
  - `HttpHeader`: id, request_id, key, value, enabled, sort_order
  - `HttpQueryParam`: id, request_id, key, value, enabled, sort_order
- Tree structure: folders contain queries and sub-folders, like neo-tree
- Layout: sidebar (40 chars, toggle with `<Space>e`) + content panel (request top / response bottom)
- HTTP methods: GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS
- Async requests via background tokio runtime with channel-based communication
- Persistence: explicit save with `:w` (dirty indicator `[+]` shown in title)
- JSON responses are auto-pretty-printed
- Keybinds (Normal mode, sidebar focused):
  - `j/k` — move up/down
  - `h` — collapse folder / go to parent
  - `l` / `Enter` — expand folder / open query in content panel
  - `gg` / `G` — go to top / bottom
  - `Ctrl-d` / `Ctrl-u` — half-page down / up
  - `Ctrl-l` — move focus to content panel
  - `a` — add entry (supports paths like `group/api/get-user`)
  - `r` — rename selected entry
  - `d` — delete selected entry (with y/n confirmation)
  - `y` — copy selected entry to clipboard
  - `x` — cut selected entry to clipboard
  - `p` — paste from clipboard (recursive for folders)
- Keybinds (Normal mode, content panel focused):
  - `Tab` / `Shift-Tab` — cycle sections (URL → Params → Headers → Body)
  - `Ctrl-h` — move focus to sidebar
  - `Ctrl-j` — move focus from request panel to response panel
  - `Ctrl-k` — move focus from response to request, or from request to sidebar
  - `Ctrl-l` — (from sidebar) move focus to content panel
  - `Ctrl-Enter` — send request
  - `<Space>s` — send request (leader key)
  - `m` / `M` — cycle HTTP method forward / backward
  - `f` — toggle fullscreen for the focused panel
  - `:w` — save request to database
- Keybinds (URL section):
  - `i` / `a` — enter insert mode to edit URL
- Keybinds (Params / Headers sections):
  - `j/k` — move up/down
  - `a` — add new key-value row
  - `i` / `Enter` — edit selected row (inline)
  - `dd` — delete selected row
  - `x` — toggle row enabled/disabled
  - `Tab` (in edit mode) — switch between key and value fields
- Keybinds (Body section):
  - `i/a/A/I` — enter insert mode
  - `o/O` — insert line below/above
  - `hjkl` — cursor movement
  - `0/$` — line start/end
- Keybinds (Response panel):
  - `j/k` — scroll response body/headers
  - `gg/G` — go to top/bottom
  - `Tab` — switch between Body and Headers tabs
- Which-key (`<Space>h`):
  - `s` — Send request
  - `e` — Toggle sidebar
  - `m` — Cycle method
- Path creation rules:
  - `group/api/get-user` — intermediate segments become folders, last becomes query
  - `group/api/` — trailing slash creates all segments as folders
  - Existing folders are reused (not duplicated)

### KeePass (`rstools-keepass`)
- Tables: `keepass_files`
- Model: id, file_path, display_name, encrypted_password, pin_salt, pin_nonce,
  pin_expires_at, last_opened_at, created_at, updated_at
- Read-only KDBX4 vault viewer using the `keepass` crate
- Layout: sidebar (40 chars, toggle with `<Space>ke`) + tree panel (40%) + detail panel (60%)
- Sidebar shows previously opened .kdbx files, ordered most-recently-opened first
- `[PIN]` indicator on files with active PIN quick-access
- Recycle Bin group is hidden from the tree
- Password fields masked with dots, toggled with `p`
- Security: best-effort zeroize via `zeroize` crate, AES-256-GCM PIN encryption via `aes-gcm` + `argon2`
- System clipboard via `arboard`, auto-clear after 30 seconds for passwords
- Auto-lock after 15 minutes of inactivity, shows lock screen
- PIN: 4-digit per-file PIN, valid for 30 days, prompted after successful password entry
- Search: telescope-style split overlay (fzf results left, preview right), searches titles only
- File picker: telescope-style, scans `~/keepass` recursively for `.kdbx` files
- Keybinds (Normal mode, sidebar focused):
  - `j/k` — move up/down
  - `Enter` — open selected file (prompts for PIN or password)
  - `dd` — remove file from history (with y/n confirmation)
  - `gg` / `G` — go to top / bottom
  - `Ctrl-d` / `Ctrl-u` — half-page down / up
  - `Ctrl-l` — move focus to tree panel
- Keybinds (Normal mode, tree panel focused):
  - `j/k` — move up/down
  - `h` — collapse group / go to parent
  - `l` / `Enter` — expand group
  - `gg` / `G` — go to top / bottom
  - `Ctrl-d` / `Ctrl-u` — half-page down / up
  - `Ctrl-h` — move focus to sidebar
  - `Ctrl-l` — move focus to detail panel
  - `/` — open search overlay
  - `p` — toggle password visibility in detail panel
  - `yu` — copy username to clipboard
  - `yp` — copy password to clipboard (auto-clears after 30s)
  - `yU` — copy URL to clipboard
- Keybinds (Normal mode, detail panel focused):
  - `j/k` — scroll up/down
  - `p` — toggle password visibility
  - `yu` — copy username
  - `yp` — copy password (auto-clears after 30s)
  - `yU` — copy URL
  - `Ctrl-h` — move focus to tree panel
- Keybinds (Lock screen):
  - `Enter` — unlock (prompts for PIN or password)
- Which-key (`<Space>k`):
  - `o` — Open file picker
  - `e` — Toggle sidebar
  - `s` — Search entries
- Commands:
  - `:open <path>` — open a .kdbx file

## Commit Conventions

- **Commit after each significant step** when building a feature — do NOT wait
  until the entire feature is done. For example, if implementing a new popup:
  commit after creating the module, commit after wiring it up, commit after
  adding tool-specific support, etc. Each commit should represent a coherent,
  compilable step.
- Use conventional commits: `feat:`, `fix:`, `chore:`, `docs:`, `refactor:`
- Keep which-key, README, and AGENTS.md updated with every change that affects
  keybinds or tool registration
