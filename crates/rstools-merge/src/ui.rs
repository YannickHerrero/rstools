use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::{conflict::HunkPreview, ConflictFile, ConflictKind};
use rstools_core::vim_editor::VimEditor;

pub const SIDEBAR_WIDTH: u16 = 40;

pub fn render_merge_tool(
    frame: &mut Frame,
    area: Rect,
    files: &[ConflictFile],
    in_git_repo: bool,
    selected: Option<usize>,
    sidebar_focused: bool,
    active_file: Option<&str>,
    active_kind: Option<ConflictKind>,
    editor: &VimEditor,
    hunk_info: Option<(usize, usize, Option<HunkPreview>)>,
    notification: Option<&str>,
    preview_scroll: u16,
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

    render_sidebar(
        frame,
        sidebar_area,
        files,
        in_git_repo,
        selected,
        sidebar_focused,
    );

    match (active_file, active_kind) {
        (Some(path), Some(ConflictKind::Text)) => render_text_conflict_content(
            frame,
            content_area,
            path,
            editor,
            hunk_info,
            !sidebar_focused,
            preview_scroll,
        ),
        (Some(path), Some(ConflictKind::Binary)) => {
            render_binary_content(frame, content_area, path, !sidebar_focused)
        }
        _ => render_empty_content(frame, content_area, in_git_repo),
    }

    if let Some(message) = notification {
        render_notification(frame, area, message);
    }
}

fn render_sidebar(
    frame: &mut Frame,
    area: Rect,
    files: &[ConflictFile],
    in_git_repo: bool,
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
        let empty_message = if in_git_repo {
            "No merge conflicts"
        } else {
            "Not a git repository"
        };
        vec![ListItem::new(Line::from(Span::styled(
            empty_message,
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
                    Span::styled(
                        format!(" ({})", file.status),
                        Style::default().fg(Color::DarkGray),
                    ),
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

fn render_empty_content(frame: &mut Frame, area: Rect, in_git_repo: bool) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Merge View ")
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines = if in_git_repo {
        vec![
            Line::from("No merge conflicts found in this repository."),
            Line::from(""),
            Line::from("Once conflicts exist, select a file from the sidebar."),
            Line::from("Top panes show hunk context and side-by-side choices."),
            Line::from("Bottom pane is a full editable merge result."),
        ]
    } else {
        vec![
            Line::from("Current folder is not a git repository."),
            Line::from(""),
            Line::from("Open rstools inside a git repo to use Merge."),
        ]
    };
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_binary_content(frame: &mut Frame, area: Rect, path: &str, focused: bool) {
    let border = if focused {
        Color::Blue
    } else {
        Color::DarkGray
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" Binary Conflict: {} ", path))
        .border_style(Style::default().fg(border));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines = vec![
        Line::from(Span::styled(
            "Binary conflict detected.",
            Style::default().fg(Color::Yellow),
        )),
        Line::from(""),
        Line::from("Accept changes with:"),
        Line::from(vec![
            Span::styled("co", Style::default().fg(Color::Cyan)),
            Span::raw(" accept ours"),
        ]),
        Line::from(vec![
            Span::styled("ct", Style::default().fg(Color::Cyan)),
            Span::raw(" accept theirs"),
        ]),
        Line::from(""),
        Line::from("Actions are applied with git checkout --ours/--theirs + git add."),
    ];

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_text_conflict_content(
    frame: &mut Frame,
    area: Rect,
    path: &str,
    editor: &VimEditor,
    hunk_info: Option<(usize, usize, Option<HunkPreview>)>,
    focused: bool,
    preview_scroll: u16,
) {
    let [top_area, bottom_area] =
        Layout::vertical([Constraint::Percentage(40), Constraint::Percentage(60)]).areas(area);
    let [left_top, right_top] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
            .areas(top_area);

    let hunk_title = hunk_info
        .as_ref()
        .map(|(idx, total, _)| {
            if *total == 0 {
                "Hunk 0/0".to_string()
            } else {
                format!("Hunk {}/{}", idx + 1, total)
            }
        })
        .unwrap_or_else(|| "Hunk 0/0".to_string());

    let preview = hunk_info.as_ref().and_then(|(_, _, p)| p.as_ref());
    render_hunk_pane(
        frame,
        left_top,
        " OURS ",
        preview,
        true,
        &hunk_title,
        preview_scroll,
    );
    render_hunk_pane(
        frame,
        right_top,
        " THEIRS ",
        preview,
        false,
        &hunk_title,
        preview_scroll,
    );

    let border_color = if focused {
        Color::Blue
    } else {
        Color::DarkGray
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(
            " Result: {}  |  Accept: co (ours), ct (theirs), cb (both) ",
            path
        ))
        .border_style(Style::default().fg(border_color));
    let inner = block.inner(bottom_area);
    frame.render_widget(block, bottom_area);
    editor.render(frame, inner, focused);
}

fn render_hunk_pane(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    preview: Option<&HunkPreview>,
    ours: bool,
    hunk_title: &str,
    scroll: u16,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!("{} {} ", title, hunk_title))
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(preview) = preview else {
        let text = Paragraph::new("No conflict hunks in current draft")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(text, inner);
        return;
    };

    let mut lines = Vec::new();
    for line in &preview.before {
        lines.push(Line::from(Span::styled(
            format!("  {line}"),
            Style::default().fg(Color::DarkGray),
        )));
    }

    lines.push(Line::from(Span::styled(
        "----------------",
        Style::default().fg(Color::DarkGray),
    )));

    let body = if ours { &preview.ours } else { &preview.theirs };
    for line in body {
        lines.push(Line::from(Span::styled(
            line.clone(),
            Style::default().fg(Color::White),
        )));
    }

    lines.push(Line::from(Span::styled(
        "----------------",
        Style::default().fg(Color::DarkGray),
    )));

    for line in &preview.after {
        lines.push(Line::from(Span::styled(
            format!("  {line}"),
            Style::default().fg(Color::DarkGray),
        )));
    }

    let max_scroll = (lines.len() as u16).saturating_sub(inner.height);
    let clamped_scroll = scroll.min(max_scroll);

    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((clamped_scroll, 0)),
        inner,
    );
}

fn render_notification(frame: &mut Frame, area: Rect, message: &str) {
    let width = (message.len() as u16 + 4).min(area.width.saturating_sub(4));
    let notification_area = Rect {
        x: area.x + area.width.saturating_sub(width) - 1,
        y: area.y + 1,
        width,
        height: 1,
    };

    let paragraph = Paragraph::new(Line::from(Span::styled(
        format!(" {} ", message),
        Style::default()
            .fg(Color::Black)
            .bg(Color::Green)
            .add_modifier(Modifier::BOLD),
    )));
    frame.render_widget(paragraph, notification_area);
}
