use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState},
    Frame,
};

use super::help::centered_rect;
use crate::app::{App, SORT_OPTIONS};

/// Renders a small centered sort selection dialog.
pub fn render(f: &mut Frame, app: &App, selected_index: usize, area: Rect) {
    let popup = centered_rect(28, (SORT_OPTIONS.len() as u16) + 2, area);
    f.render_widget(Clear, popup);

    let items: Vec<ListItem> = SORT_OPTIONS
        .iter()
        .enumerate()
        .map(|(i, opt)| {
            let is_active = app.dashboard_sort == *opt;
            let prefix = if is_active { "● " } else { "  " };
            let style = if i == selected_index {
                app.theme.selected_style()
            } else {
                Style::default()
            };
            ListItem::new(Line::from(Span::styled(
                format!("{}{}", prefix, opt.label()),
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
                .title(Span::styled(" Sort ", app.theme.title_style()))
                .border_style(app.theme.modal_border_style()),
        )
        .highlight_style(Style::default());

    f.render_stateful_widget(list, popup, &mut state);
}
