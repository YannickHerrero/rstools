use crate::sidebar::SidebarState;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

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

/// Render the tree entries in the sidebar.
fn render_tree_entries(frame: &mut Frame, area: Rect, sidebar: &SidebarState) {
    if sidebar.flat_view.is_empty() {
        let empty = Paragraph::new("  No entries yet. Press 'a' to add.")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(empty, area);
        return;
    }

    // Calculate scroll offset to keep selection visible
    let visible_lines = area.height as usize;
    let scroll_offset = if sidebar.selected >= visible_lines {
        sidebar.selected - visible_lines + 1
    } else {
        0
    };

    let lines: Vec<Line> = sidebar
        .flat_view
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(visible_lines)
        .map(|(idx, entry)| {
            let indent = "  ".repeat(entry.depth);

            let icon = match entry.entry_type {
                crate::model::EntryType::Folder => {
                    if entry.is_expanded {
                        " "
                    } else {
                        " "
                    }
                }
                crate::model::EntryType::Query => "● ",
            };

            let is_selected = idx == sidebar.selected;
            let is_cut = sidebar
                .clipboard
                .as_ref()
                .map(|c| {
                    c.entry_id == entry.entry_id && c.mode == crate::sidebar::ClipboardMode::Cut
                })
                .unwrap_or(false);

            let style = if is_selected {
                Style::default()
                    .bg(Color::DarkGray)
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

            Line::from(vec![
                Span::styled(indent, style),
                Span::styled(icon, style),
                Span::styled(entry.name.clone(), style),
            ])
        })
        .collect();

    let tree_widget = Paragraph::new(lines);
    frame.render_widget(tree_widget, area);
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
