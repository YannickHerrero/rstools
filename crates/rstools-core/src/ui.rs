use crate::keybinds::InputMode;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Tabs},
};

/// Render the top tab bar showing open tools.
/// `tools` is a list of tool names, `active` is the index of the active tool.
pub fn render_tab_bar(frame: &mut Frame, area: Rect, tools: &[&str], active: usize) {
    let titles: Vec<Line> = tools.iter().map(|t| Line::from(*t)).collect();

    let tabs = Tabs::new(titles)
        .select(active)
        .highlight_style(Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED))
        .style(Style::default().add_modifier(Modifier::DIM))
        .divider(Span::raw(" | "));

    frame.render_widget(tabs, area);
}

/// Render the bottom status bar showing the current mode and optional info.
pub fn render_status_bar(
    frame: &mut Frame,
    area: Rect,
    mode: InputMode,
    tool_name: &str,
    info: &str,
) {
    let mode_style = match mode {
        InputMode::Normal => Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED),
        InputMode::Insert => Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED),
        InputMode::Command => Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED),
    };

    let line = Line::from(vec![
        Span::styled(format!(" {} ", mode.label()), mode_style),
        Span::raw(" "),
        Span::styled(tool_name, Style::default().add_modifier(Modifier::BOLD)),
        Span::raw("  "),
        Span::styled(info, Style::default().add_modifier(Modifier::DIM)),
    ]);

    let bar = Paragraph::new(line).style(Style::default().add_modifier(Modifier::REVERSED));
    frame.render_widget(bar, area);
}

/// Render the command-line input at the bottom of the screen.
pub fn render_command_line(frame: &mut Frame, area: Rect, input: &str, cursor: usize) {
    let line = Line::from(vec![
        Span::styled(":", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(input),
    ]);

    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, area);

    // Position cursor
    frame.set_cursor_position((area.x + 1 + cursor as u16, area.y));
}

/// Standard layout: tab bar (1 line) + main content + status bar (1 line).
/// Returns (tab_area, content_area, status_area).
pub fn standard_layout(area: Rect) -> (Rect, Rect, Rect) {
    let [tab_area, content_area, status_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(area);

    (tab_area, content_area, status_area)
}

/// Create a standard bordered block for a tool view.
pub fn tool_block(title: &str) -> Block<'_> {
    Block::default()
        .title(format!(" {} ", title))
        .borders(Borders::ALL)
        .border_style(Style::default().add_modifier(Modifier::DIM))
}
