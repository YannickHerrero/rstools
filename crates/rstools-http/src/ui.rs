use crate::model::HttpMethod;
use crate::request_panel::{KvField, KvRow, PanelFocus, RequestPanel, ResponseSection, Section};
use crate::sidebar::{SidebarState, TreeSidebarRenderConfig, render_tree_sidebar};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use unicode_width::UnicodeWidthStr;

/// Fixed sidebar width in characters.
pub const SIDEBAR_WIDTH: u16 = 40;

// ── Colors ───────────────────────────────────────────────────────────

fn method_color(method: HttpMethod) -> Color {
    match method {
        HttpMethod::Get => Color::Green,
        HttpMethod::Post => Color::Yellow,
        HttpMethod::Put => Color::Blue,
        HttpMethod::Patch => Color::Rgb(255, 165, 0), // orange
        HttpMethod::Delete => Color::Red,
        HttpMethod::Head => Color::Cyan,
        HttpMethod::Options => Color::Magenta,
    }
}

fn status_color(code: u16) -> Color {
    match code {
        200..=299 => Color::Green,
        300..=399 => Color::Cyan,
        400..=499 => Color::Yellow,
        500..=599 => Color::Red,
        _ => Color::White,
    }
}

// ── Main entry point ─────────────────────────────────────────────────

/// Render the entire HTTP tool view.
pub fn render_http_tool(
    frame: &mut Frame,
    area: Rect,
    sidebar: &SidebarState,
    panel: &RequestPanel,
    sidebar_focused: bool,
    notification: Option<&str>,
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
        render_content_panel(frame, content_area, panel, !sidebar_focused);
    } else {
        render_content_panel(frame, area, panel, true);
    }

    if let Some(message) = notification {
        render_notification(frame, area, message);
    }
}

// ── Sidebar ──────────────────────────────────────────────────────────

const SELECTED_BG: Color = Color::Gray;

fn render_sidebar(frame: &mut Frame, area: Rect, sidebar: &SidebarState, focused: bool) {
    let config = TreeSidebarRenderConfig {
        title: " HTTP Explorer ",
        focused,
        leaf_icon: Some("\u{25CF} "),
        leaf_style: Some(Style::default().fg(Color::White)),
        folder_style: Some(Style::default().fg(Color::Blue)),
    };
    render_tree_sidebar(frame, area, sidebar, &config);
}

// ── Content Panel ────────────────────────────────────────────────────

fn render_content_panel(frame: &mut Frame, area: Rect, panel: &RequestPanel, focused: bool) {
    if !panel.is_active() {
        render_empty_panel(frame, area);
        return;
    }

    let request_focused = focused && panel.panel_focus == PanelFocus::Request;
    let response_focused = focused && panel.panel_focus == PanelFocus::Response;

    // Fullscreen: render only the focused panel at full height
    match panel.fullscreen {
        Some(PanelFocus::Request) => {
            render_request_area(frame, area, panel, request_focused);
            return;
        }
        Some(PanelFocus::Response) => {
            render_response_area(frame, area, panel, response_focused);
            return;
        }
        None => {}
    }

    // Normal split: request area (top 30%) and response area (bottom 70%)
    let request_height = (area.height * 30 / 100).max(5);
    let response_height = area.height.saturating_sub(request_height);

    let request_area = Rect {
        height: request_height,
        ..area
    };
    let response_area = Rect {
        y: area.y + request_height,
        height: response_height,
        ..area
    };

    render_request_area(frame, request_area, panel, request_focused);
    render_response_area(frame, response_area, panel, response_focused);
}

fn render_empty_panel(frame: &mut Frame, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" Request ");

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let text = Paragraph::new("Select a query to begin (Enter on a query in the sidebar)")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(ratatui::layout::Alignment::Center);

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

// ── Request area ─────────────────────────────────────────────────────

