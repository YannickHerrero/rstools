use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::{ConflictFile, ConflictKind};

pub const SIDEBAR_WIDTH: u16 = 40;

pub fn render_merge_tool(
    frame: &mut Frame,
    area: Rect,
    files: &[ConflictFile],
    selected: Option<usize>,
    sidebar_focused: bool,
    active_file: Option<&str>,
) {
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

    render_sidebar(frame, sidebar_area, files, selected, sidebar_focused);
    render_placeholder_content(frame, content_area, active_file);
}

fn render_sidebar(
    frame: &mut Frame,
    area: Rect,
    files: &[ConflictFile],
    selected: Option<usize>,
    focused: bool,
) {
    let border_color = if focused {
        Color::Blue
    } else {
        Color::DarkGray
    };
    let title = format!(" Conflicts ({}) ", files.len());
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(border_color));

    let items: Vec<ListItem> = if files.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "No unmerged files",
            Style::default().fg(Color::DarkGray),
        )))]
    } else {
        files
            .iter()
            .map(|file| {
                let kind = match file.kind {
                    ConflictKind::Text => "TXT",
                    ConflictKind::Binary => "BIN",
                };

                let kind_style = match file.kind {
                    ConflictKind::Text => Style::default().fg(Color::Cyan),
                    ConflictKind::Binary => Style::default().fg(Color::Yellow),
                };

                ListItem::new(Line::from(vec![
                    Span::styled(format!("[{kind}] "), kind_style),
                    Span::raw(file.path.clone()),
                ]))
            })
            .collect()
    };

    let mut state = ListState::default();
    state.select(selected);

    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .bg(Color::Gray)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(list, area, &mut state);
}

fn render_placeholder_content(frame: &mut Frame, area: Rect, active_file: Option<&str>) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Merge View ")
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines = if let Some(path) = active_file {
        vec![
            Line::from(Span::styled(
                path,
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("Conflict resolver panes will appear here."),
            Line::from(""),
            Line::from(vec![
                Span::styled("Enter", Style::default().fg(Color::Yellow)),
                Span::raw(" open file  "),
                Span::styled("<Space>r", Style::default().fg(Color::Yellow)),
                Span::raw(" refresh list"),
            ]),
        ]
    } else {
        vec![
            Line::from("Select a conflicted file from the sidebar."),
            Line::from(""),
            Line::from("Only unmerged index entries are shown."),
        ]
    };

    frame.render_widget(Paragraph::new(lines), inner);
}
