use crate::detail::DetailPanel;
use crate::sidebar::SidebarState;
use crate::vault::{FlatNode, NodeType, VaultState};
use crate::{InputPrompt, KeePassTool, ToolFocus};
use ratatui::{
    layout::{Alignment, Constraint, Flex, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

/// Maximum sidebar width in characters.
const MAX_SIDEBAR_WIDTH: u16 = 40;

/// Minimum sidebar width in characters (enough for the empty-state help text).
const MIN_SIDEBAR_WIDTH: u16 = 22;

/// Compute the sidebar width based on the longest file name, capped at [`MAX_SIDEBAR_WIDTH`].
/// Adds 4 chars of padding (2 for border, 2 for inner margin).
pub fn sidebar_width(sidebar: &SidebarState) -> u16 {
    if sidebar.files.is_empty() {
        return MIN_SIDEBAR_WIDTH;
    }
    let longest = sidebar
        .files
        .iter()
        .map(|f| f.display_name.len() as u16)
        .max()
        .unwrap_or(0);
    // +4: 2 for block borders, 2 for inner padding (" name")
    (longest + 4).clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH)
}

// ── Colors ───────────────────────────────────────────────────────────

const COLOR_GROUP: Color = Color::Blue;
const COLOR_ENTRY: Color = Color::Green;
const COLOR_LABEL: Color = Color::Cyan;
const COLOR_MASKED: Color = Color::DarkGray;
const COLOR_TAG: Color = Color::Magenta;

// ── Main entry point ─────────────────────────────────────────────────

/// Render the entire KeePass tool view.
pub fn render_keepass_tool(frame: &mut Frame, area: Rect, tool: &KeePassTool) {
    // Check for input prompts first (rendered as overlays later)
    let base_area = area;

    if tool.sidebar.visible {
        let sidebar_width = sidebar_width(&tool.sidebar).min(area.width.saturating_sub(20));
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
            &tool.sidebar,
            tool.focus == ToolFocus::Sidebar,
        );
        render_content_area(frame, content_area, tool);
    } else {
        render_content_area(frame, area, tool);
    }

    // Render overlays (popups)
    render_input_prompt(frame, base_area, tool);
    render_search_overlay(frame, base_area, tool);

    // Render clipboard notification
    if let Some(ref msg) = tool.clipboard_notification {
        render_notification(frame, base_area, msg);
    }
}

// ── Sidebar ──────────────────────────────────────────────────────────

fn render_sidebar(frame: &mut Frame, area: Rect, sidebar: &SidebarState, focused: bool) {
    let border_color = if focused {
        Color::Blue
    } else {
        Color::DarkGray
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(" KeePass Files ");

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    if sidebar.files.is_empty() {
        let help = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "No files opened yet",
                Style::default().add_modifier(Modifier::DIM),
            )),
            Line::from(""),
            Line::from(Span::styled(
                ":open <path>  or",
                Style::default().add_modifier(Modifier::DIM),
            )),
            Line::from(Span::styled(
                "<Space>ko to browse",
                Style::default().add_modifier(Modifier::DIM),
            )),
        ])
        .alignment(Alignment::Center);
        frame.render_widget(help, inner);
        return;
    }

    // Scrolling
    let visible_lines = inner.height as usize;
    let scroll_offset = if sidebar.selected >= visible_lines {
        sidebar.selected - visible_lines + 1
    } else {
        0
    };

    let lines: Vec<Line> = sidebar
        .files
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(visible_lines)
        .map(|(i, file)| {
            let is_selected = i == sidebar.selected;
            let bg = if is_selected {
                Color::DarkGray
            } else {
                Color::Reset
            };

            // File name with left padding
            let name_style = if is_selected {
                Style::default()
                    .fg(Color::White)
                    .bg(bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White).bg(bg)
            };
            let spans = vec![
                Span::styled(" ", Style::default().bg(bg)),
                Span::styled(&file.display_name, name_style),
            ];

            Line::from(spans)
        })
        .collect();

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);

    // Render confirm delete prompt if active
    if sidebar.confirm_delete {
        let prompt_area = Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(1),
            width: inner.width,
            height: 1,
        };
        let prompt = Paragraph::new(Line::from(vec![
            Span::styled(
                "Delete from history? ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled("(y/n)", Style::default().add_modifier(Modifier::DIM)),
        ]));
        frame.render_widget(prompt, prompt_area);
    }
}

// ── Content area ─────────────────────────────────────────────────────

