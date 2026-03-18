use ratatui::{
    layout::{Alignment, Constraint, Flex, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table},
    Frame,
};
use unicode_width::UnicodeWidthStr;

use crate::driver::SortDirection;
use crate::table_view::TableView;
use crate::{DatabaseTool, Focus};

const SPINNER: &[char] = &['|', '/', '-', '\\'];
const SELECTED_BG: Color = Color::Gray;

pub fn render(tool: &DatabaseTool, frame: &mut Frame, area: Rect) {
    let sidebar_width = area.width.min(30);
    let [sidebar_area, main_area] =
        Layout::horizontal([Constraint::Length(sidebar_width), Constraint::Fill(1)]).areas(area);

    render_sidebar(tool, frame, sidebar_area);
    render_main(tool, frame, main_area);

    // Overlay: connection form
    if let Some(ref form) = tool.connection_form {
        render_connection_form(form, tool.loading, tool.spinner_frame, frame, area);
    }

    // Overlay: PIN prompt
    if let Some(ref prompt) = tool.pin_prompt {
        render_pin_prompt(prompt, frame, area);
    }
}

// ── Sidebar ─────────────────────────────────────────────────────────

fn render_sidebar(tool: &DatabaseTool, frame: &mut Frame, area: Rect) {
    let focused = tool.focus == Focus::Sidebar;
    let border_style = if focused {
        Style::default().fg(Color::Blue)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let title = if let Some(ref version) = tool.connected_version {
        let short = version
            .split_whitespace()
            .take(2)
            .collect::<Vec<_>>()
            .join(" ");
        format!(" DB ({short}) ")
    } else {
        " Database ".to_string()
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if tool.sidebar_entries.is_empty() {
        let hint = Paragraph::new("Press 'a' to add a connection")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        frame.render_widget(hint, inner);
        return;
    }

    let visible_height = inner.height as usize;
    // Adjust scroll
    let scroll = if tool.sidebar_selected >= tool.sidebar_scroll + visible_height {
        tool.sidebar_selected - visible_height + 1
    } else if tool.sidebar_selected < tool.sidebar_scroll {
        tool.sidebar_selected
    } else {
        tool.sidebar_scroll
    };

    let lines: Vec<Line> = tool
        .sidebar_entries
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible_height)
        .map(|(i, entry)| {
            let selected = i == tool.sidebar_selected && focused;
            let is_active_conn = entry
                .id
                .strip_prefix("conn:")
                .and_then(|s| s.parse::<i64>().ok())
                .map(|id| Some(id) == tool.active_connection_id)
                .unwrap_or(false);
            let is_active_table = entry.id.strip_prefix("table:").map(|t| {
                tool.active_table
                    .as_ref()
                    .map(|(s, n)| format!("{s}.{n}") == t)
                    .unwrap_or(false)
            }).unwrap_or(false);

            if entry.is_header {
                Line::from(Span::styled(
                    format!("  {}", entry.label),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ))
            } else {
                let prefix = if is_active_conn || is_active_table {
                    "> "
                } else {
                    "  "
                };
                let icon = if entry.id.starts_with("conn:") {
                    "  "
                } else {
                    "  "
                };

                let style = if selected {
                    Style::default()
                        .bg(Color::DarkGray)
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD)
                } else if is_active_conn || is_active_table {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default()
                };

                Line::from(Span::styled(
                    format!("{prefix}{icon}{}", entry.label),
                    style,
                ))
            }
        })
        .collect();

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

// ── Main content ────────────────────────────────────────────────────

fn render_main(tool: &DatabaseTool, frame: &mut Frame, area: Rect) {
    if tool.active_table.is_none() {
        render_welcome(tool, frame, area);
        return;
    }

    // Table view with status bar at bottom
    let [table_area, status_area] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);

    render_table_view(&tool.table_view, tool.focus == Focus::TableView, frame, table_area);
    render_table_status(tool, frame, status_area);
}

fn render_welcome(tool: &DatabaseTool, frame: &mut Frame, area: Rect) {
    let block = Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Database Browser",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    if tool.loading {
        let spinner = SPINNER[tool.spinner_frame as usize % SPINNER.len()];
        lines.push(Line::from(Span::styled(
            format!(" {spinner} Connecting..."),
            Style::default().fg(Color::Yellow),
        )));
    } else if let Some(ref err) = tool.error_message {
        lines.push(Line::from(Span::styled(
            format!(" Error: {err}"),
            Style::default().fg(Color::Red),
        )));
    } else if let Some(ref msg) = tool.status_message {
        lines.push(Line::from(Span::styled(
            format!(" {msg}"),
            Style::default().fg(Color::Green),
        )));
    } else if tool.connections.is_empty() {
        lines.push(Line::from(Span::styled(
            "Press 'a' to add your first connection",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "Select a connection and press Enter",
            Style::default().fg(Color::DarkGray),
        )));
    }

    let paragraph = Paragraph::new(lines).alignment(Alignment::Center);
    let [_, centered, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(6),
        Constraint::Fill(1),
    ])
    .areas(inner);
    frame.render_widget(paragraph, centered);
}

