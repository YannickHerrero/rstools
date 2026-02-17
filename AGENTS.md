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
  - `<Space>f` — Find (telescope)
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

## Commit Conventions

- **Commit after each significant step** when building a feature — do NOT wait
  until the entire feature is done. For example, if implementing a new popup:
  commit after creating the module, commit after wiring it up, commit after
  adding tool-specific support, etc. Each commit should represent a coherent,
  compilable step.
- Use conventional commits: `feat:`, `fix:`, `chore:`, `docs:`, `refactor:`
- Keep which-key, README, and AGENTS.md updated with every change that affects
  keybinds or tool registration