fn render_content_area(frame: &mut Frame, area: Rect, tool: &KeePassTool) {
    if tool.locked {
        render_lock_screen(frame, area, tool);
        return;
    }

    match &tool.vault {
        Some(vault) => {
            // Split content into tree (left) and detail (right)
            let tree_width = (area.width * 40 / 100)
                .max(20)
                .min(area.width.saturating_sub(30));
            let tree_area = Rect {
                x: area.x,
                y: area.y,
                width: tree_width,
                height: area.height,
            };
            let detail_area = Rect {
                x: area.x + tree_width,
                y: area.y,
                width: area.width.saturating_sub(tree_width),
                height: area.height,
            };

            render_vault_tree(frame, tree_area, vault, tool.focus == ToolFocus::Tree);
            render_detail_panel(
                frame,
                detail_area,
                &tool.detail,
                tool.focus == ToolFocus::Detail,
            );
        }
        None => {
            render_empty_content(frame, area);
        }
    }
}

fn render_empty_content(frame: &mut Frame, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" KeePass ");

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let help = Paragraph::new(vec![
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            "No vault open",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Open a .kdbx file:",
            Style::default().add_modifier(Modifier::DIM),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  :open ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled("<path>", Style::default().add_modifier(Modifier::DIM)),
        ]),
        Line::from(vec![
            Span::styled(
                "  <Space>ko ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled("file picker", Style::default().add_modifier(Modifier::DIM)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Or select a file from the sidebar",
            Style::default().add_modifier(Modifier::DIM),
        )),
    ])
    .alignment(Alignment::Center);
    frame.render_widget(help, inner);
}

fn render_lock_screen(frame: &mut Frame, area: Rect, tool: &KeePassTool) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" Locked ");

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let vault_name = tool
        .vault
        .as_ref()
        .map(|v| v.vault_name.as_str())
        .unwrap_or("Vault");

    let lines = vec![
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            "  Locked  ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            vault_name,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Press Enter to unlock",
            Style::default().add_modifier(Modifier::DIM),
        )),
    ];

    let paragraph = Paragraph::new(lines).alignment(Alignment::Center);
    frame.render_widget(paragraph, inner);
}

// ── Vault tree ───────────────────────────────────────────────────────

