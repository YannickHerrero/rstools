use crate::sidebar::SidebarState;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use unicode_width::UnicodeWidthStr;

/// Fixed sidebar width in characters.
pub const SIDEBAR_WIDTH: u16 = 40;

/// Render the entire HTTP tool view.
pub fn render_http_tool(frame: &mut Frame, area: Rect, sidebar: &SidebarState) {
    if sidebar.visible {
        // Split into sidebar + main content
        let sidebar_width = SIDEBAR_WIDTH.min(area.width.saturating_sub(10));
        let sidebar_area = Rect {
            x: area.x,
            y: area.y,
            width: sidebar_width,
            height: area.height,
        };
        let content_area = Rect {
            x: area.x + sidebar_width,
            y: area.y,
            width: area.width.saturating_sub(sidebar_width),
            height: area.height,
        };

        render_sidebar(frame, sidebar_area, sidebar);
        render_placeholder(frame, content_area);
    } else {
        render_placeholder(frame, area);
    }
}

/// Render the sidebar tree.
fn render_sidebar(frame: &mut Frame, area: Rect, sidebar: &SidebarState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" HTTP Explorer ");

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    // Reserve space for input prompt if active
    let (tree_area, input_area) = if sidebar.input_mode != crate::sidebar::SidebarInput::None {
        let input_height = 1;
        if inner.height <= input_height {
            (inner, None)
        } else {
            let tree = Rect {
                height: inner.height - input_height,
                ..inner
            };
            let input = Rect {
                y: inner.y + inner.height - input_height,
                height: input_height,
                ..inner
            };
            (tree, Some(input))
        }
    } else {
        (inner, None)
    };

    // Render tree entries
    render_tree_entries(frame, tree_area, sidebar);

    // Render input prompt if active
    if let Some(input_area) = input_area {
        render_input_prompt(frame, input_area, sidebar);
    }
}

/// Style for the dim vertical indent guide lines.
const GUIDE_STYLE: Style = Style::new().fg(Color::DarkGray);

/// Background color for the selected/highlighted line — a subtle dark grey
/// that's light enough for guide lines to remain visible through it.
const SELECTED_BG: Color = Color::Rgb(50, 50, 50);

/// Render the tree entries in the sidebar.
fn render_tree_entries(frame: &mut Frame, area: Rect, sidebar: &SidebarState) {
    if sidebar.flat_view.is_empty() {
        // Even with no entries, show the blank root line if it's selected
        if sidebar.selected == 0 {
            let highlight = Style::default().bg(SELECTED_BG);
            let blank = Line::from(Span::styled(" ".repeat(area.width as usize), highlight));
            let widget = Paragraph::new(vec![blank]);
            frame.render_widget(widget, area);
        } else {
            let empty = Paragraph::new("  No entries yet. Press 'a' to add.")
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(empty, area);
        }
        return;
    }

    // Total renderable items: flat_view entries + 1 blank root line
    let total_items = sidebar.flat_view.len() + 1;
    let visible_lines = area.height as usize;

    // Calculate scroll offset to keep selection visible
    let scroll_offset = if sidebar.selected >= visible_lines {
        sidebar.selected - visible_lines + 1
    } else {
        0
    };

    let mut lines: Vec<Line> = Vec::new();

    for item_idx in scroll_offset..total_items.min(scroll_offset + visible_lines) {
        if item_idx < sidebar.flat_view.len() {
            // Regular tree entry
            let entry = &sidebar.flat_view[item_idx];
            lines.push(render_entry_line(entry, item_idx, sidebar, area.width));
        } else {
            // Blank root line (one past last entry)
            let is_selected = item_idx == sidebar.selected;
            if is_selected {
                let highlight = Style::default().bg(SELECTED_BG);
                lines.push(Line::from(Span::styled(
                    " ".repeat(area.width as usize),
                    highlight,
                )));
            } else {
                lines.push(Line::from(""));
            }
        }
    }

    let tree_widget = Paragraph::new(lines);
    frame.render_widget(tree_widget, area);
}

