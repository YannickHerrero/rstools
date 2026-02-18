use crate::sidebar::{render_tree_sidebar, SidebarState, TreeSidebarRenderConfig};
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
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