// ── Table view rendering ────────────────────────────────────────────

fn render_table_view(tv: &TableView, focused: bool, frame: &mut Frame, area: Rect) {
    let border_style = if focused {
        Style::default().fg(Color::Blue)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default().borders(Borders::ALL).border_style(border_style);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if tv.columns.is_empty() {
        let hint = Paragraph::new("No data")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        frame.render_widget(hint, inner);
        return;
    }

    // Calculate column widths
    let col_widths = calculate_column_widths(tv, inner.width as usize);

    // Ensure selected column is visible, adjusting horizontal scroll if needed
    let available = inner.width as usize;
    let scroll_x = ensure_col_visible(tv.selected_col, tv.scroll_offset_x.get(), &col_widths, available);
    tv.scroll_offset_x.set(scroll_x);
    let visible_cols = visible_column_range(scroll_x, &col_widths, available);

    // Header
    let header_cells: Vec<Cell> = visible_cols
        .clone()
        .map(|i| {
            let col = &tv.columns[i];
            let sort_indicator = if tv.sort_column == Some(i) {
                match tv.sort_direction {
                    SortDirection::Asc => " ▲",
                    SortDirection::Desc => " ▼",
                }
            } else {
                ""
            };
            let selected = i == tv.selected_col && focused;
            let style = if selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else {
                Style::default().add_modifier(Modifier::BOLD)
            };
            Cell::from(format!("{}{sort_indicator}", col.name)).style(style)
        })
        .collect();

    let header = Row::new(header_cells).style(Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED));

    // Data rows
    let mut rows: Vec<Row> = tv
        .rows
        .iter()
        .enumerate()
        .map(|(row_idx, row)| {
            let cells: Vec<Cell> = visible_cols
                .clone()
                .map(|col_idx| {
                    let val = row.get(col_idx).map(|s| s.as_str()).unwrap_or("");
                    // Truncate long values (find a char-safe boundary)
                    let display = if val.len() > 50 {
                        let end = val
                            .char_indices()
                            .map(|(i, _)| i)
                            .take_while(|&i| i <= 47)
                            .last()
                            .unwrap_or(0);
                        format!("{}...", &val[..end])
                    } else {
                        val.to_string()
                    };
                    Cell::from(display)
                })
                .collect();

            let style = if row_idx == tv.selected_row && focused {
                Style::default().bg(SELECTED_BG).fg(Color::Black).add_modifier(Modifier::BOLD)
            } else if row_idx % 2 == 0 {
                Style::default()
            } else {
                Style::default().add_modifier(Modifier::DIM)
            };

            Row::new(cells).style(style)
        })
        .collect();

    // "Load more" virtual row
    if tv.has_more() {
        let remaining = tv.total_count - tv.loaded_count as i64;
        let load_more_idx = tv.rows.len();
        let selected = load_more_idx == tv.selected_row && focused;
        let style = if selected {
            Style::default()
                .bg(SELECTED_BG)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::DIM)
        };
        let label = format!("  ↓ Load more ({remaining} remaining) — press Enter");
        let cells: Vec<Cell> = visible_cols
            .clone()
            .enumerate()
            .map(|(i, _)| {
                if i == 0 {
                    Cell::from(label.clone()).style(style)
                } else {
                    Cell::from("")
                }
            })
            .collect();
        rows.push(Row::new(cells));
    }

    let widths: Vec<Constraint> = visible_cols
        .clone()
        .map(|i| Constraint::Length(col_widths[i] as u16))
        .collect();

    let table = Table::new(rows, widths).header(header);
    frame.render_widget(table, inner);

    // Filter input overlay at bottom
    if let Some(filter_text) = tv.filter_text() {
        let filter_area = Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(1),
            width: inner.width,
            height: 1,
        };
        let line = Line::from(vec![
            Span::styled("/", Style::default().fg(Color::Yellow)),
            Span::raw(filter_text),
        ]);
        frame.render_widget(Clear, filter_area);
        frame.render_widget(Paragraph::new(line), filter_area);
    }
}

