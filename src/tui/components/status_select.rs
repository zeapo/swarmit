use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState},
    Frame,
};

use super::help::centered_rect;
use crate::tui::app::App;
use crate::models::Status;

/// The ordered list of statuses shown in the StatusSelect dialog.
pub const STATUS_OPTIONS: &[Status] = &[
    Status::Todo,
    Status::InProgress,
    Status::Blocked,
    Status::Done,
    Status::Cancelled,
];

fn status_label(s: &Status) -> &'static str {
    match s {
        Status::Todo => "Todo",
        Status::InProgress => "In Progress",
        Status::Blocked => "Blocked",
        Status::Done => "Done",
        Status::Cancelled => "Cancelled",
    }
}

/// Renders a small centered status selection dialog.
pub fn render(f: &mut Frame, app: &App, selected_index: usize, area: Rect) {
    let current_status = app.selected_task_status();

    let popup = centered_rect(24, (STATUS_OPTIONS.len() as u16) + 2, area);
    f.render_widget(Clear, popup);

    let items: Vec<ListItem> = STATUS_OPTIONS
        .iter()
        .enumerate()
        .map(|(i, status)| {
            let is_active = current_status.as_ref() == Some(status);
            let prefix = if is_active { "● " } else { "  " };
            let style = if i == selected_index {
                app.theme.selected_style()
            } else {
                Style::default()
            };
            ListItem::new(Line::from(Span::styled(
                format!("{}{}", prefix, status_label(status)),
                style,
            )))
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(selected_index));

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(" Status ", app.theme.title_style()))
                .border_style(app.theme.modal_border_style()),
        )
        .highlight_style(Style::default());

    f.render_stateful_widget(list, popup, &mut state);
}