fn render_request_area(frame: &mut Frame, area: Rect, panel: &RequestPanel, focused: bool) {
    let title = if panel.dirty {
        format!(" {} [+] ", panel.active_entry_name)
    } else {
        format!(" {} ", panel.active_entry_name)
    };

    let border_color = if focused {
        Color::Blue
    } else {
        Color::DarkGray
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(title);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 3 || inner.width < 10 {
        return;
    }

    // Layout: method+URL (1 line) + section tabs (1 line) + section content (rest)
    let url_area = Rect { height: 1, ..inner };
    let tabs_area = Rect {
        y: inner.y + 1,
        height: 1,
        ..inner
    };
    let content_area = Rect {
        y: inner.y + 2,
        height: inner.height.saturating_sub(2),
        ..inner
    };

    render_method_url_bar(frame, url_area, panel, focused);
    render_section_tabs(frame, tabs_area, panel, focused);

    if content_area.height > 0 {
        match panel.focused_section {
            Section::Url => {
                // URL section shows hints when focused
                render_url_hints(frame, content_area);
            }
            Section::Params => {
                render_kv_section(
                    frame,
                    content_area,
                    &panel.query_params,
                    panel.params_selected,
                    panel,
                    focused && panel.focused_section == Section::Params,
                );
            }
            Section::Headers => {
                render_kv_section(
                    frame,
                    content_area,
                    &panel.headers,
                    panel.headers_selected,
                    panel,
                    focused && panel.focused_section == Section::Headers,
                );
            }
            Section::Body => {
                render_body_editor(frame, content_area, panel, focused);
            }
        }
    }
}

fn render_method_url_bar(frame: &mut Frame, area: Rect, panel: &RequestPanel, focused: bool) {
    let method = panel.method;
    let color = method_color(method);

    let method_style = Style::default()
        .fg(Color::Black)
        .bg(color)
        .add_modifier(Modifier::BOLD);

    let url_style = if focused && panel.focused_section == Section::Url {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let method_text = format!(" {} ", method.as_str());
    let url_text = if panel.url.is_empty() {
        "Enter URL...".to_string()
    } else {
        panel.url.clone()
    };

    let url_fg = if panel.url.is_empty() && !(focused && panel.focused_section == Section::Url) {
        Style::default().fg(Color::DarkGray)
    } else {
        url_style
    };

    let line = Line::from(vec![
        Span::styled(method_text, method_style),
        Span::raw(" "),
        Span::styled(url_text, url_fg),
    ]);

    frame.render_widget(Paragraph::new(line), area);

    // Show cursor on URL when editing
    if focused && panel.focused_section == Section::Url && panel.editing {
        let method_width = panel.method.as_str().len() + 3; // " METHOD " + " "
        let cursor_x = area.x + method_width as u16 + panel.url_cursor as u16;
        if cursor_x < area.x + area.width {
            frame.set_cursor_position((cursor_x, area.y));
        }
    }
}

fn render_section_tabs(frame: &mut Frame, area: Rect, panel: &RequestPanel, focused: bool) {
    let sections = [Section::Params, Section::Headers, Section::Body];
    let mut spans: Vec<Span> = Vec::new();
    spans.push(Span::raw(" "));

    for (i, section) in sections.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
        }

        let is_active = panel.focused_section == *section;
        let style = if is_active && focused {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else if is_active {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::UNDERLINED)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        spans.push(Span::styled(section.label(), style));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_url_hints(frame: &mut Frame, area: Rect) {
    let hints = vec![Line::from(vec![
        Span::styled(
            "i",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" edit URL  ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            "m",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" cycle method  ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            "Tab",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" next section  ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            "Ctrl-Enter",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" send", Style::default().fg(Color::DarkGray)),
    ])];
    let widget = Paragraph::new(hints);
    frame.render_widget(widget, area);
}

// ── Key-Value section (headers / params) ─────────────────────────────

fn render_kv_section(
    frame: &mut Frame,
    area: Rect,
    rows: &[KvRow],
    selected: usize,
    panel: &RequestPanel,
    focused: bool,
) {
    if area.height == 0 {
        return;
    }

    if rows.is_empty() {
        let hint = Paragraph::new("  No entries. Press 'a' to add.")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(hint, area);
        return;
    }

    // Column widths: [x] (3) + key (dynamic) + = (3) + value (rest)
    let toggle_width: u16 = 4;
    let separator_width: u16 = 3; // " = "
    let available = area.width.saturating_sub(toggle_width + separator_width);
    let key_width = available / 3;
    let value_width = available.saturating_sub(key_width);

    let visible_lines = area.height as usize;
    let scroll_offset = if selected >= visible_lines {
        selected - visible_lines + 1
    } else {
        0
    };

    let mut lines: Vec<Line> = Vec::new();

    for (i, row) in rows
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(visible_lines)
    {
        let is_selected = i == selected && focused;
        let is_editing = is_selected && panel.editing;

        // Toggle indicator
        let toggle = if row.enabled { "[x]" } else { "[ ]" };
        let toggle_style = if is_selected {
            Style::default().fg(Color::Black).bg(SELECTED_BG)
        } else if !row.enabled {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::Green)
        };

        // Key
        let key_display = truncate_or_pad(&row.key, key_width as usize);
        let key_style = if is_editing && panel.editing_field == KvField::Key {
            Style::default().fg(Color::Yellow).bg(Color::DarkGray)
        } else if is_selected {
            Style::default().fg(Color::Black).bg(SELECTED_BG)
        } else if !row.enabled {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::Cyan)
        };

        // Separator
        let sep_style = if is_selected {
            Style::default().fg(Color::Black).bg(SELECTED_BG)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        // Value
        let value_display = truncate_or_pad(&row.value, value_width as usize);
        let value_style = if is_editing && panel.editing_field == KvField::Value {
            Style::default().fg(Color::Yellow).bg(Color::DarkGray)
        } else if is_selected {
            Style::default().fg(Color::Black).bg(SELECTED_BG)
        } else if !row.enabled {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::White)
        };

        lines.push(Line::from(vec![
            Span::styled(format!("{} ", toggle), toggle_style),
            Span::styled(key_display, key_style),
            Span::styled(" = ", sep_style),
            Span::styled(value_display, value_style),
        ]));

        // Position cursor when editing
        if is_editing {
            let cursor_x = match panel.editing_field {
                KvField::Key => area.x + toggle_width + row.cursor as u16,
                KvField::Value => {
                    area.x + toggle_width + key_width + separator_width + row.cursor as u16
                }
            };
            let cursor_y = area.y + (i - scroll_offset) as u16;
            if cursor_x < area.x + area.width {
                frame.set_cursor_position((cursor_x, cursor_y));
            }
        }
    }

    frame.render_widget(Paragraph::new(lines), area);
}

fn truncate_or_pad(s: &str, width: usize) -> String {
    let w = s.width();
    if w >= width {
        let mut result = String::new();
        let mut current_width = 0;
        for c in s.chars() {
            let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
            if current_width + cw > width {
                break;
            }
            result.push(c);
            current_width += cw;
        }
        // Pad if truncation left us short
        while current_width < width {
            result.push(' ');
            current_width += 1;
        }
        result
    } else {
        format!("{}{}", s, " ".repeat(width - w))
    }
}

// ── Body editor ──────────────────────────────────────────────────────

fn render_body_editor(frame: &mut Frame, area: Rect, panel: &RequestPanel, focused: bool) {
    if area.height == 0 {
        return;
    }

    let line_num_width: u16 = 4; // "123 "
    let text_area = Rect {
        x: area.x + line_num_width,
        width: area.width.saturating_sub(line_num_width),
        ..area
    };
    let num_area = Rect {
        width: line_num_width,
        ..area
    };

    let visible_lines = area.height as usize;
    let scroll_offset = if panel.body_cursor_row >= visible_lines {
        panel.body_cursor_row - visible_lines + 1
    } else {
        0
    };

    // Line numbers
    let mut num_lines: Vec<Line> = Vec::new();
    let mut text_lines: Vec<Line> = Vec::new();

    for i in scroll_offset..panel.body_lines.len().min(scroll_offset + visible_lines) {
        let is_current = i == panel.body_cursor_row && focused;
        let num_style = if is_current {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        num_lines.push(Line::from(Span::styled(
            format!("{:>3} ", i + 1),
            num_style,
        )));

        let line_text = &panel.body_lines[i];
        let text_style = if is_current {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::Gray)
        };
        text_lines.push(Line::from(Span::styled(line_text.clone(), text_style)));
    }

    frame.render_widget(Paragraph::new(num_lines), num_area);
    frame.render_widget(Paragraph::new(text_lines), text_area);

    // Show cursor when editing body
    if focused && panel.editing && panel.focused_section == Section::Body {
        let visible_row = panel.body_cursor_row.saturating_sub(scroll_offset);
        let cursor_x = text_area.x + panel.body_cursor_col as u16;
        let cursor_y = text_area.y + visible_row as u16;
        if cursor_x < text_area.x + text_area.width && cursor_y < text_area.y + text_area.height {
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }
}

// ── Response area ────────────────────────────────────────────────────

fn render_response_area(frame: &mut Frame, area: Rect, panel: &RequestPanel, focused: bool) {
    let border_color = if focused {
        Color::Blue
    } else {
        Color::DarkGray
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(" Response ");

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    // Loading state
    if panel.request_in_flight {
        let spinner = panel.spinner_char();
        let text = format!("{} Sending request...", spinner);
        let widget = Paragraph::new(text)
            .style(Style::default().fg(Color::Yellow))
            .alignment(ratatui::layout::Alignment::Center);
        let centered = Rect {
            y: inner.y + inner.height / 2,
            height: 1,
            ..inner
        };
        frame.render_widget(widget, centered);
        return;
    }

    // Error state
    if let Some(ref error) = panel.error_message {
        let lines = vec![
            Line::from(Span::styled(
                "Error",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(error.clone(), Style::default().fg(Color::Red))),
        ];
        let widget = Paragraph::new(lines);
        frame.render_widget(widget, inner);
        return;
    }

    // No response yet
    let response = match &panel.response {
        Some(r) => r,
        None => {
            let hint = Paragraph::new("Press Ctrl-Enter or <Space>s to send request")
                .style(Style::default().fg(Color::DarkGray))
                .alignment(ratatui::layout::Alignment::Center);
            if inner.height > 1 {
                let centered = Rect {
                    y: inner.y + inner.height / 2,
                    height: 1,
                    ..inner
                };
                frame.render_widget(hint, centered);
            } else {
                frame.render_widget(hint, inner);
            }
            return;
        }
    };

    // Status line (1 line) + tabs (1 line) + content (rest)
    let status_area = Rect { height: 1, ..inner };
    let tabs_area = Rect {
        y: inner.y + 1,
        height: 1,
        ..inner
    };
    let content_area = Rect {
        y: inner.y + 2,
        height: inner.height.saturating_sub(2),
        ..inner
    };

    // Status line
    let status_color = status_color(response.status_code);
    let status_badge = format!(" {} {} ", response.status_code, response.status_text);
    let time_text = format!(" {}ms ", response.elapsed_ms);
    let size_text = format_size(response.size_bytes);

    let status_line = Line::from(vec![
        Span::styled(
            status_badge,
            Style::default()
                .fg(Color::Black)
                .bg(status_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(time_text, Style::default().fg(Color::DarkGray)),
        Span::raw("  "),
        Span::styled(size_text, Style::default().fg(Color::DarkGray)),
    ]);
    frame.render_widget(Paragraph::new(status_line), status_area);

    // Response tabs
    let mut tab_spans: Vec<Span> = Vec::new();
    tab_spans.push(Span::raw(" "));

    let body_style = if response.focused_section == ResponseSection::Body {
        if focused {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::UNDERLINED)
        }
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let headers_style = if response.focused_section == ResponseSection::Headers {
        if focused {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::UNDERLINED)
        }
    } else {
        Style::default().fg(Color::DarkGray)
    };

    tab_spans.push(Span::styled("Body", body_style));
    tab_spans.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
    tab_spans.push(Span::styled(
        format!("Headers ({})", response.headers.len()),
        headers_style,
    ));

    frame.render_widget(Paragraph::new(Line::from(tab_spans)), tabs_area);

    // Content
    if content_area.height > 0 {
        match response.focused_section {
            ResponseSection::Body => {
                render_response_body(frame, content_area, response);
            }
            ResponseSection::Headers => {
                render_response_headers(frame, content_area, response);
            }
        }
    }
}

fn render_response_body(
    frame: &mut Frame,
    area: Rect,
    response: &crate::request_panel::ResponseData,
) {
    let lines: Vec<Line> = response
        .body
        .lines()
        .skip(response.body_scroll)
        .take(area.height as usize)
        .map(|l| {
            Line::from(Span::styled(
                l.to_string(),
                Style::default().fg(Color::White),
            ))
        })
        .collect();

    frame.render_widget(Paragraph::new(lines), area);
}

fn render_response_headers(
    frame: &mut Frame,
    area: Rect,
    response: &crate::request_panel::ResponseData,
) {
    let lines: Vec<Line> = response
        .headers
        .iter()
        .skip(response.headers_scroll)
        .take(area.height as usize)
        .map(|(k, v)| {
            Line::from(vec![
                Span::styled(
                    format!("{}: ", k),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(v.clone(), Style::default().fg(Color::White)),
            ])
        })
        .collect();

    frame.render_widget(Paragraph::new(lines), area);
}

fn format_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn render_notification(frame: &mut Frame, area: Rect, message: &str) {
    let width = (message.len() as u16 + 4).min(area.width.saturating_sub(4));
    let notification_area = Rect {
        x: area.x + area.width.saturating_sub(width) - 1,
        y: area.y + 1,
        width,
        height: 1,
    };

    frame.render_widget(Clear, notification_area);
    let paragraph = Paragraph::new(Line::from(Span::styled(
        format!(" {} ", message),
        Style::default()
            .fg(Color::Black)
            .bg(Color::Green)
            .add_modifier(Modifier::BOLD),
    )));
    frame.render_widget(paragraph, notification_area);
}
