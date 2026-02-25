use ratatui::{
    layout::{Alignment, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::app::App;
use crate::events::{Modal, TaskFormField};

use super::help::centered_rect;

const PRIORITIES: &[&str] = &["Low", "Medium", "High", "Urgent"];

/// Renders the task creation form overlay.
pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;

    let Some(Modal::TaskCreate {
        ref title,
        cursor_pos,
        epic_index,
        priority_index,
        ref focused_field,
        ref error,
    }) = app.modal
    else {
        return;
    };

    let popup = centered_rect(60, 14, area);
    f.render_widget(Clear, popup);

    // Build epic options: "(none)", then each epic in sorted order
    let mut epic_options: Vec<String> = vec!["(none)".to_string()];
    for (id, epic) in &app.state.epics {
        epic_options.push(format!("{} — {}", id, epic.title));
    }

    let epic_label = epic_options
        .get(epic_index)
        .map(|s| s.as_str())
        .unwrap_or("(none)");

    let priority_label = PRIORITIES
        .get(priority_index)
        .copied()
        .unwrap_or("Medium");

    let title_focused = *focused_field == TaskFormField::Title;
    let epic_focused = *focused_field == TaskFormField::Epic;
    let priority_focused = *focused_field == TaskFormField::Priority;

    let focus_style = theme.focus_style();
    let normal_label_style = theme.unfocused_label_style();

    // Title field
    let title_label_style = if title_focused { focus_style } else { normal_label_style };
    let title_value = if title_focused {
        // Show cursor by inserting a block character at cursor_pos
        let mut display = title.clone();
        if cursor_pos <= display.len() {
            display.insert(cursor_pos, '█');
        }
        display
    } else if title.is_empty() {
        "…".to_string()
    } else {
        title.clone()
    };
    let title_value_style = if title_focused {
        theme.normal_style()
    } else if title.is_empty() {
        theme.muted_style()
    } else {
        theme.normal_style()
    };

    // Epic field
    let epic_label_style = if epic_focused { focus_style } else { normal_label_style };
    let epic_value_style = if epic_focused {
        theme.focus_style()
    } else {
        theme.normal_style()
    };
    let epic_arrows = if epic_focused { ("◄ ", " ►") } else { ("  ", "  ") };

    // Priority field
    let prio_label_style = if priority_focused { focus_style } else { normal_label_style };
    let prio_value_style = if priority_focused {
        theme.focus_style()
    } else {
        theme.normal_style()
    };
    let prio_arrows = if priority_focused { ("◄ ", " ►") } else { ("  ", "  ") };

    let mut lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Title:    ", title_label_style),
            Span::styled(title_value, title_value_style),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Epic:     ", epic_label_style),
            Span::styled(epic_arrows.0, theme.muted_style()),
            Span::styled(epic_label, epic_value_style),
            Span::styled(epic_arrows.1, theme.muted_style()),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Priority: ", prio_label_style),
            Span::styled(prio_arrows.0, theme.muted_style()),
            Span::styled(priority_label, prio_value_style),
            Span::styled(prio_arrows.1, theme.muted_style()),
        ]),
        Line::from(""),
    ];

    // Error message row
    if let Some(err) = error {
        lines.push(Line::from(Span::styled(
            format!("  ✗ {}", err),
            Style::default().fg(theme.error()),
        )));
        lines.push(Line::from(""));
    } else {
        lines.push(Line::from(""));
        lines.push(Line::from(""));
    }

    lines.push(Line::from(vec![
        Span::styled("  Tab", Style::default().fg(theme.primary())),
        Span::styled(":next field  ", theme.muted_style()),
        Span::styled("Enter", Style::default().fg(theme.success())),
        Span::styled(":create  ", theme.muted_style()),
        Span::styled("Esc", Style::default().fg(theme.error())),
        Span::styled(":cancel", theme.muted_style()),
    ]));

    let para = Paragraph::new(lines)
        .alignment(Alignment::Left)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(" New Task ", theme.title_style()))
                .border_style(theme.modal_border_style()),
        );

    f.render_widget(para, popup);
}
