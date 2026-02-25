use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState},
    Frame,
};

use crate::app::{App, FILTER_OPTIONS};
use super::help::centered_rect;

const LABELS: &[&str] = &["All", "Todo", "In Progress", "Blocked", "Done", "Cancelled"];

/// Renders a small centered filter selection dialog.
pub fn render(f: &mut Frame, app: &App, selected_index: usize, area: Rect) {
    let popup = centered_rect(24, (FILTER_OPTIONS.len() as u16) + 2, area);
    f.render_widget(Clear, popup);

    let items: Vec<ListItem> = LABELS
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let is_active = app.dashboard_filter == FILTER_OPTIONS[i];
            let prefix = if is_active { "● " } else { "  " };
            let style = if i == selected_index {
                app.theme.selected_style()
            } else {
                Style::default()
            };
            ListItem::new(Line::from(Span::styled(
                format!("{}{}", prefix, label),
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
                .title(Span::styled(" Filter ", app.theme.title_style()))
                .border_style(app.theme.modal_border_style()),
        )
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));

    f.render_stateful_widget(list, popup, &mut state);
}