/// Render a single tree entry line with indent guides, icon, and name.
fn render_entry_line(
    entry: &crate::sidebar::FlatEntry,
    idx: usize,
    sidebar: &SidebarState,
    area_width: u16,
) -> Line<'static> {
    let is_selected = idx == sidebar.selected;
    let is_cut = sidebar
        .clipboard
        .as_ref()
        .map(|c| c.entry_id == entry.entry_id && c.mode == crate::sidebar::ClipboardMode::Cut)
        .unwrap_or(false);

    let base_style = if is_selected {
        Style::default()
            .bg(SELECTED_BG)
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else if is_cut {
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::DIM)
    } else {
        match entry.entry_type {
            crate::model::EntryType::Folder => Style::default().fg(Color::Blue),
            crate::model::EntryType::Query => Style::default().fg(Color::White),
        }
    };

    // Build indent with guide lines
    let mut spans: Vec<Span<'static>> = Vec::new();

    for d in 0..entry.depth {
        let has_guide = entry.guide_depths.get(d).copied().unwrap_or(false);
        if has_guide {
            // Guide lines keep their normal dim style; the selected line's
            // lighter background lets them show through naturally.
            let guide_style = if is_selected {
                GUIDE_STYLE.bg(SELECTED_BG)
            } else {
                GUIDE_STYLE
            };
            spans.push(Span::styled("\u{2502} ", guide_style));
        } else {
            spans.push(Span::styled("  ", base_style));
        }
    }

    // Icon: ▼ for expanded folder, ▶ for collapsed folder, ● for query
    let icon: &str = match entry.entry_type {
        crate::model::EntryType::Folder => {
            if entry.is_expanded {
                "\u{25BC} "
            } else {
                "\u{25B6} "
            }
        }
        crate::model::EntryType::Query => "\u{25CF} ",
    };
    spans.push(Span::styled(icon.to_string(), base_style));

    // Name
    spans.push(Span::styled(entry.name.clone(), base_style));

    // If selected, pad the rest of the line with the highlight background
    if is_selected {
        let content_width: usize = spans.iter().map(|s| s.content.width()).sum();
        let remaining = (area_width as usize).saturating_sub(content_width);
        if remaining > 0 {
            spans.push(Span::styled(
                " ".repeat(remaining),
                Style::default().bg(SELECTED_BG),
            ));
        }
    }

    Line::from(spans)
}

/// Render the input prompt at the bottom of the sidebar.
fn render_input_prompt(frame: &mut Frame, area: Rect, sidebar: &SidebarState) {
    let (label, input_text) = match &sidebar.input_mode {
        crate::sidebar::SidebarInput::Adding => ("New: ", &sidebar.input_buffer),
        crate::sidebar::SidebarInput::Renaming => ("Name: ", &sidebar.input_buffer),
        crate::sidebar::SidebarInput::ConfirmDelete => {
            let name = sidebar
                .selected_entry()
                .map(|e| e.name.as_str())
                .unwrap_or("?");
            // We'll render this specially
            let prompt = format!("Delete {}? (y/n)", name);
            let line = Line::from(vec![Span::styled(
                prompt,
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )]);
            let widget = Paragraph::new(line);
            frame.render_widget(widget, area);
            return;
        }
        crate::sidebar::SidebarInput::None => return,
    };

    let line = Line::from(vec![
        Span::styled(
            label,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(input_text.as_str()),
    ]);

    let widget = Paragraph::new(line);
    frame.render_widget(widget, area);

    // Position the cursor
    let cursor_x = area.x + label.len() as u16 + sidebar.input_cursor as u16;
    let cursor_y = area.y;
    if cursor_x < area.x + area.width {
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

/// Render the placeholder main content area.
fn render_placeholder(frame: &mut Frame, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" Request ");

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let text = Paragraph::new("Select a query to begin")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(ratatui::layout::Alignment::Center);

    // Center vertically
    if inner.height > 1 {
        let centered_area = Rect {
            y: inner.y + inner.height / 2,
            height: 1,
            ..inner
        };
        frame.render_widget(text, centered_area);
    } else {
        frame.render_widget(text, inner);
    }
}
