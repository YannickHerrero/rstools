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
| Hub | WIP | Main orchestrator — tool picker, tab switching, dashboard |
| Todo | WIP | Minimalist todo list with search and filtering |

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
| `Esc` | Return to Normal mode |
| `Ctrl-c` | Force quit |

### Normal Mode — Global

| Key | Action |
|-----|--------|
| `<Space>` | Open which-key (leader menu) |
| `<Space><Space>` | Tool picker (telescope) |
| `<Space>ff` | Find (telescope fuzzy finder) |
| `<Space>1-9` | Switch to tool by index |
| `gt` / `gT` | Next / previous tool |
| `:q` | Close current tool / quit |
| `?` | Help |

### Normal Mode — List Navigation (all tools)

| Key | Action |
|-----|--------|
| `j` / `k` | Move down / up |
| `gg` | Go to top |
| `G` | Go to bottom |
| `Ctrl-d` / `Ctrl-u` | Half-page down / up |
| `/` | Search / filter |
| `Enter` | Confirm / select / toggle |
| `dd` | Delete item |

### Todo Tool

| Key | Action |
|-----|--------|
| `<Space>ta` | Add new todo |
| `a` | Add new todo (in todo view) |
| `e` | Edit selected todo |
| `o` | Add todo below current |
| `Enter` | Toggle completed |
| `dd` | Delete todo |
| `/` | Filter todos |

## Architecture

```
rstools/
├── crates/
│   ├── rstools-core/    # Shared: vim keybinds, which-key, telescope, db, UI
│   ├── rstools-hub/     # The orchestrator binary
│   └── rstools-todo/    # Todo list tool
```

- **rstools-core** — every tool depends on this. It provides the `Tool` trait,
  vim-style input handling, which-key popup, telescope overlay, database
  connection, and shared UI components.
- **rstools-hub** — the only binary. It manages a registry of tools and renders
  them as embedded views.
- **rstools-todo** (and future tools) — library crates that implement the `Tool`
  trait and provide their own UI and data models.

## Data Storage

All data lives in `~/.local/share/rstools/rstools.db` (XDG-compliant). Each tool
manages its own tables. Back up this single file to back up everything.

## License

MIT