fn render_vault_tree(frame: &mut Frame, area: Rect, vault: &VaultState, focused: bool) {
    let border_color = if focused {
        Color::Blue
    } else {
        Color::DarkGray
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(format!(" {} ", vault.vault_name));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    if vault.flat_view.is_empty() {
        let empty = Paragraph::new(Span::styled(
            "Empty vault",
            Style::default().add_modifier(Modifier::DIM),
        ))
        .alignment(Alignment::Center);
        frame.render_widget(empty, inner);
        return;
    }

    let visible_lines = inner.height as usize;
    let scroll_offset = if vault.selected >= visible_lines {
        vault.selected - visible_lines + 1
    } else {
        0
    };

    let lines: Vec<Line> = vault
        .flat_view
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(visible_lines)
        .map(|(i, node)| render_tree_line(node, i == vault.selected, inner.width as usize))
        .collect();

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

fn render_tree_line(node: &FlatNode, is_selected: bool, _max_width: usize) -> Line<'static> {
    let bg = if is_selected {
        Color::DarkGray
    } else {
        Color::Reset
    };

    let mut spans = Vec::new();

    // Indent with guide lines
    for (d, &has_guide) in node.guide_depths.iter().enumerate() {
        if d < node.depth {
            if has_guide {
                spans.push(Span::styled(
                    " \u{2502} ",
                    Style::default().fg(Color::DarkGray).bg(bg),
                ));
            } else {
                spans.push(Span::styled("   ", Style::default().bg(bg)));
            }
        }
    }

    // Fill remaining indent if guide_depths is shorter than depth
    let guides_shown = node.guide_depths.len().min(node.depth);
    for _ in guides_shown..node.depth {
        spans.push(Span::styled("   ", Style::default().bg(bg)));
    }

    // Icon
    match node.node_type {
        NodeType::Group => {
            let icon = if node.is_expanded {
                "\u{25BC} "
            } else {
                "\u{25B6} "
            };
            spans.push(Span::styled(icon, Style::default().fg(COLOR_GROUP).bg(bg)));
        }
        NodeType::Entry => {
            spans.push(Span::styled(
                "\u{25CF} ",
                Style::default().fg(COLOR_ENTRY).bg(bg),
            ));
        }
    }

    // Name
    let name_style = if is_selected {
        Style::default()
            .fg(Color::White)
            .bg(bg)
            .add_modifier(Modifier::BOLD)
    } else {
        let fg = match node.node_type {
            NodeType::Group => COLOR_GROUP,
            NodeType::Entry => Color::White,
        };
        Style::default().fg(fg).bg(bg)
    };
    spans.push(Span::styled(node.name.clone(), name_style));

    Line::from(spans)
}

// ── Detail panel ─────────────────────────────────────────────────────

fn render_detail_panel(frame: &mut Frame, area: Rect, detail: &DetailPanel, focused: bool) {
    let border_color = if focused {
        Color::Blue
    } else {
        Color::DarkGray
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(" Details ");

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let details = match &detail.details {
        Some(d) => d,
        None => {
            let empty = Paragraph::new(Span::styled(
                "Select an entry to view details",
                Style::default().add_modifier(Modifier::DIM),
            ))
            .alignment(Alignment::Center);
            frame.render_widget(empty, inner);
            return;
        }
    };

    let mut lines: Vec<Line> = Vec::new();

    // Title
    lines.push(Line::from(vec![
        Span::styled(
            "Title    ",
            Style::default()
                .fg(COLOR_LABEL)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(&details.title, Style::default().fg(Color::White)),
    ]));
    lines.push(Line::from(""));

    // Username
    lines.push(Line::from(vec![
        Span::styled(
            "Username ",
            Style::default()
                .fg(COLOR_LABEL)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(&details.username, Style::default().fg(Color::White)),
    ]));
    lines.push(Line::from(""));

    // Password
    let password_display = if detail.password_visible {
        details.password.clone()
    } else {
        "\u{2022}".repeat(details.password.len().min(20))
    };
    lines.push(Line::from(vec![
        Span::styled(
            "Password ",
            Style::default()
                .fg(COLOR_LABEL)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            password_display,
            Style::default().fg(if detail.password_visible {
                Color::White
            } else {
                COLOR_MASKED
            }),
        ),
        Span::styled(
            if detail.password_visible {
                "  [p: hide]"
            } else {
                "  [p: show]"
            },
            Style::default().add_modifier(Modifier::DIM),
        ),
    ]));
    lines.push(Line::from(""));

    // URL
    if !details.url.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(
                "URL      ",
                Style::default()
                    .fg(COLOR_LABEL)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(&details.url, Style::default().fg(Color::Cyan)),
        ]));
        lines.push(Line::from(""));
    }

    // Tags
    if !details.tags.is_empty() {
        let mut tag_spans = vec![Span::styled(
            "Tags     ",
            Style::default()
                .fg(COLOR_LABEL)
                .add_modifier(Modifier::BOLD),
        )];
        for (i, tag) in details.tags.iter().enumerate() {
            if i > 0 {
                tag_spans.push(Span::styled(", ", Style::default().fg(Color::DarkGray)));
            }
            tag_spans.push(Span::styled(tag.as_str(), Style::default().fg(COLOR_TAG)));
        }
        lines.push(Line::from(tag_spans));
        lines.push(Line::from(""));
    }

    // Custom fields
    if !details.custom_fields.is_empty() {
        lines.push(Line::from(Span::styled(
            "--- Custom Fields ---",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
        )));
        lines.push(Line::from(""));

        for (idx, (key, value, is_protected)) in details.custom_fields.iter().enumerate() {
            let display_value =
                if *is_protected && !detail.revealed_custom.get(idx).copied().unwrap_or(false) {
                    "\u{2022}".repeat(value.len().min(20))
                } else {
                    value.clone()
                };

            let label = format!("{:<9}", key);
            lines.push(Line::from(vec![
                Span::styled(
                    label,
                    Style::default()
                        .fg(COLOR_LABEL)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    display_value,
                    Style::default().fg(if *is_protected {
                        COLOR_MASKED
                    } else {
                        Color::White
                    }),
                ),
            ]));
        }
        lines.push(Line::from(""));
    }

    // Notes
    if !details.notes.is_empty() {
        lines.push(Line::from(Span::styled(
            "--- Notes ---",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
        )));
        lines.push(Line::from(""));
        for line in details.notes.lines() {
            lines.push(Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(Color::White),
            )));
        }
    }

    // Keybind hints at bottom
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(" yu", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(" user  ", Style::default().add_modifier(Modifier::DIM)),
        Span::styled("yp", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(" pass  ", Style::default().add_modifier(Modifier::DIM)),
        Span::styled("yU", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(" URL  ", Style::default().add_modifier(Modifier::DIM)),
        Span::styled("p", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(" toggle pass", Style::default().add_modifier(Modifier::DIM)),
    ]));

    // Apply scroll
    let scroll = detail.scroll;
    let visible: Vec<Line> = lines.into_iter().skip(scroll).collect();

    let paragraph = Paragraph::new(visible).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

// ── Input prompts (overlays) ─────────────────────────────────────────

fn render_input_prompt(frame: &mut Frame, area: Rect, tool: &KeePassTool) {
    let prompt = match &tool.input_prompt {
        Some(p) => p,
        None => return,
    };

    let popup_width = 50u16.min(area.width.saturating_sub(4));
    let popup_height = 5u16;

    let vertical = Layout::vertical([Constraint::Length(popup_height)]).flex(Flex::Center);
    let horizontal = Layout::horizontal([Constraint::Length(popup_width)]).flex(Flex::Center);
    let [popup_area] = vertical.areas(area);
    let [popup_area] = horizontal.areas(popup_area);

    frame.render_widget(Clear, popup_area);

    let title = match prompt {
        InputPrompt::MasterPassword { .. } => " Master Password ",
        InputPrompt::PinInput { .. } => " Enter PIN ",
        InputPrompt::PinSetup { .. } => " Set Up PIN? ",
        InputPrompt::PinCreate { .. } => " Create PIN ",
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(title);

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    match prompt {
        InputPrompt::MasterPassword { buffer, error, .. } => {
            let mut lines = Vec::new();

            if let Some(err) = error {
                lines.push(Line::from(Span::styled(
                    err.as_str(),
                    Style::default().fg(Color::Red),
                )));
            }

            // Show dots for each character
            let masked: String = "\u{2022}".repeat(buffer.len());
            lines.push(Line::from(vec![
                Span::styled("> ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(masked),
            ]));

            let paragraph = Paragraph::new(lines);
            frame.render_widget(paragraph, inner);

            // Place cursor
            let cursor_x = inner.x + 2 + buffer.len() as u16;
            let cursor_y = if error.is_some() {
                inner.y + 1
            } else {
                inner.y
            };
            frame.set_cursor_position((cursor_x, cursor_y));
        }
        InputPrompt::PinInput { buffer, error, .. } => {
            let mut lines = Vec::new();

            if let Some(err) = error {
                lines.push(Line::from(Span::styled(
                    err.as_str(),
                    Style::default().fg(Color::Red),
                )));
            }

            // Show dots and remaining blanks for 4-digit PIN
            let mut pin_display = String::new();
            for i in 0..4 {
                if i < buffer.len() {
                    pin_display.push('\u{2022}');
                } else {
                    pin_display.push('_');
                }
                if i < 3 {
                    pin_display.push(' ');
                }
            }
            lines.push(Line::from(vec![
                Span::styled("PIN: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(pin_display, Style::default().fg(Color::Yellow)),
            ]));

            let paragraph = Paragraph::new(lines);
            frame.render_widget(paragraph, inner);
        }
        InputPrompt::PinSetup { .. } => {
            let lines = vec![
                Line::from(Span::raw("Set up a 4-digit PIN for quick access?")),
                Line::from(""),
                Line::from(vec![
                    Span::styled("y", Style::default().add_modifier(Modifier::BOLD)),
                    Span::styled(" Yes  ", Style::default().add_modifier(Modifier::DIM)),
                    Span::styled("n", Style::default().add_modifier(Modifier::BOLD)),
                    Span::styled(" No", Style::default().add_modifier(Modifier::DIM)),
                ]),
            ];
            let paragraph = Paragraph::new(lines);
            frame.render_widget(paragraph, inner);
        }
        InputPrompt::PinCreate { buffer, .. } => {
            let mut pin_display = String::new();
            for i in 0..4 {
                if i < buffer.len() {
                    pin_display.push('\u{2022}');
                } else {
                    pin_display.push('_');
                }
                if i < 3 {
                    pin_display.push(' ');
                }
            }
            let lines = vec![
                Line::from(Span::raw("Enter a 4-digit PIN:")),
                Line::from(""),
                Line::from(vec![
                    Span::styled("PIN: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::styled(pin_display, Style::default().fg(Color::Yellow)),
                ]),
            ];
            let paragraph = Paragraph::new(lines);
            frame.render_widget(paragraph, inner);
        }
    }
}

// ── Search overlay (telescope with preview) ──────────────────────────

fn render_search_overlay(frame: &mut Frame, area: Rect, tool: &KeePassTool) {
    if !tool.search_active {
        return;
    }

    // Size: 70% width, 60% height, centered
    let popup_width = (area.width * 70 / 100)
        .max(50)
        .min(area.width.saturating_sub(4));
    let popup_height = (area.height * 60 / 100)
        .max(10)
        .min(area.height.saturating_sub(4));

    let vertical = Layout::vertical([Constraint::Length(popup_height)]).flex(Flex::Center);
    let horizontal = Layout::horizontal([Constraint::Length(popup_width)]).flex(Flex::Center);
    let [popup_area] = vertical.areas(area);
    let [popup_area] = horizontal.areas(popup_area);

    frame.render_widget(Clear, popup_area);

    // Split into: search input (3 lines) + body
    let [input_area, body_area] =
        Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).areas(popup_area);

    // Search input
    let input_block = Block::default()
        .title(" Search Entries ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let input_text = Paragraph::new(Line::from(vec![
        Span::styled("> ", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(&tool.search_query),
    ]))
    .block(input_block);
    frame.render_widget(input_text, input_area);

    // Place cursor
    frame.set_cursor_position((
        input_area.x + 2 + tool.search_query.len() as u16 + 1,
        input_area.y + 1,
    ));

    // Split body into results (left) and preview (right)
    let results_width = body_area.width / 2;
    let results_area = Rect {
        x: body_area.x,
        y: body_area.y,
        width: results_width,
        height: body_area.height,
    };
    let preview_area = Rect {
        x: body_area.x + results_width,
        y: body_area.y,
        width: body_area.width.saturating_sub(results_width),
        height: body_area.height,
    };

    // Results list
    let results_block = Block::default()
        .borders(Borders::LEFT | Borders::BOTTOM)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" Results ");

    let results_inner = results_block.inner(results_area);
    frame.render_widget(results_block, results_area);

    let visible_lines = results_inner.height as usize;
    let scroll = if tool.search_selected >= visible_lines {
        tool.search_selected - visible_lines + 1
    } else {
        0
    };

    let result_lines: Vec<Line> = tool
        .search_results
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible_lines)
        .map(|(i, entry)| {
            let is_selected = i == tool.search_selected;
            let bg = if is_selected {
                Color::DarkGray
            } else {
                Color::Reset
            };
            let prefix = if is_selected { "> " } else { "  " };
            Line::from(vec![
                Span::styled(prefix, Style::default().fg(Color::Yellow).bg(bg)),
                Span::styled(
                    entry.title.clone(),
                    Style::default()
                        .fg(Color::White)
                        .bg(bg)
                        .add_modifier(if is_selected {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
            ])
        })
        .collect();

    let results_paragraph = Paragraph::new(result_lines);
    frame.render_widget(results_paragraph, results_inner);

    // Preview panel
    let preview_block = Block::default()
        .borders(Borders::RIGHT | Borders::BOTTOM)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" Preview ");

    let preview_inner = preview_block.inner(preview_area);
    frame.render_widget(preview_block, preview_area);

    if let Some(entry) = tool.search_results.get(tool.search_selected) {
        // Show preview of the selected search result
        let vault = match &tool.vault {
            Some(v) => v,
            None => return,
        };

        if let Some(node) = vault.node_at_path(&entry.tree_path) {
            if let Some(ref details) = node.details {
                let mut preview_lines = Vec::new();
                preview_lines.push(Line::from(vec![
                    Span::styled(
                        "Title: ",
                        Style::default()
                            .fg(COLOR_LABEL)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(&details.title, Style::default().fg(Color::White)),
                ]));
                if !entry.group_path.is_empty() {
                    preview_lines.push(Line::from(vec![
                        Span::styled(
                            "Group: ",
                            Style::default()
                                .fg(COLOR_LABEL)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(&entry.group_path, Style::default().fg(Color::DarkGray)),
                    ]));
                }
                preview_lines.push(Line::from(vec![
                    Span::styled(
                        "User:  ",
                        Style::default()
                            .fg(COLOR_LABEL)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(&details.username, Style::default().fg(Color::White)),
                ]));
                if !details.url.is_empty() {
                    preview_lines.push(Line::from(vec![
                        Span::styled(
                            "URL:   ",
                            Style::default()
                                .fg(COLOR_LABEL)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(&details.url, Style::default().fg(Color::Cyan)),
                    ]));
                }
                if !details.tags.is_empty() {
                    preview_lines.push(Line::from(vec![
                        Span::styled(
                            "Tags:  ",
                            Style::default()
                                .fg(COLOR_LABEL)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(details.tags.join(", "), Style::default().fg(COLOR_TAG)),
                    ]));
                }

                let preview = Paragraph::new(preview_lines).wrap(Wrap { trim: false });
                frame.render_widget(preview, preview_inner);
            }
        }
    }
}

// ── Notification ─────────────────────────────────────────────────────

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