fn render_table_status(tool: &DatabaseTool, frame: &mut Frame, area: Rect) {
    let tv = &tool.table_view;
    let page_info = format!(
        " {}/{} rows loaded",
        tv.loaded_count,
        tv.total_count
    );

    let filter_info = if tv.filters.is_empty() {
        String::new()
    } else {
        format!(" · {} filter(s)", tv.filters.len())
    };

    let loading = if tool.loading {
        let spinner = SPINNER[tool.spinner_frame as usize % SPINNER.len()];
        format!(" {spinner} ")
    } else {
        String::new()
    };

    let line = Line::from(vec![
        Span::styled(
            format!("{page_info}{filter_info}"),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(loading, Style::default().fg(Color::Yellow)),
    ]);

    frame.render_widget(Paragraph::new(line), area);
}

fn calculate_column_widths(tv: &TableView, _available_width: usize) -> Vec<usize> {
    let num_cols = tv.columns.len();
    if num_cols == 0 {
        return Vec::new();
    }

    // Start with header widths + 2 for padding
    let mut widths: Vec<usize> = tv
        .columns
        .iter()
        .map(|c| c.name.width() + 3) // +3 for sort indicator space + padding
        .collect();

    // Adjust based on data content (sample first few rows)
    for row in tv.rows.iter().take(20) {
        for (i, val) in row.iter().enumerate() {
            if i < widths.len() {
                let w = val.width().min(50) + 2;
                if w > widths[i] {
                    widths[i] = w;
                }
            }
        }
    }

    // Cap individual columns
    for w in &mut widths {
        *w = (*w).min(40);
    }

    widths
}

/// Adjust scroll offset so that `selected` is within the visible column range.
fn ensure_col_visible(
    selected: usize,
    current_scroll: usize,
    col_widths: &[usize],
    available_width: usize,
) -> usize {
    if col_widths.is_empty() {
        return 0;
    }
    let visible = visible_column_range(current_scroll, col_widths, available_width);
    if visible.contains(&selected) {
        return current_scroll;
    }
    if selected < current_scroll {
        return selected;
    }
    // Selected column is to the right — walk backwards to find the start
    // that makes it the rightmost visible column.
    let mut total = 0;
    let mut start = selected;
    for i in (0..=selected).rev() {
        total += col_widths[i];
        if total > available_width {
            start = i + 1;
            break;
        }
        start = i;
    }
    start
}

fn visible_column_range(
    scroll_x: usize,
    col_widths: &[usize],
    available_width: usize,
) -> std::ops::Range<usize> {
    let start = scroll_x.min(col_widths.len().saturating_sub(1));
    let mut total = 0;
    let mut end = start;
    for i in start..col_widths.len() {
        total += col_widths[i];
        if total > available_width && end > start {
            break;
        }
        end = i + 1;
    }
    start..end
}

// ── Connection form overlay ─────────────────────────────────────────

fn render_connection_form(
    form: &crate::connection_form::ConnectionForm,
    loading: bool,
    spinner_frame: u8,
    frame: &mut Frame,
    area: Rect,
) {
    let popup_width = (area.width * 60 / 100).max(50).min(70);
    let popup_height = (area.height * 80 / 100).max(20).min(30);

    let popup_area = centered_rect(popup_width, popup_height, area);
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .title(" New Connection ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let visible_fields = form.visible_fields();
    let mut lines: Vec<Line> = Vec::new();

    for (i, field) in visible_fields.iter().enumerate() {
        let is_focused = i == form.focused_field;
        let label = field.label();
        let value = form.display_value(*field);

        let label_style = if is_focused {
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let value_style = if is_focused {
            Style::default().fg(Color::White)
        } else {
            Style::default()
        };

        if field.is_button() {
            let btn_style = if is_focused {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Blue)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Blue)
            };
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(value, btn_style),
            ]));
        } else if field.is_toggle() {
            lines.push(Line::from(vec![
                Span::styled(format!("  {label:<16}"), label_style),
                Span::styled(value, value_style),
            ]));
        } else {
            let cursor = if is_focused { "_" } else { "" };
            lines.push(Line::from(vec![
                Span::styled(format!("  {label:<16}"), label_style),
                Span::styled(format!("{value}{cursor}"), value_style),
            ]));
        }
    }

    // Test result
    if let Some(ref result) = form.test_result {
        lines.push(Line::from(""));
        match result {
            Ok(version) => {
                lines.push(Line::from(Span::styled(
                    format!("  OK: {version}"),
                    Style::default().fg(Color::Green),
                )));
            }
            Err(msg) => {
                lines.push(Line::from(Span::styled(
                    format!("  Error: {msg}"),
                    Style::default().fg(Color::Red),
                )));
            }
        }
    }

    if loading {
        let spinner = SPINNER[spinner_frame as usize % SPINNER.len()];
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  {spinner} Testing..."),
            Style::default().fg(Color::Yellow),
        )));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

// ── PIN prompt overlay ──────────────────────────────────────────────

fn render_pin_prompt(prompt: &crate::PinPrompt, frame: &mut Frame, area: Rect) {
    let popup_area = centered_rect(40, 7, area);
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .title(" Enter PIN ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let dots: String = "*".repeat(prompt.buffer.len());
    let remaining = 4 - prompt.buffer.len();
    let placeholder = "_".repeat(remaining);

    let mut lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("  PIN: "),
            Span::styled(&dots, Style::default().fg(Color::Yellow)),
            Span::styled(placeholder, Style::default().fg(Color::DarkGray)),
        ]),
    ];

    if let Some(ref error) = prompt.error {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  {error}"),
            Style::default().fg(Color::Red),
        )));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

// ── Helpers ─────────────────────────────────────────────────────────

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([Constraint::Length(height)]).flex(Flex::Center);
    let horizontal = Layout::horizontal([Constraint::Length(width)]).flex(Flex::Center);
    let [area] = vertical.areas(area);
    let [area] = horizontal.areas(area);
    area
}
