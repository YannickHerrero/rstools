# rstools Usage Guide

This guide contains the detailed usage and keybinding references that were moved
out of the main README.

## Core Concepts

- **Modes**: Normal, Insert, Command.
- **Leader key**: `Space` opens which-key in Normal mode.
- **Global fuzzy find**: `<Space><Space>` opens the tool picker.
- **Modal quit**: `:q` closes current context, `:qa` quits all.

## Demo Mode

Use demo mode when taking screenshots:

```bash
rstools --demo
```

- Uses an isolated database at `./.demo/rstools-demo.db`.
- Seeds Todo, HTTP, and KeePass mock data (idempotent).
- Demo KeePass files under `/demo/vaults/*.kdbx` open directly with in-memory sample entries.
- Keeps your regular `~/.local/share/rstools/rstools.db` untouched.

## Global

### Global Keys

| Key | Action |
|-----|--------|
| `Esc` | Return to Normal mode / cancel input |
| `Ctrl-c` | Force quit |
| `?` | Help |

### Normal Mode (Cross-tool)

| Key | Action |
|-----|--------|
| `<Space>` | Open which-key leader menu |
| `<Space><Space>` | Tool picker |
| `<Space>1-9` | Switch to tool by index |
| `gt` / `gT` | Next / previous tool tab |
| `j` / `k` | Move down / up |
| `gg` / `G` | Go to top / bottom |
| `Ctrl-d` / `Ctrl-u` | Half-page down / up |
| `/` | Search / filter current view |
| `Enter` | Confirm / select / toggle |
| `dd` | Delete item |

## Todo

### Leader

| Key | Action |
|-----|--------|
| `<Space>t` | Switch to Todo (or open Todo submenu) |

### In Tool

| Key | Action |
|-----|--------|
| `a` | Add new todo |
| `o` | Add todo below current |
| `e` | Edit selected todo |
| `Enter` | Toggle completed |
| `dd` | Delete todo |
| `/` | Filter todos |
| `j` / `k` | Move down / up |
| `gg` / `G` | Jump top / bottom |

## HTTP

### Leader

| Key | Action |
|-----|--------|
| `<Space>h` | Switch to HTTP tool (or open HTTP submenu) |
| `<Space>hs` | Send request |
| `<Space>he` | Toggle explorer sidebar |
| `<Space>hm` | Cycle method |

### Sidebar

| Key | Action |
|-----|--------|
| `a` | Add entry (`group/api/get-user`) |
| `r` | Rename selected entry |
| `d` | Delete selected entry |
| `y` / `x` / `p` | Copy / cut / paste entries |
| `h` | Collapse folder / go to parent |
| `l` / `Enter` | Expand folder / open query |
| `Ctrl-l` | Move focus to content panel |

Path rules:

- `group/api/get-user` creates folders + query.
- `group/api/` creates folders only.
- Existing folders are reused.

### Request Panel

| Key | Action |
|-----|--------|
| `Tab` / `Shift-Tab` | Cycle URL/Params/Headers/Body |
| `Ctrl-h` | Focus sidebar |
| `Ctrl-j` | Focus response panel |
| `Ctrl-k` | Move focus back toward request/sidebar |
| `Ctrl-Enter` | Send request |
| `m` / `M` | Cycle method forward / backward |
| `f` | Toggle fullscreen focused panel |
| `:w` | Save request |

Section-specific:

- **URL**: `i` / `a` to edit.
- **Params/Headers**: `a`, `i`/`Enter`, `dd`, `x`, and `Tab` while editing.
- **Body**: `i/a/A/I`, `o/O`, `hjkl`, `0/$`.

### Response Panel

| Key | Action |
|-----|--------|
| `j` / `k` | Scroll body / headers |
| `gg` / `G` | Go top / bottom |
| `Ctrl-d` / `Ctrl-u` | Half-page down / up |
| `Tab` | Switch Body/Headers tab |

## KeePass

### Leader

| Key | Action |
|-----|--------|
| `<Space>k` | Switch to KeePass tool (or open KeePass submenu) |
| `<Space>ko` | Open file picker |
| `<Space>ke` | Toggle sidebar |
| `<Space>ks` | Search entries |
| `:open <path>` | Open `.kdbx` file |

### Sidebar (File History)

| Key | Action |
|-----|--------|
| `j` / `k` | Move down / up |
| `Enter` | Open selected file |
| `dd` | Remove file from history |
| `Ctrl-l` | Focus tree panel |

### Tree Panel

| Key | Action |
|-----|--------|
| `j` / `k` | Move down / up |
| `h` | Collapse group / parent |
| `l` / `Enter` | Expand group |
| `Ctrl-h` / `Ctrl-l` | Focus sidebar / detail |
| `/` | Search entries overlay |
| `p` | Toggle password visibility |
| `yu` / `yp` / `yU` | Copy username / password / URL |

### Detail Panel

| Key | Action |
|-----|--------|
| `j` / `k` | Scroll |
| `p` | Toggle password visibility |
| `yu` / `yp` / `yU` | Copy username / password / URL |
| `Ctrl-h` | Focus tree panel |

## Insert Mode

| Key | Action |
|-----|--------|
| `Enter` | Submit input / new line |
| `Esc` | Return to Normal mode |
| `Left` / `Right` | Move cursor |
| `Backspace` | Delete previous character |

## Telescope Overlay

| Key | Action |
|-----|--------|
| Type | Filter results |
| `Tab` / `Down` | Move selection down |
| `Shift-Tab` / `Up` | Move selection up |
| `Enter` | Select result |
| `Esc` | Close overlay |
| `Backspace` | Delete search character |
