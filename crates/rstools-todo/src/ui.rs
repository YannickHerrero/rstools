use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::model::Todo;

/// Render the todo list.
pub fn render_todo_list(
    frame: &mut Frame,
    area: Rect,
    todos: &[Todo],
    list_state: &mut ListState,
    filter: Option<&str>,
) {
    let [list_area, info_area] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);

    // Build list items
    let items: Vec<ListItem> = todos
        .iter()
        .map(|todo| {
            let checkbox = if todo.completed { "[x]" } else { "[ ]" };
            let style = if todo.completed {
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::CROSSED_OUT)
            } else {
                Style::default().fg(Color::White)
            };

            let mut spans = vec![
                Span::styled(
                    format!("{} ", checkbox),
                    if todo.completed {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    },
                ),
                Span::styled(&todo.title, style),
            ];

            if let Some(desc) = &todo.description {
                if !desc.is_empty() {
                    spans.push(Span::styled(
                        format!("  {}", desc),
                        Style::default().fg(Color::DarkGray),
                    ));
                }
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    let title = if let Some(f) = filter {
        format!(" Todo [/{}] ", f)
    } else {
        " Todo ".to_string()
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(40, 40, 60))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    frame.render_stateful_widget(list, list_area, list_state);

    // Info bar
    let count_done = todos.iter().filter(|t| t.completed).count();
    let info = Paragraph::new(Line::from(vec![
        Span::styled(
            format!(" {} items", todos.len()),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            format!("  {} done", count_done),
            Style::default().fg(Color::Green),
        ),
        Span::styled(
            "  a:add  e:edit  dd:del  Enter:toggle  /:filter",
            Style::default().fg(Color::DarkGray),
        ),
    ]));
    frame.render_widget(info, info_area);
}

/// Render the input line for adding/editing a todo.
pub fn render_todo_input(frame: &mut Frame, area: Rect, prompt: &str, input: &str, cursor: usize) {
    let block = Block::default()
        .title(format!(" {} ", prompt))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let text = Paragraph::new(input);
    frame.render_widget(text, inner);

    // Position cursor
    frame.set_cursor_position((inner.x + cursor as u16, inner.y));
}
