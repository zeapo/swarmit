use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::app::App;
use crate::events::{Modal, TaskFormField};

const PRIORITIES: &[&str] = &["Low", "Medium", "High", "Urgent"];

/// Renders the task creation form as a full-screen overlay.
pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;

    let Some(Modal::TaskCreate {
        ref title,
        cursor_pos,
        ref description,
        desc_row,
        desc_col,
        epic_index,
        priority_index,
        ref focused_field,
        ref error,
    }) = app.modal
    else {
        return;
    };

    // Clear the entire area and render an outer border
    f.render_widget(Clear, area);

    let outer_block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(" New Task ", theme.title_style()))
        .border_style(theme.modal_border_style());

    let inner = outer_block.inner(area);
    f.render_widget(outer_block, area);

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
    let desc_focused = *focused_field == TaskFormField::Description;
    let epic_focused = *focused_field == TaskFormField::Epic;
    let priority_focused = *focused_field == TaskFormField::Priority;

    let focus_style = theme.focus_style();
    let normal_label_style = theme.unfocused_label_style();

    // Vertical layout: label, input box, gap, label, description box, gap, meta row, error
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // [0] Title label
            Constraint::Length(3), // [1] Title input box
            Constraint::Length(1), // [2] gap
            Constraint::Length(1), // [3] Description label
            Constraint::Min(4),    // [4] Description input box
            Constraint::Length(1), // [5] gap
            Constraint::Length(1), // [6] Meta row (Epic + Priority)
            Constraint::Length(1), // [7] Error row
        ])
        .split(inner);

    // ── Title label ──────────────────────────────────────────────────────────
    let title_label_style = if title_focused { focus_style } else { normal_label_style };
    let title_label = Paragraph::new(Line::from(Span::styled("Title", title_label_style)));
    f.render_widget(title_label, chunks[0]);

    // ── Title input box ───────────────────────────────────────────────────────
    let title_border_style = if title_focused {
        theme.focus_style()
    } else {
        theme.border_style()
    };
    let title_display = if title_focused {
        let mut s = title.clone();
        if cursor_pos <= s.len() {
            s.insert(cursor_pos, '█');
        }
        s
    } else if title.is_empty() {
        "…".to_string()
    } else {
        title.clone()
    };
    let title_text_style = if title.is_empty() && !title_focused {
        theme.muted_style()
    } else {
        theme.normal_style()
    };
    let title_para = Paragraph::new(Line::from(Span::styled(title_display, title_text_style)))
        .block(Block::default().borders(Borders::ALL).border_style(title_border_style));
    f.render_widget(title_para, chunks[1]);

    // ── Description label ─────────────────────────────────────────────────────
    let desc_label_style = if desc_focused { focus_style } else { normal_label_style };
    let desc_label = Paragraph::new(Line::from(Span::styled("Description", desc_label_style)));
    f.render_widget(desc_label, chunks[3]);

    // ── Description input box ─────────────────────────────────────────────────
    let desc_border_style = if desc_focused {
        theme.focus_style()
    } else {
        theme.border_style()
    };

    // Build display lines, inserting block cursor on the active row when focused
    let desc_lines: Vec<Line> = description
        .iter()
        .enumerate()
        .map(|(row_idx, line)| {
            if desc_focused && row_idx == desc_row {
                let mut display = line.clone();
                let insert_pos = desc_col.min(display.len());
                display.insert(insert_pos, '█');
                Line::from(Span::styled(display, theme.normal_style()))
            } else if line.is_empty() {
                Line::from(Span::styled("", theme.normal_style()))
            } else {
                Line::from(Span::styled(line.clone(), theme.normal_style()))
            }
        })
        .collect();

    // Compute scroll offset so cursor line stays visible
    let desc_inner_height = chunks[4].height.saturating_sub(2) as usize; // minus borders
    let scroll_offset = if desc_row >= desc_inner_height {
        desc_row - desc_inner_height + 1
    } else {
        0
    };

    let desc_para = Paragraph::new(desc_lines)
        .block(Block::default().borders(Borders::ALL).border_style(desc_border_style))
        .scroll((scroll_offset as u16, 0));
    f.render_widget(desc_para, chunks[4]);

    // ── Meta row: Epic + Priority ─────────────────────────────────────────────
    let meta_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[6]);

    // Epic selector
    let epic_label_style = if epic_focused { focus_style } else { normal_label_style };
    let epic_value_style = if epic_focused { focus_style } else { theme.normal_style() };
    let epic_arrows = if epic_focused { ("◄ ", " ►") } else { ("  ", "  ") };
    let epic_line = Line::from(vec![
        Span::styled("Epic: ", epic_label_style),
        Span::styled(epic_arrows.0, theme.muted_style()),
        Span::styled(epic_label, epic_value_style),
        Span::styled(epic_arrows.1, theme.muted_style()),
    ]);
    f.render_widget(Paragraph::new(epic_line), meta_chunks[0]);

    // Priority selector
    let prio_label_style = if priority_focused { focus_style } else { normal_label_style };
    let prio_value_style = if priority_focused { focus_style } else { theme.normal_style() };
    let prio_arrows = if priority_focused { ("◄ ", " ►") } else { ("  ", "  ") };
    let prio_line = Line::from(vec![
        Span::styled("Priority: ", prio_label_style),
        Span::styled(prio_arrows.0, theme.muted_style()),
        Span::styled(priority_label, prio_value_style),
        Span::styled(prio_arrows.1, theme.muted_style()),
    ]);
    f.render_widget(Paragraph::new(prio_line), meta_chunks[1]);

    // ── Error row ─────────────────────────────────────────────────────────────
    if let Some(err) = error {
        let err_para = Paragraph::new(Line::from(Span::styled(
            format!("✗ {}", err),
            Style::default().fg(theme.error()),
        )));
        f.render_widget(err_para, chunks[7]);
    }
}
