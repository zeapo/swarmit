use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState},
    Frame,
};

use super::help::centered_rect;
use crate::tui::app::App;
use crate::models::ItemId;

/// Builds the list of epic options: "None" at index 0, then all epics sorted by ID.
pub fn epic_options(app: &App) -> Vec<Option<ItemId>> {
    let mut options: Vec<Option<ItemId>> = vec![None];
    let mut epic_ids: Vec<ItemId> = app.state.epics.keys().cloned().collect();
    epic_ids.sort();
    options.extend(epic_ids.into_iter().map(Some));
    options
}

/// Renders a small centered epic selection dialog.
pub fn render(f: &mut Frame, app: &App, selected_index: usize, area: Rect) {
    let options = epic_options(app);
    let current_epic = app.selected_task_epic();

    let popup = centered_rect(36, (options.len() as u16) + 2, area);
    f.render_widget(Clear, popup);

    let items: Vec<ListItem> = options
        .iter()
        .enumerate()
        .map(|(i, opt)| {
            let is_active = current_epic == *opt;
            let prefix = if is_active { "● " } else { "  " };
            let label = match opt {
                None => "(none)".to_string(),
                Some(eid) => {
                    let title = app
                        .state
                        .epics
                        .get(eid)
                        .map(|e| e.title.as_str())
                        .unwrap_or("?");
                    format!("{} {}", eid, title)
                }
            };
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
                .title(Span::styled(" Epic ", app.theme.title_style()))
                .border_style(app.theme.modal_border_style()),
        )
        .highlight_style(Style::default());

    f.render_stateful_widget(list, popup, &mut state);
}
