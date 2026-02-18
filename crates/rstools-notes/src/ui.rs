use crate::sidebar::{render_tree_sidebar, SidebarState, TreeSidebarRenderConfig};
use ratatui::{
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};
use rstools_core::vim_editor::VimEditor;

/// Fixed sidebar width in characters.
pub const SIDEBAR_WIDTH: u16 = 40;

// ── Main entry point ─────────────────────────────────────────────────

/// Render the entire Notes tool view.
pub fn render_notes_tool(
    frame: &mut Frame,
    area: Rect,
    sidebar: &SidebarState,
    editor: &VimEditor,
    sidebar_focused: bool,
    active_note_name: Option<&str>,
) {
    if sidebar.visible {
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

        render_sidebar(frame, sidebar_area, sidebar, sidebar_focused);
        render_editor_panel(
            frame,
            content_area,
            editor,
            !sidebar_focused,
            active_note_name,
        );
    } else {
        render_editor_panel(frame, area, editor, true, active_note_name);
    }
}

// ── Sidebar ──────────────────────────────────────────────────────────

fn render_sidebar(frame: &mut Frame, area: Rect, sidebar: &SidebarState, focused: bool) {
    let config = TreeSidebarRenderConfig {
        title: " Notes ",
        focused,
        leaf_icon: Some("\u{25A0} "), // filled square
        leaf_style: Some(Style::default().fg(Color::White)),
        folder_style: Some(Style::default().fg(Color::Blue)),
    };
    render_tree_sidebar(frame, area, sidebar, &config);
}

// ── Editor Panel ─────────────────────────────────────────────────────

fn render_editor_panel(
    frame: &mut Frame,
    area: Rect,
    editor: &VimEditor,
    focused: bool,
    note_name: Option<&str>,
) {
    match note_name {
        Some(name) => {
            // Build title with dirty indicator
            let dirty = if editor.is_dirty() { " [+]" } else { "" };
            let title = format!(" {}{} ", name, dirty);

            let border_color = if focused {
                Color::White
            } else {
                Color::DarkGray
            };
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .title(title);

            let inner = block.inner(area);
            frame.render_widget(block, area);

            // Render the vim editor inside the block
            editor.render(frame, inner, focused);
        }
        None => {
            render_empty_panel(frame, area);
        }
    }
}

fn render_empty_panel(frame: &mut Frame, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let msg = "Select a note to edit";
    let text = Paragraph::new(Line::from(vec![Span::styled(
        msg,
        Style::default().fg(Color::DarkGray),
    )]))
    .alignment(ratatui::layout::Alignment::Center);

    // Center vertically
    if inner.height > 0 {
        let y = inner.y + inner.height / 2;
        let centered = Rect {
            x: inner.x,
            y,
            width: inner.width,
            height: 1,
        };
        frame.render_widget(text, centered);
    }
}

pub fn render_grep_overlay(
    frame: &mut Frame,
    area: Rect,
    query: &str,
    results: &[String],
    selected: usize,
    preview_title: &str,
    preview_text: &str,
    preview_target_line: Option<usize>,
) {
    let popup_width = (area.width * 80 / 100)
        .max(50)
        .min(area.width.saturating_sub(4));
    let popup_height = (area.height * 70 / 100)
        .max(12)
        .min(area.height.saturating_sub(4));

    let vertical = Layout::vertical([Constraint::Length(popup_height)]).flex(Flex::Center);
    let horizontal = Layout::horizontal([Constraint::Length(popup_width)]).flex(Flex::Center);
    let [popup_area] = vertical.areas(area);
    let [popup_area] = horizontal.areas(popup_area);

    frame.render_widget(Clear, popup_area);

    let [input_area, content_area] =
        Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).areas(popup_area);

    let [results_area, preview_area] =
        Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)])
            .areas(content_area);

    let input = Paragraph::new(Line::from(vec![
        Span::styled("> ", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(query),
    ]))
    .block(
        Block::default()
            .title(" Grep Notes ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::White)),
    );
    frame.render_widget(input, input_area);

    frame.set_cursor_position((input_area.x + 3 + query.len() as u16, input_area.y + 1));

    let items: Vec<ListItem> = if results.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "No matches",
            Style::default().fg(Color::DarkGray),
        )))]
    } else {
        results
            .iter()
            .map(|r| ListItem::new(Line::from(Span::raw(r))))
            .collect()
    };

    let mut list_state = ListState::default();
    if !results.is_empty() {
        list_state.select(Some(selected.min(results.len() - 1)));
    }

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Results "))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD))
        .highlight_symbol("> ");

    frame.render_stateful_widget(list, results_area, &mut list_state);

    let preview_height = preview_area.height.saturating_sub(2) as usize; // account for borders
    let preview_scroll = if let Some(target_line) = preview_target_line {
        if preview_height == 0 {
            0
        } else {
            let half = preview_height / 2;
            target_line.saturating_sub(half) as u16
        }
    } else {
        0
    };

    let mut preview_lines: Vec<Line> = preview_text
        .lines()
        .enumerate()
        .map(|(i, line)| {
            if Some(i) == preview_target_line {
                Line::from(Span::styled(
                    line.to_string(),
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ))
            } else {
                Line::from(Span::raw(line.to_string()))
            }
        })
        .collect();
    if preview_lines.is_empty() {
        preview_lines.push(Line::from(""));
    }

    let preview = Paragraph::new(preview_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" Preview: {} ", preview_title)),
        )
        .scroll((preview_scroll, 0))
        .wrap(ratatui::widgets::Wrap { trim: false });
    frame.render_widget(preview, preview_area);
}
